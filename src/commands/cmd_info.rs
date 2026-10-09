//! Rust translation of `src/commands/CmdInfo.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 6 (reports and columns).
//!
//! Each struct below is one C++ class from `CmdInfo.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdInfo` (`information`).
pub struct CmdInfo {
    meta: CommandMeta,
}

impl CmdInfo {
    /// C++ `CmdInfo::CmdInfo`.
    pub fn new() -> Self {
        CmdInfo {
            meta: CommandMeta {
                keyword: "information".into(),
                usage: "task <filter> information".into(),
                description: "Shows all data and metadata".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdInfo {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdInfo::execute`: not yet implemented; ported by milestone 6 (reports and columns).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
