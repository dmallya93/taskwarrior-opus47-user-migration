//! Rust translation of `src/commands/CmdConfig.cpp`.
//!
//! Ports both `config` and `_config` (milestone 1, foundation: the reference
//! acceptance tests configure their rc file with `task config`).
//!
//! Each struct below is one C++ class from `CmdConfig.cpp`, carrying the
//! exact metadata of its C++ constructor. The C++ static helpers
//! `CmdConfig::setConfigVariable` / `unsetConfigVariable` (also used by
//! `context` and `news`) are [`CmdConfig::set_config_variable`] and
//! [`CmdConfig::unset_config_variable`]; they take the configuration
//! explicitly instead of reading `Context::getContext().config`.
//!
//! The command words are the MISCELLANEOUS arguments,
//! [`crate::cli2::Cli2::get_words`].

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::fs::File;
use crate::shared::json;
use crate::shared::shared::{confirm, trim_with};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdConfig` (`config`).
pub struct CmdConfig {
    meta: CommandMeta,
}

impl CmdConfig {
    /// C++ `CmdConfig::CmdConfig`.
    pub fn new() -> Self {
        CmdConfig {
            meta: CommandMeta {
                keyword: "config".into(),
                usage: "task          config [name [value | '']]".into(),
                description: "Change settings in the task configuration".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Config,
                ..CommandMeta::default()
            },
        }
    }

    /// C++ static `CmdConfig::setConfigVariable`: sets `name` to the
    /// JSON-encoded `value` in the configuration file `config.file()`.
    ///
    /// Every line that starts with `name=` after trimming spaces is rewritten
    /// (after confirmation, when `confirmation` is set): as in C++ the kept
    /// prefix is the first `name.len() + 1` bytes of the *untrimmed* line,
    /// and a `#` comment anywhere on the line is re-appended after two
    /// spaces. Without a matching line, `name=value` is appended (again
    /// after confirmation). Returns whether the file was rewritten; fails with
    /// `Could not write to '<file>'.` if that fails.
    pub fn set_config_variable(config: &Configuration, name: &str, value: &str, confirmation: bool) -> Result<bool> {
        // Read .taskrc (or equivalent)
        let mut contents = File::read_file_lines(&config.file()).unwrap_or_default();

        let mut found = false;
        let mut change = false;

        let pattern = format!("{name}=");
        for line in contents.iter_mut() {
            // Get l-trimmed version of the line
            let trimmed_line = trim_with(line, " ");

            // If there is a comment on the line, it must follow the pattern.
            let comment = line.find('#');

            // TODO: Use std::regex here
            if trimmed_line.starts_with(&pattern) {
                found = true;
                if !confirmation
                    || confirm(&format(
                        "Are you sure you want to change the value of '{1}' from '{2}' to '{3}'?",
                        &[&name, &config.get(name), &value],
                    ))
                {
                    // line.substr (0, pos + name.length () + 1), with pos == 0.
                    let keep = (name.len() + 1).min(line.len());
                    let mut new_line = String::from_utf8_lossy(&line.as_bytes()[..keep]).into_owned();
                    new_line += &json::encode(value);

                    // Preserve the comment
                    if let Some(comment) = comment {
                        new_line += "  ";
                        new_line += &line[comment..];
                    }

                    // Rewrite the line
                    *line = new_line;
                    change = true;
                }
            }
        }

        // Not found, so append instead.
        if !found
            && (!confirmation
                || confirm(&format(
                    "Are you sure you want to add '{1}' with a value of '{2}'?",
                    &[&name, &value],
                )))
        {
            contents.push(format!("{name}={}", json::encode(value)));
            change = true;
        }

        if change && !File::write_file_lines(&config.file(), &contents, true) {
            return Err(TwError::Message(format("Could not write to '{1}'.", &[&config.file()])));
        }

        Ok(change)
    }

    /// C++ static `CmdConfig::unsetConfigVariable`: removes every line of
    /// the configuration file `config.file()` that starts with `name=` after
    /// trimming spaces (each after confirmation, when `confirmation` is set).
    ///
    /// Returns 0 if a line was removed, 1 if lines matched but none was
    /// removed, 2 if no line matched; fails with `Could not write to
    /// '<file>'.` if rewriting the file fails.
    pub fn unset_config_variable(config: &Configuration, name: &str, confirmation: bool) -> Result<i32> {
        // Read .taskrc (or equivalent)
        let contents = File::read_file_lines(&config.file()).unwrap_or_default();

        let mut found = false;
        let mut change = false;

        let pattern = format!("{name}=");
        let mut kept = Vec::with_capacity(contents.len());
        for line in contents {
            // Get l-trimmed version of the line

            // If there is a comment on the line, it must follow the pattern.
            // TODO: Use std::regex here
            if trim_with(&line, " ").starts_with(&pattern) {
                found = true;

                // Remove name
                if !confirmation || confirm(&format("Are you sure you want to remove '{1}'?", &[&name])) {
                    change = true;
                    continue;
                }
            }

            kept.push(line);
        }

        if change && !File::write_file_lines(&config.file(), &kept, true) {
            return Err(TwError::Message(format("Could not write to '{1}'.", &[&config.file()])));
        }

        Ok(if change && found {
            0
        } else if found {
            1
        } else {
            2
        })
    }
}

