//! Rust translation of `src/commands/CmdShow.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 1 (foundation).
//!
//! Milestone 1 ports `_show` (raw config output); `show` is completed by milestone 8.
//!
//! Each struct below is one C++ class from `CmdShow.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdShow` (`show`).
pub struct CmdShow {
    meta: CommandMeta,
}

impl CmdShow {
    /// C++ `CmdShow::CmdShow`.
    pub fn new() -> Self {
        CmdShow {
            meta: CommandMeta {
                keyword: "show".into(),
                usage: "task          show [all | substring]".into(),
                description: "Shows all configuration variables or subset".into(),
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

impl Default for CmdShow {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdShow {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdShow::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdShowRaw` (`_show`).
pub struct CmdShowRaw {
    meta: CommandMeta,
}

impl CmdShowRaw {
    /// C++ `CmdShowRaw::CmdShowRaw`.
    pub fn new() -> Self {
        CmdShowRaw {
            meta: CommandMeta {
                keyword: "_show".into(),
                usage: "task          _show".into(),
                description: "Shows all configuration settings in a machine-readable format".into(),
                read_only: true,
                displays_id: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdShowRaw {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdShowRaw {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdShowRaw::execute`: not yet implemented; ported by foundation task #10 (milestone 1).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
