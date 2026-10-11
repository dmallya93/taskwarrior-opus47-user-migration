//! Rust translation of `src/commands/CmdCommands.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdCommands.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCommands` (`commands`).
pub struct CmdCommands {
    meta: CommandMeta,
}

impl CmdCommands {
    /// C++ `CmdCommands::CmdCommands`.
    pub fn new() -> Self {
        CmdCommands {
            meta: CommandMeta {
                keyword: "commands".into(),
                usage: "task          commands".into(),
                description: "Generates a list of all commands, with behavior details".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCommands {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCommands {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCommands::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionCommands` (`_commands`).
pub struct CmdCompletionCommands {
    meta: CommandMeta,
}

impl CmdCompletionCommands {
    /// C++ `CmdCompletionCommands::CmdCompletionCommands`.
    pub fn new() -> Self {
        CmdCompletionCommands {
            meta: CommandMeta {
                keyword: "_commands".into(),
                usage: "task          _commands".into(),
                description: "Generates a list of all commands, for autocompletion purposes".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
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

impl Default for CmdCompletionCommands {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionCommands {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionCommands::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdZshCommands` (`_zshcommands`).
pub struct CmdZshCommands {
    meta: CommandMeta,
}

impl CmdZshCommands {
    /// C++ `CmdZshCommands::CmdZshCommands`.
    pub fn new() -> Self {
        CmdZshCommands {
            meta: CommandMeta {
                keyword: "_zshcommands".into(),
                usage: "task          _zshcommands".into(),
                description: "Generates a list of all commands, for zsh autocompletion purposes"
                    .into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
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

impl Default for CmdZshCommands {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdZshCommands {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdZshCommands::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
