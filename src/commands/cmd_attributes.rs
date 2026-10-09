//! Rust translation of `src/commands/CmdAttributes.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdAttributes.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdZshAttributes` (`_zshattributes`).
pub struct CmdZshAttributes {
    meta: CommandMeta,
}

impl CmdZshAttributes {
    /// C++ `CmdZshAttributes::CmdZshAttributes`.
    pub fn new() -> Self {
        CmdZshAttributes {
            meta: CommandMeta {
                keyword: "_zshattributes".into(),
                usage: "task          _zshattributes".into(),
                description: "Generates a list of all attributes, for zsh autocompletion purposes".into(),
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

impl Default for CmdZshAttributes {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdZshAttributes {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdZshAttributes::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
