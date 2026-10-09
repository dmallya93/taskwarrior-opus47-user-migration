//! Rust translation of `src/commands/CmdPrepend.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdPrepend.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdPrepend` (`prepend`).
pub struct CmdPrepend {
    meta: CommandMeta,
}

impl CmdPrepend {
    /// C++ `CmdPrepend::CmdPrepend`.
    pub fn new() -> Self {
        CmdPrepend {
            meta: CommandMeta {
                keyword: "prepend".into(),
                usage: "task <filter> prepend <mods>".into(),
                description: "Prepends text to an existing task description".into(),
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

impl Default for CmdPrepend {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdPrepend {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdPrepend::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
