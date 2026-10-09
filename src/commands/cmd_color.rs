//! Rust translation of `src/commands/CmdColor.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 6 (reports and columns).
//!
//! Each struct below is one C++ class from `CmdColor.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdColor` (`colors`).
pub struct CmdColor {
    meta: CommandMeta,
}

impl CmdColor {
    /// C++ `CmdColor::CmdColor`.
    pub fn new() -> Self {
        CmdColor {
            meta: CommandMeta {
                keyword: "colors".into(),
                usage: "task          colors [sample | legend]".into(),
                description: "All colors, a sample, or a legend".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdColor {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdColor {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdColor::execute`: not yet implemented; ported by milestone 6 (reports and columns).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