impl Default for CmdConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdConfig {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdConfig::execute`:
    ///
    /// - `task config name value...` sets `name` to the words after it,
    ///   joined by single spaces;
    /// - `task config name ''` sets `name` to blank;
    /// - `task config name` removes `name` (`No entry named '<name>' found.`
    ///   if there is no such line).
    ///
    /// Each change is confirmed when `rc.confirmation` is on. Reports
    /// `Config file <file> modified.` or `No changes made.`; the status is 1
    /// when a removal was declined, otherwise 0.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut rc = 0;
        let mut out = String::new();

        // Get the non-attribute, non-fancy command line arguments.
        let words = context.cli2.get_words(&context.config, &mut context.debug_messages);

        // Support:
        //   task config name value    # set name to value
        //   task config name ""       # set name to blank
        //   task config name          # remove name
        if words.is_empty() {
            return Err(TwError::Message("Specify the name of a config variable to modify.".to_string()));
        }

        let confirmation = context.config.get_boolean("confirmation");
        let mut found = false;

        let name = &words[0];

        // Join the remaining words into config variable's value
        let value = words[1..].join(" ");

        if name.is_empty() {
            return Err(TwError::Message("Specify the name of a config variable to modify.".to_string()));
        }

        let mut change = false;

        // task config name value
        // task config name ""
        if words.len() > 1 {
            change = CmdConfig::set_config_variable(&context.config, name, &value, confirmation)?;
        }
        // task config name
        else {
            rc = CmdConfig::unset_config_variable(&context.config, name, confirmation)?;
            if rc == 0 {
                change = true;
                found = true;
            } else if rc == 1 {
                found = true;
            }

            if !found {
                return Err(TwError::Message(format("No entry named '{1}' found.", &[name])));
            }
        }

        // Show feedback depending on whether .taskrc has been rewritten
        if change {
            out += &format("Config file {1} modified.", &[&context.config.file()]);
            out.push('\n');
        } else {
            out += "No changes made.\n";
        }

        *output = out;
        Ok(rc)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionConfig` (`_config`).
pub struct CmdCompletionConfig {
    meta: CommandMeta,
}

impl CmdCompletionConfig {
    /// C++ `CmdCompletionConfig::CmdCompletionConfig`.
    pub fn new() -> Self {
        CmdCompletionConfig {
            meta: CommandMeta {
                keyword: "_config".into(),
                usage: "task          _config".into(),
                description: "Lists all supported configuration variables, for completion purposes".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionConfig {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionConfig::execute`: every configuration variable name,
    /// sorted, one per line.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut configs = context.config.all();
        configs.sort();

        for config in &configs {
            output.push_str(config);
            output.push('\n');
        }

        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::configuration::NO_SEARCH_PATHS;

    /// A configuration loaded from `dir/rc` holding `contents`.
    fn config_from(dir: &std::path::Path, contents: &str) -> (Configuration, String) {
        let rc = dir.join("rc").to_str().expect("utf-8").to_string();
        std::fs::write(&rc, contents).expect("write rc");
        let mut config = Configuration::new();
        config.load(&rc, 1, NO_SEARCH_PATHS).expect("load");
        (config, rc)
    }

    fn read(rc: &str) -> String {
        std::fs::read_to_string(rc).expect("read rc")
    }

    /// A Context whose rc file holds `contents`, with `task <args>` parsed.
    fn context_with(dir: &std::path::Path, contents: &str, args: &[&str]) -> (Context, String) {
        let (config, rc) = config_from(dir, contents);
        let mut context = Context::new();
        context.config = config;
        crate::commands::command::factory(&mut context.commands, &Configuration::new()).expect("factory");
        context.cli2.entity("cmd", "config");
        context.cli2.entity("cmd", "_config");
        for arg in args {
            context.cli2.add(arg);
        }
        crate::cli2::with_cli2(&mut context, |cli2, ctx| cli2.analyze(ctx)).expect("analyze");
        context.debug_messages.clear();
        (context, rc)
    }

    #[test]
    fn set_config_variable_rewrites_appends_and_keeps_comments() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (config, rc) = config_from(dir.path(), "a=1 # note\n# a=commented\nb=2\n  a=indented\nxa=1\n");

        assert!(CmdConfig::set_config_variable(&config, "a", "new", false).unwrap());
        // The comment follows two spaces; the kept prefix comes from the
        // untrimmed line, so leading spaces eat into the name (C++ quirk).
        assert_eq!(read(&rc), "a=new  # note\n# a=commented\nb=2\n  new\nxa=1\n");

        assert!(CmdConfig::set_config_variable(&config, "c", "x\"y\\z/", false).unwrap());
        assert_eq!(
            read(&rc),
            "a=new  # note\n# a=commented\nb=2\n  new\nxa=1\nc=x\\\"y\\\\z\\/\n",
            "values are JSON-encoded"
        );

        assert!(CmdConfig::set_config_variable(&config, "b", "", false).unwrap());
        assert!(read(&rc).contains("\nb=\n"), "blank value");
    }

    #[test]
    fn set_config_variable_creates_missing_file_and_reports_write_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = Configuration::new();
        let fresh = dir.path().join("fresh").to_str().expect("utf-8").to_string();
        config.load(&fresh, 1, NO_SEARCH_PATHS).expect("missing file loads empty");
        assert!(CmdConfig::set_config_variable(&config, "k", "v", false).unwrap());
        assert_eq!(read(&fresh), "k=v\n");

        let bad = dir.path().join("missing/dir/rc").to_str().expect("utf-8").to_string();
        config.load(&bad, 1, NO_SEARCH_PATHS).expect("missing file loads empty");
        let err = CmdConfig::set_config_variable(&config, "k", "v", false).unwrap_err();
        assert_eq!(err.to_string(), format!("Could not write to '{bad}'."));
        let status = CmdConfig::unset_config_variable(&config, "k", false).unwrap();
        assert_eq!(status, 2, "nothing to remove, so nothing is written");
    }

    #[test]
    fn unset_config_variable_status_codes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (config, rc) = config_from(dir.path(), "a=1\n# a=c\n  a=2 # x\nab=3\nxa=4\n");

        assert_eq!(CmdConfig::unset_config_variable(&config, "a", false).unwrap(), 0);
        assert_eq!(read(&rc), "# a=c\nab=3\nxa=4\n");
        assert_eq!(CmdConfig::unset_config_variable(&config, "a", false).unwrap(), 2);
        assert_eq!(read(&rc), "# a=c\nab=3\nxa=4\n");
    }

    #[test]
    fn execute_sets_joins_blanks_and_removes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut context, rc) = context_with(
            dir.path(),
            "confirmation=off\n",
            &["task", "rc.confirmation=off", "config", "foo", "bar", "baz"],
        );
        let mut output = String::new();
        assert_eq!(CmdConfig::new().execute(&mut context, &mut output).unwrap(), 0);
        assert_eq!(output, format!("Config file {rc} modified.\n"));
        assert_eq!(read(&rc), "confirmation=off\nfoo=bar baz\n");

        let (mut context, _) = context_with(dir.path(), &read(&rc), &["task", "config", "--", "foo", ""]);
        let mut output = String::new();
        assert_eq!(CmdConfig::new().execute(&mut context, &mut output).unwrap(), 0);
        assert_eq!(read(&rc), "confirmation=off\nfoo=\n");

        let (mut context, _) = context_with(dir.path(), &read(&rc), &["task", "config", "foo"]);
        let mut output = String::new();
        assert_eq!(CmdConfig::new().execute(&mut context, &mut output).unwrap(), 0);
        assert_eq!(output, format!("Config file {rc} modified.\n"));
        assert_eq!(read(&rc), "confirmation=off\n");

        let (mut context, _) = context_with(dir.path(), &read(&rc), &["task", "config", "foo"]);
        let mut output = String::new();
        let err = CmdConfig::new().execute(&mut context, &mut output).unwrap_err();
        assert_eq!(err.to_string(), "No entry named 'foo' found.");
        assert_eq!(output, "");

        for args in [&["task", "config"][..], &["task", "config", ""][..], &["task", "config", "", "x"][..]] {
            let (mut context, _) = context_with(dir.path(), &read(&rc), args);
            let err = CmdConfig::new().execute(&mut context, &mut String::new()).unwrap_err();
            assert_eq!(err.to_string(), "Specify the name of a config variable to modify.", "{args:?}");
        }
        assert_eq!(read(&rc), "confirmation=off\n");
    }

    #[test]
    fn completion_config_lists_sorted_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut context, _) = context_with(dir.path(), "zeta=1\nalpha=2\nmid.dle=3\n", &["task", "_config"]);
        let mut output = String::new();
        assert_eq!(CmdCompletionConfig::new().execute(&mut context, &mut output).unwrap(), 0);
        assert_eq!(output, "alpha\nmid.dle\nzeta\n");
    }
}
