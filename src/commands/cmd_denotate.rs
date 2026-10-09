//! Rust translation of `src/commands/CmdDenotate.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations, hooks, recurrence, undo, sync).
//!
//! Each struct below is one C++ class from `CmdDenotate.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDenotate` (`denotate`).
pub struct CmdDenotate {
    meta: CommandMeta,
}

impl CmdDenotate {
    /// C++ `CmdDenotate::CmdDenotate`.
    pub fn new() -> Self {
        CmdDenotate {
            meta: CommandMeta {
                keyword: "denotate".into(),
                usage: "task <filter> denotate <pattern>".into(),
                description: "Deletes an annotation".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdDenotate {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDenotate {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDenotate::execute`: not yet implemented; ported by milestone 7 (mutations, hooks,
    /// recurrence, undo, sync).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
