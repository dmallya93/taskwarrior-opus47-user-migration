//! Rust translation of `src/commands/CmdStats.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdStats.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdStats` (`stats`).
pub struct CmdStats {
    meta: CommandMeta,
}

impl CmdStats {
    /// C++ `CmdStats::CmdStats`.
    pub fn new() -> Self {
        CmdStats {
            meta: CommandMeta {
                keyword: "stats".into(),
                usage: "task <filter> stats".into(),
                description: "Shows task database statistics".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: false,
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

impl Default for CmdStats {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdStats {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdStats::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
