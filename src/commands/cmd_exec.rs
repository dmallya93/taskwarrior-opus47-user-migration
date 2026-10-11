//! Rust translation of `src/commands/CmdExec.cpp`.
//!
//! `task execute <external command>`: joins the command words with spaces
//! and runs them through the shell with libc `system`, returning its raw
//! wait status as the command status (as C++ does; the process exit code is
//! its low byte). There is no configuration gate: Taskwarrior 3.5.0 has no
//! `allow.execute` setting.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::shared::shared::join;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdExec` (`execute`).
pub struct CmdExec {
    meta: CommandMeta,
}

impl CmdExec {
    /// C++ `CmdExec::CmdExec`.
    pub fn new() -> Self {
        CmdExec {
            meta: CommandMeta {
                keyword: "execute".into(),
                usage: "task          execute <external command>".into(),
                description: "Executes external commands and scripts".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdExec {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdExec {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdExec::execute`: `system (join (" ", words))`, or the error
    /// `Cannot execute an empty command.` and status 1 without words.
    fn execute(&self, context: &mut Context, _output: &mut String) -> Result<i32> {
        let command = join(
            " ",
            &context
                .cli2
                .get_words(&context.config, &mut context.debug_messages),
        );

        if command.is_empty() {
            context.error("Cannot execute an empty command.");
            Ok(1)
        } else {
            Ok(system(&command))
        }
    }
}

/// libc `system (command.c_str ())`. Like `c_str`, the command ends at the
/// first NUL byte (command-line words cannot contain one).
fn system(command: &str) -> i32 {
    let bytes = command.as_bytes();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let c_command = std::ffi::CString::new(&bytes[..end]).unwrap_or_default();
    // SAFETY: `c_command` is a valid NUL-terminated string that outlives the call.
    unsafe { libc::system(c_command.as_ptr()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    #[test]
    fn empty_command_is_an_error() {
        let mut f = CommandFixture::new(&[], "", &["execute"]);
        assert_eq!(f.run(&CmdExec::new()).unwrap(), 1);
        assert_eq!(f.ctx.errors, ["Cannot execute an empty command."]);
    }

    #[test]
    fn runs_words_through_the_shell() {
        let mut f = CommandFixture::new(&[], "", &["execute", "true"]);
        assert_eq!(f.run(&CmdExec::new()).unwrap(), 0);
        assert!(f.ctx.errors.is_empty());
    }

    #[test]
    fn returns_the_raw_wait_status() {
        // `exit 3` gives wait status 3 << 8, as C++ `system` returns it.
        let mut f = CommandFixture::new(&[], "", &["execute", "exit", "3"]);
        assert_eq!(f.run(&CmdExec::new()).unwrap(), 3 << 8);
    }

    #[test]
    fn system_stops_at_nul() {
        assert_eq!(system("exit 0\0exit 1"), 0);
    }
}
