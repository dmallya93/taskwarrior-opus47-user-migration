//! Rust translation of `src/commands/CmdDiagnostics.cpp`.
//!
//! Ported in milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdDiagnostics.cpp`, carrying the exact
//! metadata of its C++ constructor.
//!
//! The section and label layout is the C++ one. Values the C++ takes from
//! compiler and CMake macros are replaced by their honest Rust equivalents,
//! captured by `build.rs` (see [`crate::version`]):
//!
//! - `Version:` is `rustc --version` plus the target triple (C++ `__VERSION__`);
//! - `Caps:` lists the POSIX levels of the C library (`sysconf`, for the C++
//!   `_POSIX_VERSION` / `_POSIX2_C_VERSION` macros), the `ILP32`/`LP64` data
//!   model and the C type sizes; the C-compiler-only `+stdc`, `+stdc_hosted`
//!   and `__STDC_VERSION__` entries have no Rust equivalent and are omitted;
//! - `Compliance:` stays [`cpp_compliance`] (the `C++17` level of the
//!   reference build, which `diag.test.py` asserts);
//! - `CMake:` is `-` (no CMake) followed by the cargo version;
//! - `libuuid:` names the `uuid` crate, which formats lowercase;
//! - `Build type:` is the cargo profile followed by `+<feature>` for each
//!   enabled cargo feature.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::format::format;
use crate::shared::fs::{File, Path};
use crate::shared::shared::{cpp_compliance, os_name};
use crate::version::{CARGO_VERSION, COMMIT, FEATURES, PACKAGE_STRING, PROFILE, RUSTC_VERSION, TARGET};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDiagnostics` (`diagnostics`).
pub struct CmdDiagnostics {
    meta: CommandMeta,
}

impl CmdDiagnostics {
    /// C++ `CmdDiagnostics::CmdDiagnostics`.
    pub fn new() -> Self {
        CmdDiagnostics {
            meta: CommandMeta {
                keyword: "diagnostics".into(),
                usage: "task          diagnostics".into(),
                description: "Platform, build and environment details".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDiagnostics {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDiagnostics::execute`: output intended to help diagnose
    /// problems — platform, compiler, build, configuration, hooks, terminal
    /// size and a broken-reference scan of every task.
    ///
    /// As in C++, `std::setbase (8)` set for the `mode` values stays in effect
    /// on the stream, so the terminal dimensions are printed in octal.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut bold = Color::new();
        if context.color() {
            bold = Color::parse("bold")?;
        }

        let mut out = String::new();
        out.push('\n');
        out.push_str(&bold.colorize(PACKAGE_STRING));
        out.push('\n');

        out.push_str(&format!("   Platform: {}\n\n", os_name()));

        // Compiler.
        out.push_str(&bold.colorize("Compiler"));
        out.push('\n');
        if !RUSTC_VERSION.is_empty() {
            out.push_str(&format!("    Version: {}\n", compiler_version()));
        }
        out.push_str(&format!("       Caps:{}\n", caps()));

        // Compiler compliance level.
        out.push_str(&format!(" Compliance: {}\n\n", cpp_compliance()));

        out.push_str(&bold.colorize("Build Features"));
        out.push('\n');
        if let Some(commit) = COMMIT {
            out.push_str(&format!("     Commit: {commit}\n"));
        }
        out.push_str(&format!("      CMake: {}\n", cmake_version()));

        out.push_str("    libuuid: uuid crate + lowercase hyphenated\n");

        out.push_str(&format!(" Build type: {}\n\n", build_type()));

        // Config: .taskrc found, readable, writable
        let rc_file = File::new(&context.config.file());
        out.push_str(&bold.colorize("Configuration"));
        out.push('\n');
        out.push_str(&format!(
            "       File: {} {}, {} bytes, mode {:o}\n",
            rc_file.data,
            if rc_file.exists() { "(found)" } else { "(missing)" },
            rc_file.size()?,
            rc_file.mode()?
        ));

        // Config: data.location found, readable, writable
        let location = File::new(&context.config.get("data.location"));
        out.push_str(&format!(
            "       Data: {} {}, {}, mode {:o}\n",
            location.data,
            if location.exists() { "(found)" } else { "(missing)" },
            if location.is_directory()? { "dir" } else { "?" },
            location.mode()?
        ));

        if let Some(env) = getenv("TASKRC") {
            out.push_str(&format!("     TASKRC: {env}\n"));
        }

        if let Some(env) = getenv("TASKDATA") {
            out.push_str(&format!("   TASKDATA: {env}\n"));
        }

        out.push_str(&format!(
            "         GC: {}\n",
            if context.config.get_boolean("gc") {
                "Enabled"
            } else {
                "Disabled"
            }
        ));

        // Determine rc.editor/$EDITOR/$VISUAL.
        if !context.config.get("editor").is_empty() {
            out.push_str(&format!("  rc.editor: {}\n", context.config.get("editor")));
        } else if let Some(peditor) = getenv("VISUAL") {
            out.push_str(&format!("    $VISUAL: {peditor}\n"));
        } else if let Some(peditor) = getenv("EDITOR") {
            out.push_str(&format!("    $EDITOR: {peditor}\n"));
        }

        // Display hook status.
        let hook_location = if context.config.has("hooks.location") {
            Path::new(&context.config.get("hooks.location"))
        } else {
            let mut location = Path::new(&context.config.get("data.location"));
            location += "hooks";
            location
        };

        out.push_str(&bold.colorize("Hooks"));
        out.push('\n');
        out.push_str(&format!(
            "     System: {}\n",
            if context.config.get_boolean("hooks") {
                "Enabled"
            } else {
                "Disabled"
            }
        ));
        out.push_str(&format!("   Location: {}\n", hook_location.data));

        let hooks = context.hooks.list();
        if !hooks.is_empty() {
            let mut longest: usize = 0;
            for hook in &hooks {
                if hook.len() > longest {
                    longest = hook.len();
                }
            }
            // C++ subtracts from an `unsigned int`; every script lies inside
            // the hook location, so this cannot underflow there. Saturating
            // avoids a 4 GiB padding should it ever do so.
            longest = longest.saturating_sub(hook_location.data.len() + 1);

            out.push_str("     Active: ");
            let mut count = 0;
            for hook in &hooks {
                let p = Path::new(hook);
                if !p.is_directory()? {
                    let name = p.name();

                    if p.executable()? && is_hook_name(&name) {
                        out.push_str(if count > 0 { "             " } else { "" });
                        count += 1;

                        out.push_str(&pad_left(&name, longest));
                        out.push_str(" (executable)");
                        out.push_str(if p.is_link()? { " (symlink)" } else { "" });
                        out.push('\n');
                    }
                }
            }

            if count == 0 {
                out.push('\n');
            }

            out.push_str("   Inactive: ");
            count = 0;
            for hook in &hooks {
                let p = Path::new(hook);
                if !p.is_directory()? {
                    let name = p.name();

                    let executable = p.executable()?;
                    if !executable || !is_hook_name(&name) {
                        out.push_str(if count > 0 { "             " } else { "" });
                        count += 1;

                        out.push_str(&pad_left(&name, longest));
                        out.push_str(if executable {
                            " (executable)"
                        } else {
                            " (not executable)"
                        });
                        out.push_str(if p.is_link()? { " (symlink)" } else { "" });
                        out.push_str(if is_hook_name(&name) {
                            ""
                        } else {
                            "unrecognized hook name"
                        });
                        out.push('\n');
                    }
                }
            }

            if count == 0 {
                out.push('\n');
            }
        } else {
            out.push_str("             (-none-)\n");
        }

        out.push('\n');

        // Verify UUIDs are all unique.
        out.push_str(&bold.colorize("Tests"));
        out.push('\n');

        // Report terminal dimensions (in octal: the stream base is still 8).
        let width = context.get_width();
        let height = context.get_height();
        out.push_str(&format!("   Terminal: {width:o}x{height:o}\n"));

        // Check all the UUID references
        let all = context.tdb2.all_tasks()?;

        let mut no_broken_refs = true;
        out.push_str(" Broken ref: ");
        out.push_str(&format(
            "Scanned {1} tasks for broken references:",
            &[&all.len()],
        ));
        out.push('\n');

        for task in &all {
            // Check dependencies
            for uuid in task.get_dependency_uuids() {
                if !context.tdb2.has(&uuid)? {
                    out.push_str("             ");
                    out.push_str(&format(
                        "Task {1} depends on nonexistent task: {2}",
                        &[&task.get("uuid"), &uuid],
                    ));
                    out.push('\n');
                    no_broken_refs = false;
                }
            }

            // Check recurrence parent
            let parent_uuid = task.get("parent");

            if !parent_uuid.is_empty() && !context.tdb2.has(&parent_uuid)? {
                out.push_str("             ");
                out.push_str(&format(
                    "Task {1} has nonexistent recurrence template {2}",
                    &[&task.get("uuid"), &parent_uuid],
                ));
                out.push('\n');
                no_broken_refs = false;
            }
        }

        if no_broken_refs {
            out.push_str("             No broken references found\n");
        }

        out.push('\n');
        *output = out;
        Ok(0)
    }
}

