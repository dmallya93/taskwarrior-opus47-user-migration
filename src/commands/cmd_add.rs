//! Rust translation of `src/commands/CmdAdd.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 5 (CLI2/Filter/DOM/dispatch).
//!
//! Each struct below is one C++ class from `CmdAdd.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdAdd` (`add`).
pub struct CmdAdd {
    meta: CommandMeta,
}

impl CmdAdd {
    /// C++ `CmdAdd::CmdAdd`.
    pub fn new() -> Self {
        CmdAdd {
            meta: CommandMeta {
                keyword: "add".into(),
                usage: "task          add <mods>".into(),
                description: "Adds a new task".into(),
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

impl Default for CmdAdd {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdAdd {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdAdd::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
