//! Rust translation of `src/commands/CmdTimesheet.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdTimesheet.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdTimesheet` (`timesheet`).
///
/// The C++ class also overrides `uses_context` to honour
/// `report.timesheet.context`; that needs the configuration, which the
/// [`Command::uses_context`] signature does not take yet, so the override
/// arrives with the port (milestone 8). Until then the constructor value is
/// returned.
pub struct CmdTimesheet {
    meta: CommandMeta,
}

impl CmdTimesheet {
    /// C++ `CmdTimesheet::CmdTimesheet`.
    pub fn new() -> Self {
        CmdTimesheet {
            meta: CommandMeta {
                keyword: "timesheet".into(),
                usage: "task [filter] timesheet".into(),
                description: "Summary of completed and started tasks".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Report,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdTimesheet {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdTimesheet {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdTimesheet::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
