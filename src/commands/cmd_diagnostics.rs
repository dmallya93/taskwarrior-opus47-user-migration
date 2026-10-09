//! Rust translation of `src/commands/CmdDiagnostics.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdDiagnostics.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDiagnostics` (`diagnostics`).
pub struct CmdDiagnostics {
    meta: CommandMeta,
}

impl CmdDiagnostics {
    /// C++ `CmdDiagnostics::CmdDiagnostics`.
    pub fn new() -> Self {
        CmdDiagnostics {
            meta: CommandMeta {
                keyword: "diagnostics".into(),
                usage: "task          diagnostics".into(),
                description: "Platform, build and environment details".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDiagnostics {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDiagnostics::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
