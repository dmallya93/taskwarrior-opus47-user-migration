//! Rust translation of `src/commands/CmdContext.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdContext.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdContext` (`context`).
pub struct CmdContext {
    meta: CommandMeta,
}

impl CmdContext {
    /// C++ `CmdContext::CmdContext`.
    pub fn new() -> Self {
        CmdContext {
            meta: CommandMeta {
                keyword: "context".into(),
                usage: "task          context [<name> | <subcommand>]".into(),
                description: "Set and define contexts (default filters / modifications)".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Context,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdContext {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdContext::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionContext` (`_context`).
pub struct CmdCompletionContext {
    meta: CommandMeta,
}

impl CmdCompletionContext {
    /// C++ `CmdCompletionContext::CmdCompletionContext`.
    pub fn new() -> Self {
        CmdCompletionContext {
            meta: CommandMeta {
                keyword: "_context".into(),
                usage: "task          _context".into(),
                description: "Lists all supported contexts, for completion purposes".into(),
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

impl Default for CmdCompletionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionContext {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionContext::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
