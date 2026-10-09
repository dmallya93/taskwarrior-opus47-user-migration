//! Rust translation of `src/commands/CmdImport.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdImport.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdImport` (`import`).
pub struct CmdImport {
    meta: CommandMeta,
}

impl CmdImport {
    /// C++ `CmdImport::CmdImport`.
    pub fn new() -> Self {
        CmdImport {
            meta: CommandMeta {
                keyword: "import".into(),
                usage: "task          import [<file> ...]".into(),
                description: "Imports JSON files".into(),
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

impl Default for CmdImport {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdImport {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdImport::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
