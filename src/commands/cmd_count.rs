//! Rust translation of `src/commands/CmdCount.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 5 (CLI2/Filter/DOM/dispatch).
//!
//! Each struct below is one C++ class from `CmdCount.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCount` (`count`).
pub struct CmdCount {
    meta: CommandMeta,
}

impl CmdCount {
    /// C++ `CmdCount::CmdCount`.
    pub fn new() -> Self {
        CmdCount {
            meta: CommandMeta {
                keyword: "count".into(),
                usage: "task <filter> count".into(),
                description: "Counts matching tasks".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCount {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCount {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCount::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
