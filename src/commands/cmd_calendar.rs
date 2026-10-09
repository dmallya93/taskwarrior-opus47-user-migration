//! Rust translation of `src/commands/CmdCalendar.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdCalendar.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCalendar` (`calendar`).
pub struct CmdCalendar {
    meta: CommandMeta,
}

impl CmdCalendar {
    /// C++ `CmdCalendar::CmdCalendar`.
    pub fn new() -> Self {
        CmdCalendar {
            meta: CommandMeta {
                keyword: "calendar".into(),
                usage: "task          calendar [due|<month> <year>|<year>] [y]".into(),
                description: "Shows a calendar, with due tasks marked".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCalendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCalendar {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCalendar::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
