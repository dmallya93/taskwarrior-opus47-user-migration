//! Rust translation of `src/commands/CmdImportV2.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdImportV2.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdImportV2` (`import-v2`).
pub struct CmdImportV2 {
    meta: CommandMeta,
}

impl CmdImportV2 {
    /// C++ `CmdImportV2::CmdImportV2`.
    pub fn new() -> Self {
        CmdImportV2 {
            meta: CommandMeta {
                keyword: "import-v2".into(),
                usage: "task          import-v2".into(),
                description: "Imports Taskwarrior v2.x files".into(),
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

impl Default for CmdImportV2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdImportV2 {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdImportV2::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