/// C++ `getenv`: the variable's value (lossily decoded) if it is set, even
/// when empty.
fn getenv(name: &str) -> Option<String> {
    std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
}

/// The C++ `name.substr (0, 6) == "on-add" || ...` hook name test.
fn is_hook_name(name: &str) -> bool {
    name.starts_with("on-add")
        || name.starts_with("on-modify")
        || name.starts_with("on-launch")
        || name.starts_with("on-exit")
}

/// C++ `out.width (width); out << std::left << text`: pads `text` with
/// spaces on the right to `width` bytes.
fn pad_left(text: &str, width: usize) -> String {
    let mut padded = text.to_string();
    if padded.len() < width {
        padded.push_str(&" ".repeat(width - padded.len()));
    }
    padded
}

/// The `Version:` value: `rustc --version` and the target triple (the C++
/// `__VERSION__` macro).
fn compiler_version() -> String {
    if TARGET.is_empty() {
        RUSTC_VERSION.to_string()
    } else {
        format!("{RUSTC_VERSION}, target {TARGET}")
    }
}

/// The `Caps:` value: the C library POSIX levels, the data model and the C
/// type sizes in bits, each preceded by ` +`.
fn caps() -> String {
    let mut caps = String::new();

    #[cfg(unix)]
    {
        // SAFETY: sysconf has no preconditions.
        let posix_version = unsafe { libc::sysconf(libc::_SC_VERSION) };
        if posix_version > 0 {
            caps.push_str(&format!(" +{posix_version}"));
        }
    }
    // `_SC_2_C_VERSION` is only exposed by the libc crate for these targets.
    #[cfg(any(all(target_os = "linux", target_env = "gnu"), target_os = "android"))]
    {
        // SAFETY: sysconf has no preconditions.
        let posix2_c_version = unsafe { libc::sysconf(libc::_SC_2_C_VERSION) };
        if posix2_c_version > 0 {
            caps.push_str(&format!(" +{posix2_c_version}"));
        }
    }

    let char_bits = 8 * std::mem::size_of::<libc::c_char>();
    let int_bits = 8 * std::mem::size_of::<libc::c_int>();
    let long_bits = 8 * std::mem::size_of::<libc::c_long>();
    let pointer_bits = 8 * std::mem::size_of::<*const libc::c_void>();
    let time_t_bits = 8 * std::mem::size_of::<libc::time_t>();

    if int_bits == 32 && long_bits == 32 && pointer_bits == 32 {
        caps.push_str(" +ILP32");
    }
    if long_bits == 64 && pointer_bits == 64 {
        caps.push_str(" +LP64");
    }

    caps.push_str(&format!(
        " +c{char_bits} +i{int_bits} +l{long_bits} +vp{pointer_bits} +time_t{time_t_bits}"
    ));
    caps
}

