//! Rust translation of `src/commands/CmdCalc.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 3 (Color/Table/Variant/Eval).
//!
//! Each struct below is one C++ class from `CmdCalc.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCalc` (`calc`).
pub struct CmdCalc {
    meta: CommandMeta,
}

impl CmdCalc {
    /// C++ `CmdCalc::CmdCalc`.
    pub fn new() -> Self {
        CmdCalc {
            meta: CommandMeta {
                keyword: "calc".into(),
                usage: "task          calc <expression>".into(),
                description: "Calculator".into(),
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

impl Default for CmdCalc {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCalc {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCalc::execute`: not yet implemented; ported by milestone 3
    /// (Color/Table/Variant/Eval).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
