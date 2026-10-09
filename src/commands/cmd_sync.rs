//! Rust translation of `src/commands/CmdSync.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdSync.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdSync` (`synchronize`).
pub struct CmdSync {
    meta: CommandMeta,
}

impl CmdSync {
    /// C++ `CmdSync::CmdSync`.
    pub fn new() -> Self {
        CmdSync {
            meta: CommandMeta {
                keyword: "synchronize".into(),
                usage: "task          synchronize [initialize]".into(),
                description: "Synchronizes data with the Taskserver".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Migration,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdSync {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdSync {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdSync::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