/// The `CMake:` value: `-` (there is no CMake), then the cargo version.
fn cmake_version() -> String {
    if CARGO_VERSION.is_empty() {
        "-".to_string()
    } else {
        format!("- ({CARGO_VERSION})")
    }
}

/// The `Build type:` value: the cargo profile (`-` if unknown, as C++ without
/// `CMAKE_BUILD_TYPE`) followed by ` +<feature>` per enabled cargo feature.
fn build_type() -> String {
    let mut build_type = if PROFILE.is_empty() {
        "-".to_string()
    } else {
        PROFILE.to_string()
    };
    for feature in FEATURES.split(',').filter(|f| !f.is_empty()) {
        build_type.push_str(&format!(" +{feature}"));
    }
    build_type
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;
    use std::os::unix::fs::PermissionsExt;

    const RC: &str = "detection=off\ndefaultwidth=80\ndefaultheight=24\neditor=edlin\n";

    fn run(f: &mut CommandFixture) -> String {
        let mut output = String::new();
        let rc = CmdDiagnostics::new()
            .execute(&mut f.ctx, &mut output)
            .unwrap();
        assert_eq!(rc, 0);
        output
    }

    #[test]
    fn helpers() {
        assert_eq!(pad_left("ab", 4), "ab  ");
        assert_eq!(pad_left("abcde", 4), "abcde");
        assert!(is_hook_name("on-add.py"));
        assert!(is_hook_name("on-modify"));
        assert!(is_hook_name("on-launch-x"));
        assert!(is_hook_name("on-exit"));
        assert!(!is_hook_name("on-delete"));
        assert!(caps().ends_with(&format!(
            " +c8 +i32 +l{} +vp{} +time_t64",
            8 * std::mem::size_of::<libc::c_long>(),
            8 * std::mem::size_of::<usize>()
        )));
        assert!(build_type().starts_with(PROFILE));
        assert!(cmake_version().starts_with('-'));
        assert!(!RUSTC_VERSION.is_empty());
        assert!(RUSTC_VERSION.starts_with("rustc "));
        assert!(compiler_version().ends_with(TARGET));
    }

    #[test]
    fn layout_without_hooks_or_tasks() {
        let mut f = CommandFixture::new(&[], RC, &["diagnostics"]);
        let output = run(&mut f);
        let rc = f.ctx.config.file();
        let data = f.ctx.config.get("data.location");

        assert!(output.starts_with("\ntask 3.5.0\n   Platform: "), "{output}");
        assert!(output.contains("\n\nCompiler\n    Version: rustc "), "{output}");
        assert!(output.contains("\n       Caps: +"), "{output}");
        assert!(output.contains(" +time_t64\n Compliance: C++17\n\nBuild Features\n"));
        assert!(output.contains("\n      CMake: -"), "{output}");
        assert!(output.contains("\n    libuuid: uuid crate + lowercase hyphenated\n Build type: "));
        assert!(
            output.contains(&format!(
                "\n\nConfiguration\n       File: {rc} (found), {} bytes, mode 100",
                std::fs::metadata(&rc).unwrap().len()
            )),
            "{output}"
        );
        assert!(
            output.contains(&format!("\n       Data: {data} (found), dir, mode 40")),
            "{output}"
        );
        assert!(output.contains(&format!("\n     TASKRC: {rc}\n")), "{output}");
        assert!(output.contains("\n         GC: Enabled\n  rc.editor: edlin\nHooks\n"));
        assert!(output.ends_with(&format!(
            "Hooks\n     System: Enabled\n   Location: {data}/hooks\n             (-none-)\n\n\
             Tests\n   Terminal: 120x30\n Broken ref: Scanned 0 tasks for broken references:\n\
             \x20            No broken references found\n\n"
        )));
        // No colour without a TTY.
        assert!(!output.contains("\x1b[1m"));
    }

    #[test]
    fn forced_colour_bolds_the_headings() {
        let mut f = CommandFixture::new(
            &[],
            &format!("{RC}_forcecolor=on\n"),
            &["diagnostics"],
        );
        let output = run(&mut f);
        assert!(output.starts_with("\n\x1b[1mtask 3.5.0\x1b[0m\n"), "{output:?}");
        for heading in ["Compiler", "Build Features", "Configuration", "Hooks", "Tests"] {
            assert!(output.contains(&format!("\x1b[1m{heading}\x1b[0m\n")), "{heading}");
        }
    }

    #[test]
    fn hook_scripts_are_classified() {
        let mut f = CommandFixture::new(&[], &format!("{RC}hooks=off\n"), &["diagnostics"]);
        let hooks = format!("{}/hooks", f.ctx.config.get("data.location"));
        std::fs::create_dir(&hooks).unwrap();
        for (name, mode) in [("on-add-a", 0o755), ("on-exit-b", 0o644), ("zz", 0o755)] {
            let path = format!("{hooks}/{name}");
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        std::fs::create_dir(format!("{hooks}/subdir")).unwrap();
        std::os::unix::fs::symlink(format!("{hooks}/on-add-a"), format!("{hooks}/on-modify-l"))
            .unwrap();
        crate::hooks::Hooks::initialize(&mut f.ctx).unwrap();

        let output = run(&mut f);
        assert!(
            output.contains(&format!(
                "Hooks\n     System: Disabled\n   Location: {hooks}\n\
                 \x20    Active: on-add-a    (executable)\n\
                 \x20            on-modify-l (executable) (symlink)\n\
                 \x20  Inactive: on-exit-b   (not executable)\n\
                 \x20            zz          (executable)unrecognized hook name\n\n"
            )),
            "{output}"
        );
    }

    #[test]
    fn hooks_location_without_active_or_inactive_scripts() {
        let mut f = CommandFixture::new(&[], RC, &["diagnostics"]);
        let hooks = format!("{}/elsewhere", f.ctx.config.get("data.location"));
        std::fs::create_dir(&hooks).unwrap();
        let path = format!("{hooks}/on-launch");
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        f.ctx.config.set("hooks.location", &hooks);
        crate::hooks::Hooks::initialize(&mut f.ctx).unwrap();

        let output = run(&mut f);
        assert!(
            output.contains(&format!(
                "   Location: {hooks}\n     Active: on-launch (executable)\n   Inactive: \n\n"
            )),
            "{output}"
        );
    }

    #[test]
    fn broken_references_are_reported() {
        let missing = "00000000-0000-0000-0000-000000000001";
        let orphan = "00000000-0000-0000-0000-000000000002";
        let child = "00000000-0000-0000-0000-000000000003";
        let mut f = CommandFixture::new(
            &[
                &format!(r#"{{"uuid":"{orphan}","description":"a","status":"pending","depends":"{missing}"}}"#),
                &format!(r#"{{"uuid":"{child}","description":"b","status":"pending","parent":"{missing}"}}"#),
            ],
            RC,
            &["diagnostics"],
        );
        let output = run(&mut f);
        let (_, tests) = output.split_once(" Broken ref: ").unwrap();
        // The replica decides the task order.
        let mut lines: Vec<&str> = tests.lines().collect();
        assert_eq!(lines[0], "Scanned 2 tasks for broken references:");
        assert_eq!(&lines[3..], [""]);
        lines[1..3].sort();
        assert_eq!(
            lines[1],
            format!("             Task {orphan} depends on nonexistent task: {missing}")
        );
        assert_eq!(
            lines[2],
            format!("             Task {child} has nonexistent recurrence template {missing}")
        );
        assert!(output.ends_with("\n\n"));
    }
}
