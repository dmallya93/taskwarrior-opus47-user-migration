//! Rust translation of `src/commands/CmdSync.cpp`.
//!
//! `task synchronize`: sync the replica with the first configured backend,
//! in the C++ order `sync.local.server_dir`, `sync.aws.bucket`,
//! `sync.gcp.bucket`, `sync.server.url` (or its deprecated synonym
//! `sync.server.origin`), `sync.git.local_path`. The `[initialize]` word of
//! the usage is accepted as a miscellaneous argument and ignored: Taskwarrior
//! 3.5.0 has no separate initialisation step.

use std::sync::OnceLock;

use regex::Regex;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::configuration::Configuration;
use crate::tdb2::{uuid_from_string, Tdb2};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdSync` (`synchronize`).
pub struct CmdSync {
    meta: CommandMeta,
}

impl CmdSync {
    /// C++ `CmdSync::CmdSync`.
    pub fn new() -> Self {
        CmdSync {
            meta: CommandMeta {
                keyword: "synchronize".into(),
                usage: "task          synchronize [initialize]".into(),
                description: "Synchronizes data with the Taskserver".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Migration,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdSync {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdSync {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdSync::execute`.
    ///
    /// Messages accumulate in a local buffer that becomes `output` only after
    /// the sync succeeded, so a configuration or sync error (C++ `throw
    /// std::string` / `rust::Error`, exit status -1 from `main`) discards the
    /// earlier "sync.server.origin is deprecated" line, as in C++.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let status = 0;
        let avoid_snapshots = false;
        let verbose = context.verbose("sync");

        let num_local_operations = context.tdb2.num_local_operations()?;
        let mut out = sync_with_configured_backend(
            &context.config,
            &mut context.tdb2,
            verbose,
            avoid_snapshots,
        )?;

        if context.config.get_boolean("purge.on-sync") {
            context.tdb2.expire_tasks()?;
        }

        if verbose {
            out.push_str("Success!\n");
            // Taskchampion does not provide a measure of the number of operations received from
            // the server, but we can give some indication of the number sent.
            if num_local_operations != 0 {
                out.push_str(&format!(
                    "Sent {num_local_operations} local operations to the server\n"
                ));
            }
        }

        *output = out;
        Ok(status)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `std::regex_replace (server_url, remove_creds_regex, "$1:****@$3")`
/// with `remove_creds_regex ("^(https?://.+):(.+)@(.+)")`: redact the
/// password of a `user:password@host` URL.
///
/// The libstdc++ ECMAScript `.` matches any byte except `\n` and `\r`, hence
/// the explicit class. Greedy groups pick the last `:` and the last `@`
/// leaving at least one character, in both engines.
fn redact_credentials(server_url: &str) -> String {
    static REMOVE_CREDS: OnceLock<Regex> = OnceLock::new();
    let regex = REMOVE_CREDS.get_or_init(|| {
        Regex::new(r"^(https?://[^\n\r]+):([^\n\r]+)@([^\n\r]+)")
            .expect("the credentials regex is valid")
    });
    regex.replace(server_url, "${1}:****@${3}").into_owned()
}

