//! Rust translation of `src/commands/CmdUDAs.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdUDAs.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUDAs` (`udas`).
pub struct CmdUDAs {
    meta: CommandMeta,
}

impl CmdUDAs {
    /// C++ `CmdUDAs::CmdUDAs`.
    pub fn new() -> Self {
        CmdUDAs {
            meta: CommandMeta {
                keyword: "udas".into(),
                usage: "task          udas".into(),
                description: "Shows all the defined UDA details".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Config,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdUDAs {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUDAs {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUDAs::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionUDAs` (`_udas`).
pub struct CmdCompletionUDAs {
    meta: CommandMeta,
}

impl CmdCompletionUDAs {
    /// C++ `CmdCompletionUDAs::CmdCompletionUDAs`.
    pub fn new() -> Self {
        CmdCompletionUDAs {
            meta: CommandMeta {
                keyword: "_udas".into(),
                usage: "task          _udas".into(),
                description: "Shows the defined UDAs for completion purposes".into(),
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

impl Default for CmdCompletionUDAs {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionUDAs {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionUDAs::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
