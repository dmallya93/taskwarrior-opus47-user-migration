//! Rust translation of `src/commands/CmdConfig.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdConfig.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdConfig` (`config`).
pub struct CmdConfig {
    meta: CommandMeta,
}

impl CmdConfig {
    /// C++ `CmdConfig::CmdConfig`.
    pub fn new() -> Self {
        CmdConfig {
            meta: CommandMeta {
                keyword: "config".into(),
                usage: "task          config [name [value | '']]".into(),
                description: "Change settings in the task configuration".into(),
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

impl Default for CmdConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdConfig {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdConfig::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionConfig` (`_config`).
pub struct CmdCompletionConfig {
    meta: CommandMeta,
}

impl CmdCompletionConfig {
    /// C++ `CmdCompletionConfig::CmdCompletionConfig`.
    pub fn new() -> Self {
        CmdCompletionConfig {
            meta: CommandMeta {
                keyword: "_config".into(),
                usage: "task          _config".into(),
                description: "Lists all supported configuration variables, for completion purposes".into(),
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

impl Default for CmdCompletionConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionConfig {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionConfig::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
