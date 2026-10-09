//! Rust translation of `src/commands/CmdGet.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 5 (CLI2/Filter/DOM/dispatch).
//!
//! Each struct below is one C++ class from `CmdGet.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGet` (`_get`).
pub struct CmdGet {
    meta: CommandMeta,
}

impl CmdGet {
    /// C++ `CmdGet::CmdGet`.
    pub fn new() -> Self {
        CmdGet {
            meta: CommandMeta {
                keyword: "_get".into(),
                usage: "task          _get <DOM> [<DOM> ...]".into(),
                description: "DOM Accessor".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdGet {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGet {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdGet::execute`: not yet implemented; ported by milestone 5
    /// (CLI2/Filter/DOM/dispatch).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
