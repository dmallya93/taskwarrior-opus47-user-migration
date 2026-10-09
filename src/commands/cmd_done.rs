//! Rust translation of `src/commands/CmdDone.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdDone.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDone` (`done`).
pub struct CmdDone {
    meta: CommandMeta,
}

impl CmdDone {
    /// C++ `CmdDone::CmdDone`.
    pub fn new() -> Self {
        CmdDone {
            meta: CommandMeta {
                keyword: "done".into(),
                usage: "task <filter> done <mods>".into(),
                description: "Marks the specified task as completed".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: true,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdDone {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDone {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDone::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
