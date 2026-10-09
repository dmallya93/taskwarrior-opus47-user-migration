//! Rust translation of `src/commands/CmdUndo.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdUndo.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUndo` (`undo`).
pub struct CmdUndo {
    meta: CommandMeta,
}

impl CmdUndo {
    /// C++ `CmdUndo::CmdUndo`.
    pub fn new() -> Self {
        CmdUndo {
            meta: CommandMeta {
                keyword: "undo".into(),
                usage: "task          undo".into(),
                description: "Reverts the most recent change to a task".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdUndo {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUndo {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUndo::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
