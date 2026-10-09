//! Rust translation of `src/commands/CmdPurge.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdPurge.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdPurge` (`purge`).
pub struct CmdPurge {
    meta: CommandMeta,
}

impl CmdPurge {
    /// C++ `CmdPurge::CmdPurge`.
    pub fn new() -> Self {
        CmdPurge {
            meta: CommandMeta {
                keyword: "purge".into(),
                usage: "task <filter> purge".into(),
                description: "Removes the specified tasks from the data files. Causes permanent loss of data.".into(),
                read_only: false,
                displays_id: false,
                needs_confirm: true,
                needs_gc: true,
                needs_recur_update: false,
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

impl Default for CmdPurge {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdPurge {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdPurge::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
