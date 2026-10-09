//! Rust translation of `src/commands/CmdColumns.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 6 (reports and columns).
//!
//! Each struct below is one C++ class from `CmdColumns.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdColumns` (`columns`).
pub struct CmdColumns {
    meta: CommandMeta,
}

impl CmdColumns {
    /// C++ `CmdColumns::CmdColumns`.
    pub fn new() -> Self {
        CmdColumns {
            meta: CommandMeta {
                keyword: "columns".into(),
                usage: "task          columns [substring]".into(),
                description: "All supported columns and formatting styles".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Config,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdColumns {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdColumns {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdColumns::execute`: not yet implemented; ported by milestone 6 (reports and columns).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionColumns` (`_columns`).
pub struct CmdCompletionColumns {
    meta: CommandMeta,
}

impl CmdCompletionColumns {
    /// C++ `CmdCompletionColumns::CmdCompletionColumns`.
    pub fn new() -> Self {
        CmdCompletionColumns {
            meta: CommandMeta {
                keyword: "_columns".into(),
                usage: "task          _columns".into(),
                description: "Displays only a list of supported columns".into(),
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

impl Default for CmdCompletionColumns {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionColumns {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionColumns::execute`: not yet implemented; ported by milestone 6 (reports and
    /// columns).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
