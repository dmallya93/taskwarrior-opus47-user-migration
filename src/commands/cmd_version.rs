//! Rust translation of `src/commands/CmdVersion.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 1 (foundation).
//!
//! Ports both `version` and `_version`.
//!
//! Each struct below is one C++ class from `CmdVersion.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdVersion` (`version`).
pub struct CmdVersion {
    meta: CommandMeta,
}

impl CmdVersion {
    /// C++ `CmdVersion::CmdVersion`.
    pub fn new() -> Self {
        CmdVersion {
            meta: CommandMeta {
                keyword: "version".into(),
                usage: "task          version".into(),
                description: "Shows the Taskwarrior version number".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdVersion {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdVersion {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdVersion::execute`: not yet implemented; ported by foundation task #10 (milestone 1).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionVersion` (`_version`).
pub struct CmdCompletionVersion {
    meta: CommandMeta,
}

impl CmdCompletionVersion {
    /// C++ `CmdCompletionVersion::CmdCompletionVersion`.
    pub fn new() -> Self {
        CmdCompletionVersion {
            meta: CommandMeta {
                keyword: "_version".into(),
                usage: "task          _version".into(),
                description: "Shows only the Taskwarrior version number".into(),
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

impl Default for CmdCompletionVersion {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionVersion {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionVersion::execute`: not yet implemented; ported by foundation task #10
    /// (milestone 1).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
