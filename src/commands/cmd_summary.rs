//! Rust translation of `src/commands/CmdSummary.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdSummary.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdSummary` (`summary`).
pub struct CmdSummary {
    meta: CommandMeta,
}

impl CmdSummary {
    /// C++ `CmdSummary::CmdSummary`.
    pub fn new() -> Self {
        CmdSummary {
            meta: CommandMeta {
                keyword: "summary".into(),
                usage: "task <filter> summary".into(),
                description: "Shows a report of task status by project".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdSummary {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdSummary {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdSummary::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
