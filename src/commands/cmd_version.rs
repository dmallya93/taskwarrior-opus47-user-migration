//! Rust translation of `src/commands/CmdVersion.cpp`.
//!
//! Ports both `version` and `_version` (milestone 1, foundation).
//!
//! Each struct below is one C++ class from `CmdVersion.cpp`, carrying the
//! exact metadata of its C++ constructor.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::datetime::Datetime;
use crate::shared::format::format;
use crate::shared::shared::os_name;
use crate::shared::table::Table;
use crate::version::{COMMIT, PACKAGE, VERSION};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdVersion` (`version`).
pub struct CmdVersion {
    meta: CommandMeta,
}

impl CmdVersion {
    /// C++ `CmdVersion::CmdVersion`.
    pub fn new() -> Self {
        CmdVersion {
            meta: CommandMeta {
                keyword: "version".into(),
                usage: "task          version".into(),
                description: "Shows the Taskwarrior version number".into(),
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

impl Default for CmdVersion {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdVersion {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdVersion::execute`: the version banner, copyright, license
    /// and documentation pointers.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Create a table for the disclaimer.
        let width = context.get_width();
        let mut disclaimer = Table::new();
        disclaimer.width(width);
        disclaimer.add("");
        let row = disclaimer.add_row();
        disclaimer.set(
            row,
            0,
            "Taskwarrior may be copied only under the terms of the MIT license, which may be \
             found in the Taskwarrior source kit.",
        );

        // Create a table for the URL.
        let mut link = Table::new();
        link.width(width);
        link.add("");
        let row = link.add_row();
        link.set(
            row,
            0,
            "Documentation for Taskwarrior can be found using 'man task', 'man taskrc', 'man \
             task-color', 'man task-sync' or at https://taskwarrior.org",
        );

        let now = Datetime::new();
        let year = now.year();

        let mut bold = Color::new();
        if context.color() {
            bold = Color::parse("bold")?;
        }

        let mut out = String::new();
        out.push('\n');
        out.push_str(&format(
            "{1} {2} built for ",
            &[&bold.colorize(PACKAGE), &bold.colorize(VERSION)],
        ));
        out.push_str(&os_name());
        out.push('\n');
        out.push_str(&format!("Copyright (C) 2006 - {year} T. Babej, P. Beckingham, F. Hernandez.\n"));
        out.push('\n');
        out.push_str(&disclaimer.render());
        out.push('\n');
        out.push_str(&link.render());
        out.push('\n');

        *output = out;
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionVersion` (`_version`).
pub struct CmdCompletionVersion {
    meta: CommandMeta,
}

impl CmdCompletionVersion {
    /// C++ `CmdCompletionVersion::CmdCompletionVersion`.
    pub fn new() -> Self {
        CmdCompletionVersion {
            meta: CommandMeta {
                keyword: "_version".into(),
                usage: "task          _version".into(),
                description: "Shows only the Taskwarrior version number".into(),
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

impl Default for CmdCompletionVersion {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionVersion {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionVersion::execute`: `VERSION (COMMIT)` with
    /// `HAVE_COMMIT`, otherwise `VERSION`.
    fn execute(&self, _context: &mut Context, output: &mut String) -> Result<i32> {
        *output = completion_version(COMMIT);
        Ok(0)
    }
}

/// The `_version` text for a build with (`Some`) or without (`None`) a
/// commit.
fn completion_version(commit: Option<&str>) -> String {
    let mut output = match commit {
        Some(commit) => format!("{VERSION} ({commit})"),
        None => VERSION.to_string(),
    };
    output.push('\n');
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::set_stdout_tty_override;

    const DISCLAIMER: &str = "Taskwarrior may be copied only under the terms of the MIT license, which may be\n\
                              found in the Taskwarrior source kit.\n";
    const LINK: &str = "Documentation for Taskwarrior can be found using 'man task', 'man taskrc', 'man\n\
                        task-color', 'man task-sync' or at https://taskwarrior.org\n";

    fn context(settings: &[(&str, &str)]) -> Context {
        set_stdout_tty_override(Some(false));
        let mut context = Context::new();
        for (key, value) in [("defaultwidth", "80"), ("detection", "off"), ("color", "on")] {
            context.config.set(key, value);
        }
        for (key, value) in settings {
            context.config.set(key, *value);
        }
        context
    }

    fn version(context: &mut Context) -> String {
        let mut output = String::new();
        assert_eq!(CmdVersion::new().execute(context, &mut output).unwrap(), 0);
        output
    }

    fn year() -> i32 {
        Datetime::new().year()
    }

    #[test]
    fn version_plain_at_width_80() {
        let mut context = context(&[]);
        let output = version(&mut context);
        let expected = format!(
            "\ntask 3.5.0 built for {}\nCopyright (C) 2006 - {} T. Babej, P. Beckingham, F. Hernandez.\n\n{DISCLAIMER}\n{LINK}\n",
            os_name(),
            year()
        );
        assert_eq!(output, expected);
        assert!(!output.contains('\x1b'));
    }

    #[test]
    fn version_bold_with_color() {
        let mut context = context(&[("_forcecolor", "on")]);
        let output = version(&mut context);
        assert!(
            output.starts_with("\n\x1b[1mtask\x1b[0m \x1b[1m3.5.0\x1b[0m built for "),
            "{output:?}"
        );
        assert!(output.ends_with(&format!("\n\n{DISCLAIMER}\n{LINK}\n")));
    }

    #[test]
    fn version_wraps_to_width() {
        let mut context = context(&[("defaultwidth", "40")]);
        let output = version(&mut context);
        assert!(output.contains(
            "\nTaskwarrior may be copied only under the\nterms of the MIT license, which may be\n\
             found in the Taskwarrior source kit.\n\n"
        ));
        assert!(output.ends_with("\nhttps://taskwarrior.org\n\n"), "{output}");
    }

    #[test]
    fn version_detection_without_terminal_is_width_zero() {
        // C++ `getWidth` with `detection` on and no terminal size gives 0,
        // so the tables shrink to their longest word.
        let mut context = context(&[("detection", "on")]);
        context.terminal_width = 0;
        context.terminal_height = 1;
        let output = version(&mut context);
        assert!(output.contains("\nTaskwarrior\nmay be\n"), "{output}");
    }

    #[test]
    fn completion_version_with_and_without_commit() {
        assert_eq!(completion_version(None), "3.5.0\n");
        assert_eq!(completion_version(Some("abc1234")), "3.5.0 (abc1234)\n");
        assert_eq!(completion_version(Some("")), "3.5.0 ()\n");

        let mut context = Context::new();
        let mut output = String::new();
        assert_eq!(CmdCompletionVersion::new().execute(&mut context, &mut output).unwrap(), 0);
        assert_eq!(output, completion_version(COMMIT));
    }
}
