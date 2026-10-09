//! Rust translation of `src/commands/CmdHelp.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdHelp.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHelp` (`help`).
pub struct CmdHelp {
    meta: CommandMeta,
}

impl CmdHelp {
    /// C++ `CmdHelp::CmdHelp`.
    pub fn new() -> Self {
        CmdHelp {
            meta: CommandMeta {
                keyword: "help".into(),
                usage: "task          help ['usage']".into(),
                description: "Displays this usage help text".into(),
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

impl Default for CmdHelp {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHelp {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHelp::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
