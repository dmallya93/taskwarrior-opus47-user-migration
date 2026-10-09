//! Rust translation of `src/commands/CmdStart.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdStart.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdStart` (`start`).
pub struct CmdStart {
    meta: CommandMeta,
}

impl CmdStart {
    /// C++ `CmdStart::CmdStart`.
    pub fn new() -> Self {
        CmdStart {
            meta: CommandMeta {
                keyword: "start".into(),
                usage: "task <filter> start <mods>".into(),
                description: "Marks specified task as started".into(),
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

impl Default for CmdStart {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdStart {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdStart::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