////////////////////////////////////////////////////////////////////////////////
/// The backend selection of C++ `CmdSync::execute`, from reading the
/// `sync.*` settings to the `replica->sync_to_*` call. Returns the messages
/// written so far ("sync.server.origin is deprecated...", "Syncing with ...").
///
/// # Errors
/// The C++ configuration errors ("sync.aws.region is required", "No sync.*
/// settings are configured. See task-sync(5).", ...) and any sync failure.
fn sync_with_configured_backend(
    config: &Configuration,
    tdb2: &mut Tdb2,
    verbose: bool,
    avoid_snapshots: bool,
) -> Result<String> {
    let mut out = String::new();

    let origin = config.get("sync.server.origin");
    let url = config.get("sync.server.url");
    let server_dir = config.get("sync.local.server_dir");
    let client_id = config.get("sync.server.client_id");
    let aws_bucket = config.get("sync.aws.bucket");
    let gcp_bucket = config.get("sync.gcp.bucket");
    let git_local_path = config.get("sync.git.local_path");
    let encryption_secret = config.get("sync.encryption_secret");

    // sync.server.origin is a deprecated synonym for sync.server.url
    let server_url = if url.is_empty() { origin.clone() } else { url };
    if !origin.is_empty() {
        out.push_str("sync.server.origin is deprecated. Use sync.server.url instead.\n");
    }

    // redact credentials from `server_url`, if present
    let safe_server_url = redact_credentials(&server_url);

    let required = |message: &str| Err(TwError::Message(message.to_string()));

    if !server_dir.is_empty() {
        if verbose {
            out.push_str(&format!("Syncing with {server_dir}\n"));
        }
        tdb2.sync_to_local(&server_dir, avoid_snapshots)?;
    } else if !aws_bucket.is_empty() {
        let aws_region = config.get("sync.aws.region");
        let aws_profile = config.get("sync.aws.profile");
        let aws_access_key_id = config.get("sync.aws.access_key_id");
        let aws_secret_access_key = config.get("sync.aws.secret_access_key");
        let aws_default_credentials = config.get("sync.aws.default_credentials");
        let aws_endpoint_url = config.get("sync.aws.endpoint_url");
        let aws_force_path_style = config.get_boolean("sync.aws.force_path_style");
        if aws_region.is_empty() {
            return required("sync.aws.region is required");
        }
        if encryption_secret.is_empty() {
            return required("sync.encryption_secret is required");
        }

        let using_profile = !aws_profile.is_empty();
        let using_creds = !aws_access_key_id.is_empty() || !aws_secret_access_key.is_empty();
        let using_default = !aws_default_credentials.is_empty();

        if u8::from(using_profile) + u8::from(using_creds) + u8::from(using_default) != 1 {
            return required("exactly one method of specifying AWS credentials is required");
        }

        if verbose {
            out.push_str(&format!("Syncing with AWS bucket {aws_bucket}\n"));
        }

        if using_profile {
            tdb2.sync_to_aws_with_profile(
                &aws_region,
                &aws_bucket,
                &aws_profile,
                &aws_endpoint_url,
                aws_force_path_style,
                &encryption_secret,
                avoid_snapshots,
            )?;
        } else if using_creds {
            tdb2.sync_to_aws_with_access_key(
                &aws_region,
                &aws_bucket,
                &aws_access_key_id,
                &aws_secret_access_key,
                &aws_endpoint_url,
                aws_force_path_style,
                &encryption_secret,
                avoid_snapshots,
            )?;
        } else {
            tdb2.sync_to_aws_with_default_creds(
                &aws_region,
                &aws_bucket,
                &aws_endpoint_url,
                aws_force_path_style,
                &encryption_secret,
                avoid_snapshots,
            )?;
        }
    } else if !gcp_bucket.is_empty() {
        let gcp_credential_path = config.get("sync.gcp.credential_path");
        if encryption_secret.is_empty() {
            return required("sync.encryption_secret is required");
        }
        if verbose {
            out.push_str(&format!("Syncing with GCP bucket {gcp_bucket}\n"));
        }
        tdb2.sync_to_gcp(
            &gcp_bucket,
            &gcp_credential_path,
            &encryption_secret,
            avoid_snapshots,
        )?;
    } else if !server_url.is_empty() {
        if client_id.is_empty() || encryption_secret.is_empty() {
            return required("sync.server.client_id and sync.encryption_secret are required");
        }
        if verbose {
            out.push_str(&format!("Syncing with sync server at {safe_server_url}\n"));
        }
        tdb2.sync_to_remote(
            &server_url,
            uuid_from_string(&client_id)?,
            &encryption_secret,
            avoid_snapshots,
        )?;
    } else if !git_local_path.is_empty() {
        let git_branch = config.get("sync.git.branch");
        let git_remote = config.get("sync.git.remote");
        let git_local_only = config.get_boolean("sync.git.local_only");
        let git_git_path = config.get("sync.git.git_path");
        if git_branch.is_empty() {
            return required("sync.git.branch is required");
        }
        if encryption_secret.is_empty() {
            return required("sync.encryption_secret is required");
        }
        if verbose {
            if !git_remote.is_empty() {
                out.push_str(&format!("Syncing with git remote {git_remote}\n"));
            } else {
                out.push_str(&format!(
                    "Syncing with local git repository at {git_local_path}\n"
                ));
            }
        }
        tdb2.sync_to_git(
            &git_local_path,
            &git_branch,
            &git_remote,
            git_local_only,
            &encryption_secret,
            &git_git_path,
            avoid_snapshots,
        )?;
    } else {
        return required("No sync.* settings are configured. See task-sync(5).");
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z"}"#,
    ];

    /// Runs `synchronize`, returning its output buffer.
    fn sync(f: &mut CommandFixture) -> Result<String> {
        let mut output = String::new();
        let rc = CmdSync::new().execute(&mut f.ctx, &mut output)?;
        assert_eq!(rc, 0);
        Ok(output)
    }

    fn error(f: &mut CommandFixture) -> String {
        sync(f).unwrap_err().to_string()
    }

    #[test]
    fn metadata_matches_cpp() {
        let cmd = CmdSync::new();
        let meta = cmd.meta();
        assert_eq!(meta.keyword, "synchronize");
        assert_eq!(meta.usage, "task          synchronize [initialize]");
        assert_eq!(meta.description, "Synchronizes data with the Taskserver");
        assert!(!meta.read_only);
        assert!(meta.accepts_miscellaneous);
        assert!(!meta.accepts_filter);
        assert!(!meta.accepts_modifications);
        assert!(matches!(meta.category, Category::Migration));
    }

    #[test]
    fn redacts_credentials() {
        assert_eq!(
            redact_credentials("https://user:secret@example.com"),
            "https://user:****@example.com"
        );
        assert_eq!(
            redact_credentials("http://u:p:q@x@host"),
            "http://u:p:****@host"
        );
        assert_eq!(
            redact_credentials("https://example.com"),
            "https://example.com"
        );
        assert_eq!(redact_credentials("ftp://u:p@h"), "ftp://u:p@h");
        assert_eq!(redact_credentials(""), "");
        // `.` does not match a line break.
        assert_eq!(
            redact_credentials("https://u:p@h\nx:y@z"),
            "https://u:****@h\nx:y@z"
        );
    }

    #[test]
    fn no_settings() {
        let mut f = CommandFixture::new(&[], "", &["synchronize"]);
        assert_eq!(
            error(&mut f),
            "No sync.* settings are configured. See task-sync(5)."
        );
    }

    #[test]
    fn aws_configuration_errors() {
        let mut f = CommandFixture::new(&[], "sync.aws.bucket=b\n", &["synchronize"]);
        assert_eq!(error(&mut f), "sync.aws.region is required");
        drop(f);

        let mut f = CommandFixture::new(
            &[],
            "sync.aws.bucket=b\nsync.aws.region=r\n",
            &["synchronize"],
        );
        assert_eq!(error(&mut f), "sync.encryption_secret is required");
        drop(f);

        for creds in [
            "",
            "sync.aws.profile=p\nsync.aws.access_key_id=k\n",
            "sync.aws.secret_access_key=s\nsync.aws.default_credentials=true\n",
            "sync.aws.profile=p\nsync.aws.access_key_id=k\nsync.aws.default_credentials=1\n",
        ] {
            let mut f = CommandFixture::new(
                &[],
                &format!("sync.aws.bucket=b\nsync.aws.region=r\nsync.encryption_secret=e\n{creds}"),
                &["synchronize"],
            );
            assert_eq!(
                error(&mut f),
                "exactly one method of specifying AWS credentials is required",
                "{creds}"
            );
            drop(f);
        }
    }

    #[test]
    fn gcp_requires_encryption_secret() {
        let mut f = CommandFixture::new(&[], "sync.gcp.bucket=b\n", &["synchronize"]);
        assert_eq!(error(&mut f), "sync.encryption_secret is required");
    }

    #[test]
    fn server_requires_client_id_and_secret() {
        for rc in [
            "sync.server.url=http://localhost:1\n",
            "sync.server.url=http://localhost:1\nsync.encryption_secret=e\n",
            "sync.server.origin=http://localhost:1\nsync.server.client_id=x\n",
        ] {
            let mut f = CommandFixture::new(&[], rc, &["synchronize"]);
            assert_eq!(
                error(&mut f),
                "sync.server.client_id and sync.encryption_secret are required",
                "{rc}"
            );
            drop(f);
        }
    }

    #[test]
    fn server_rejects_invalid_client_id() {
        let mut f = CommandFixture::new(
            &[],
            "sync.server.url=http://localhost:1\nsync.server.client_id=nope\nsync.encryption_secret=e\n",
            &["synchronize"],
        );
        assert_eq!(error(&mut f), "nope is not a valid UUID");
    }

    #[test]
    fn git_configuration_errors() {
        let mut f =
            CommandFixture::new(&[], "sync.git.local_path=/nonexistent\n", &["synchronize"]);
        assert_eq!(error(&mut f), "sync.git.branch is required");
        drop(f);

        let mut f = CommandFixture::new(
            &[],
            "sync.git.local_path=/nonexistent\nsync.git.branch=main\n",
            &["synchronize"],
        );
        assert_eq!(error(&mut f), "sync.encryption_secret is required");
    }

    #[test]
    fn backend_precedence_follows_cpp() {
        // A local server_dir wins over every other backend, even incomplete ones.
        let server = tempfile::TempDir::new().unwrap();
        let dir = server.path().to_str().unwrap().to_string();
        let mut f = CommandFixture::new(
            &[],
            &format!(
                "sync.local.server_dir={dir}\nsync.aws.bucket=b\nsync.gcp.bucket=b\nsync.server.url=x\nsync.git.local_path=y\n"
            ),
            &["synchronize"],
        );
        assert_eq!(sync(&mut f).unwrap(), "");
        drop(f);

        // AWS before GCP before the sync server before git.
        let mut f = CommandFixture::new(
            &[],
            "sync.aws.bucket=b\nsync.gcp.bucket=b\nsync.server.url=x\nsync.git.local_path=y\n",
            &["synchronize"],
        );
        assert_eq!(error(&mut f), "sync.aws.region is required");
        drop(f);
        let mut f = CommandFixture::new(
            &[],
            "sync.gcp.bucket=b\nsync.server.url=x\nsync.git.local_path=y\n",
            &["synchronize"],
        );
        assert_eq!(error(&mut f), "sync.encryption_secret is required");
        drop(f);
        let mut f = CommandFixture::new(
            &[],
            "sync.server.url=x\nsync.git.local_path=y\n",
            &["synchronize"],
        );
        assert_eq!(
            error(&mut f),
            "sync.server.client_id and sync.encryption_secret are required"
        );
    }

    #[test]
    fn local_sync_verbose_messages() {
        let server = tempfile::TempDir::new().unwrap();
        let dir = server.path().to_str().unwrap().to_string();
        let mut f = CommandFixture::new(
            SEED,
            &format!("sync.local.server_dir={dir}\nverbose=sync\n"),
            &["synchronize"],
        );
        let sent = f.ctx.tdb2.num_local_operations().unwrap();
        assert!(sent > 0);
        assert_eq!(
            sync(&mut f).unwrap(),
            format!("Syncing with {dir}\nSuccess!\nSent {sent} local operations to the server\n")
        );
        assert_eq!(f.ctx.tdb2.num_local_operations().unwrap(), 0);
        // Nothing left to send: no "Sent" line.
        assert_eq!(
            sync(&mut f).unwrap(),
            format!("Syncing with {dir}\nSuccess!\n")
        );
    }

    #[test]
    fn local_sync_quiet_without_sync_verbosity() {
        let server = tempfile::TempDir::new().unwrap();
        let dir = server.path().to_str().unwrap().to_string();
        let mut f = CommandFixture::new(
            SEED,
            &format!("sync.local.server_dir={dir}\n"),
            &["synchronize"],
        );
        assert_eq!(sync(&mut f).unwrap(), "");
        assert_eq!(f.ctx.tdb2.num_local_operations().unwrap(), 0);
    }

    #[test]
    fn deprecated_origin_message() {
        let server = tempfile::TempDir::new().unwrap();
        let dir = server.path().to_str().unwrap().to_string();
        let mut f = CommandFixture::new(
            &[],
            &format!("sync.local.server_dir={dir}\nsync.server.origin=http://o\n"),
            &["synchronize"],
        );
        assert_eq!(
            sync(&mut f).unwrap(),
            "sync.server.origin is deprecated. Use sync.server.url instead.\n"
        );
    }

    #[test]
    fn purge_on_sync_expires_old_deleted_tasks() {
        let server = tempfile::TempDir::new().unwrap();
        let dir = server.path().to_str().unwrap().to_string();
        let old = r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"old","status":"deleted","entry":"20000101T000000Z","end":"20000101T000000Z","modified":"20000101T000000Z"}"#;
        let mut f = CommandFixture::new(
            &[SEED[0], old],
            &format!("sync.local.server_dir={dir}\npurge.on-sync=1\n"),
            &["synchronize"],
        );
        let uuid = "aaaaaaaa-0000-4000-8000-000000000002";
        assert!(f.ctx.tdb2.get_by_uuid(uuid).unwrap().is_some());
        sync(&mut f).unwrap();
        assert!(f.ctx.tdb2.get_by_uuid(uuid).unwrap().is_none());
    }
}
