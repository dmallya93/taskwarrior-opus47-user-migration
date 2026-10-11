//! Rust translation of `src/commands/CmdTags.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdTags.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdTags` (`tags`).
pub struct CmdTags {
    meta: CommandMeta,
}

impl CmdTags {
    /// C++ `CmdTags::CmdTags`.
    pub fn new() -> Self {
        CmdTags {
            meta: CommandMeta {
                keyword: "tags".into(),
                usage: "task <filter> tags".into(),
                description: "Shows a list of all tags used".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: false,
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

impl Default for CmdTags {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdTags {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdTags::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionTags` (`_tags`).
pub struct CmdCompletionTags {
    meta: CommandMeta,
}

impl CmdCompletionTags {
    /// C++ `CmdCompletionTags::CmdCompletionTags`.
    pub fn new() -> Self {
        CmdCompletionTags {
            meta: CommandMeta {
                keyword: "_tags".into(),
                usage: "task <filter> _tags".into(),
                description: "Shows only a list of all tags used, for autocompletion purposes"
                    .into(),
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

impl Default for CmdCompletionTags {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionTags {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionTags::execute`: not yet implemented; ported by milestone 8 (remaining
    /// commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
