//! Rust translation of `src/commands/CmdUrgency.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 6 (reports and columns).
//!
//! Each struct below is one C++ class from `CmdUrgency.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUrgency` (`_urgency`).
pub struct CmdUrgency {
    meta: CommandMeta,
}

impl CmdUrgency {
    /// C++ `CmdUrgency::CmdUrgency`.
    pub fn new() -> Self {
        CmdUrgency {
            meta: CommandMeta {
                keyword: "_urgency".into(),
                usage: "task <filter> _urgency".into(),
                description: "Displays the urgency measure of a task".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdUrgency {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUrgency {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUrgency::execute`: not yet implemented; ported by milestone 6 (reports and columns).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
