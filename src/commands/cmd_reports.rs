//! Rust translation of `src/commands/CmdReports.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdReports.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdReports` (`reports`).
pub struct CmdReports {
    meta: CommandMeta,
}

impl CmdReports {
    /// C++ `CmdReports::CmdReports`.
    pub fn new() -> Self {
        CmdReports {
            meta: CommandMeta {
                keyword: "reports".into(),
                usage: "task          reports".into(),
                description: "Lists all supported reports".into(),
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

impl Default for CmdReports {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdReports {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdReports::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
