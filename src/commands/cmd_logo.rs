//! Rust translation of `src/commands/CmdLogo.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdLogo.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

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

    /// C++ `CmdLogo::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
