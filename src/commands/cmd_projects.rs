//! Rust translation of `src/commands/CmdProjects.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdProjects.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdProjects` (`projects`).
pub struct CmdProjects {
    meta: CommandMeta,
}

impl CmdProjects {
    /// C++ `CmdProjects::CmdProjects`.
    pub fn new() -> Self {
        CmdProjects {
            meta: CommandMeta {
                keyword: "projects".into(),
                usage: "task <filter> projects".into(),
                description: "Shows all project names used".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdProjects {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdProjects {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdProjects::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionProjects` (`_projects`).
pub struct CmdCompletionProjects {
    meta: CommandMeta,
}

impl CmdCompletionProjects {
    /// C++ `CmdCompletionProjects::CmdCompletionProjects`.
    pub fn new() -> Self {
        CmdCompletionProjects {
            meta: CommandMeta {
                keyword: "_projects".into(),
                usage: "task <filter> _projects".into(),
                description: "Shows only a list of all project names used".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionProjects {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionProjects {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionProjects::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
