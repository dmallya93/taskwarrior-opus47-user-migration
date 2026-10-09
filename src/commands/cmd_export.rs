//! Rust translation of `src/commands/CmdExport.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 5 (CLI2/Filter/DOM/dispatch).
//!
//! Each struct below is one C++ class from `CmdExport.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdExport` (`export`).
pub struct CmdExport {
    meta: CommandMeta,
}

impl CmdExport {
    /// C++ `CmdExport::CmdExport`.
    pub fn new() -> Self {
        CmdExport {
            meta: CommandMeta {
                keyword: "export".into(),
                usage: "task <filter> export [<report>]".into(),
                description: "Exports tasks in JSON format".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Migration,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdExport {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdExport {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdExport::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
