//! Rust translation of `src/commands/CmdUnique.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 5 (CLI2/Filter/DOM/dispatch).
//!
//! Each struct below is one C++ class from `CmdUnique.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUnique` (`_unique`).
pub struct CmdUnique {
    meta: CommandMeta,
}

impl CmdUnique {
    /// C++ `CmdUnique::CmdUnique`.
    pub fn new() -> Self {
        CmdUnique {
            meta: CommandMeta {
                keyword: "_unique".into(),
                usage: "task <filter> _unique <attribute>".into(),
                description: "Generates lists of unique attribute values".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdUnique {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUnique {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUnique::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
