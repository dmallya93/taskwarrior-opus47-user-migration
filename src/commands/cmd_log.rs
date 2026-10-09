//! Rust translation of `src/commands/CmdLog.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 5 (CLI2/Filter/DOM/dispatch).
//!
//! Each struct below is one C++ class from `CmdLog.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdLog` (`log`).
pub struct CmdLog {
    meta: CommandMeta,
}

impl CmdLog {
    /// C++ `CmdLog::CmdLog`.
    pub fn new() -> Self {
        CmdLog {
            meta: CommandMeta {
                keyword: "log".into(),
                usage: "task          log <mods>".into(),
                description: "Adds a new task that is already completed".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: false,
                accepts_modifications: true,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdLog {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdLog {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdLog::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
