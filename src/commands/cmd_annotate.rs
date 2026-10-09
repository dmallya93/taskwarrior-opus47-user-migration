//! Rust translation of `src/commands/CmdAnnotate.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdAnnotate.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdAnnotate` (`annotate`).
pub struct CmdAnnotate {
    meta: CommandMeta,
}

impl CmdAnnotate {
    /// C++ `CmdAnnotate::CmdAnnotate`.
    pub fn new() -> Self {
        CmdAnnotate {
            meta: CommandMeta {
                keyword: "annotate".into(),
                usage: "task <filter> annotate <mods>".into(),
                description: "Adds an annotation to an existing task".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: true,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdAnnotate {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdAnnotate {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdAnnotate::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
