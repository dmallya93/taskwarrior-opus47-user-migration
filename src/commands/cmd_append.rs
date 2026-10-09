//! Rust translation of `src/commands/CmdAppend.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdAppend.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdAppend` (`append`).
pub struct CmdAppend {
    meta: CommandMeta,
}

impl CmdAppend {
    /// C++ `CmdAppend::CmdAppend`.
    pub fn new() -> Self {
        CmdAppend {
            meta: CommandMeta {
                keyword: "append".into(),
                usage: "task <filter> append <mods>".into(),
                description: "Appends text to an existing task description".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: true,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdAppend {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdAppend {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdAppend::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
