//! Rust translation of `src/commands/CmdDuplicate.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdDuplicate.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDuplicate` (`duplicate`).
pub struct CmdDuplicate {
    meta: CommandMeta,
}

impl CmdDuplicate {
    /// C++ `CmdDuplicate::CmdDuplicate`.
    pub fn new() -> Self {
        CmdDuplicate {
            meta: CommandMeta {
                keyword: "duplicate".into(),
                usage: "task <filter> duplicate <mods>".into(),
                description: "Duplicates the specified tasks".into(),
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

impl Default for CmdDuplicate {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDuplicate {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDuplicate::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
