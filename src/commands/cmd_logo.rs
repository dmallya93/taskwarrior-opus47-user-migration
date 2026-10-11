//! Rust translation of `src/commands/CmdLogo.cpp`.
//!
//! `task logo`: the Taskwarrior logo drawn with 256-colour background
//! blocks (each data row mirrored), indented by `indent.report`. Fails when
//! colour is off.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::util::optional_blank_line;

/// C++ `data` in `CmdLogo::execute`: the left half of each logo line; `.`
/// is blank, any other byte is colour `byte + 167`. (The C++ array ends
/// with an empty-string sentinel, which the slice length replaces.)
const DATA: [&[u8; 14]; 28] = [
    b".........ABDEF",
    b".......BDILNMM",
    b".....ACJNLHFEE",
    b"....AFMMFDEEEE",
    b"...AFOIDEEEEDD",
    b"..AFOHDEEEDIOR",
    b"..DMIDEEEEOXXX",
    b".BJMDEEEDMXXXX",
    b".ENFEEEEFVXXXX",
    b"BILDEEEEJXXXXX",
    b"DLHEEEEDLXXXXX",
    b"GMFEEEEDKXXXXX",
    b"GMEEEEEEGTIKOR",
    b"GMEEEEEEETODDD",
    b"GMEEEEEEETXRGE",
    b"GMEEEEEEFVXXSE",
    b"ENFEEEEEHWXXUE",
    b"CLHEEEEDLXXXUE",
    b"AILDEEEDRXXXUE",
    b".DNFEEEEPXXXUE",
    b".BJMDEEEEPXXUE",
    b"..DMIDEEEDLXUE",
    b"..AFOHDEEEDHME",
    b"...AFOIDEEEEDE",
    b"....AFMMFDEEEE",
    b".....ACJNLHFEE",
    b".......BDILNMM",
    b".........ABDEF",
];

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdLogo` (`logo`).
pub struct CmdLogo {
    meta: CommandMeta,
}

impl CmdLogo {
    /// C++ `CmdLogo::CmdLogo`.
    pub fn new() -> Self {
        CmdLogo {
            meta: CommandMeta {
                keyword: "logo".into(),
                usage: "task          logo".into(),
                description: "Displays the Taskwarrior logo".into(),
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

impl Default for CmdLogo {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdLogo {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdLogo::execute`: each line is the 14 data cells followed by
    /// the same cells reversed, every non-`.` cell a two-space block with
    /// background colour `cell + 167`.
    ///
    /// # Errors
    /// `The logo command requires that color support is enabled.` when
    /// colour is off.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        if !context.color() {
            return Err(TwError::Message(
                "The logo command requires that color support is enabled.".into(),
            ));
        }

        // C++ `std::string indent (getInteger ("indent.report"), ' ')`.
        // Deviation: a negative `indent.report` gives no indent here, where
        // the C++ `std::string` constructor would throw `std::length_error`.
        let indent =
            " ".repeat(usize::try_from(context.config.get_integer("indent.report")).unwrap_or(0));
        output.push_str(optional_blank_line(context));

        for line in DATA {
            output.push_str(&indent);

            for &cell in line.iter() {
                push_cell(output, cell);
            }

            for &cell in line.iter().rev() {
                push_cell(output, cell);
            }

            output.push('\n');
        }

        output.push_str(optional_blank_line(context));
        Ok(0)
    }
}

/// One logo cell: two spaces for `.`, otherwise C++
/// `snprintf (block, 24, "\033[48;5;%dm  \033[0m", value + 167)`.
fn push_cell(output: &mut String, cell: u8) {
    let value = i32::from(cell);
    if value == i32::from(b'.') {
        output.push_str("  ");
    } else {
        let value = value + 167;
        output.push_str(&format!("\x1b[48;5;{value}m  \x1b[0m"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::set_stdout_tty_override;

    fn context(settings: &[(&str, &str)]) -> Context {
        set_stdout_tty_override(Some(false));
        let mut context = Context::new();
        context
            .config
            .parse(
                crate::context::CONFIGURATION_DEFAULTS,
                1,
                &[crate::context::task_rcdir()],
                "",
            )
            .unwrap();
        for (key, value) in settings {
            context.config.set(key, *value);
        }
        crate::rules::initialize_color_rules(&mut context).unwrap();
        context
    }

    #[test]
    fn requires_colour() {
        let mut context = context(&[]);
        let mut output = String::new();
        let err = CmdLogo::new()
            .execute(&mut context, &mut output)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "The logo command requires that color support is enabled."
        );
        assert_eq!(output, "");
    }

    #[test]
    fn draws_mirrored_colour_blocks() {
        let mut context = context(&[("_forcecolor", "on"), ("indent.report", "2")]);
        let mut output = String::new();
        let rc = CmdLogo::new().execute(&mut context, &mut output).unwrap();
        assert_eq!(rc, 0);
        let lines: Vec<&str> = output.split('\n').collect();
        // Blank line, 28 logo lines, blank line, and the final empty split.
        assert_eq!(lines.len(), 31);
        assert_eq!(lines[0], "");
        assert_eq!(lines[29], "");
        let block = |v: u8| format!("\x1b[48;5;{}m  \x1b[0m", i32::from(v) + 167);
        let first = format!(
            "  {}{}{}{}{}{}{}{}{}{}",
            "  ".repeat(9),
            block(b'A'),
            block(b'B'),
            block(b'D'),
            block(b'E'),
            block(b'F'),
            block(b'F'),
            block(b'E'),
            block(b'D'),
            block(b'B'),
        ) + &block(b'A')
            + &"  ".repeat(9);
        assert_eq!(lines[1], first);
        assert_eq!(lines[1], lines[28]);
        assert!(output.contains("\x1b[48;5;255m  \x1b[0m"), "X is 88 + 167");
    }

    #[test]
    fn no_blank_lines_without_blank_verbosity() {
        let mut context = context(&[("_forcecolor", "on"), ("verbose", "nothing")]);
        let mut output = String::new();
        CmdLogo::new().execute(&mut context, &mut output).unwrap();
        assert!(
            output.starts_with("                  \x1b[48;5;"),
            "{output:?}"
        );
        assert_eq!(output.lines().count(), 28);
    }
}
