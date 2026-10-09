//! Rust translation of `src/commands/CmdAliases.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdAliases.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionAliases` (`_aliases`).
pub struct CmdCompletionAliases {
    meta: CommandMeta,
}

impl CmdCompletionAliases {
    /// C++ `CmdCompletionAliases::CmdCompletionAliases`.
    pub fn new() -> Self {
        CmdCompletionAliases {
            meta: CommandMeta {
                keyword: "_aliases".into(),
                usage: "task          _aliases".into(),
                description: "Generates a list of all aliases, for autocompletion purposes".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionAliases {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionAliases {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionAliases::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
