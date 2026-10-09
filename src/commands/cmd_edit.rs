//! Rust translation of `src/commands/CmdEdit.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdEdit.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdEdit` (`edit`).
pub struct CmdEdit {
    meta: CommandMeta,
}

impl CmdEdit {
    /// C++ `CmdEdit::CmdEdit`.
    pub fn new() -> Self {
        CmdEdit {
            meta: CommandMeta {
                keyword: "edit".into(),
                usage: "task <filter> edit".into(),
                description: "Launches an editor to modify a task directly".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdEdit {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdEdit {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdEdit::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
