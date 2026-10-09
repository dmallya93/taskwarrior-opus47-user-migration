//! Rust translation of `src/commands/CmdNews.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 8 (remaining commands).
//!
//! Each struct below is one C++ class from `CmdNews.cpp`, carrying the exact
//! metadata of its C++ constructor; `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdNews` (`news`).
pub struct CmdNews {
    meta: CommandMeta,
}

impl CmdNews {
    /// C++ `CmdNews::CmdNews`.
    pub fn new() -> Self {
        CmdNews {
            meta: CommandMeta {
                keyword: "news".into(),
                usage: "task          news".into(),
                description: "Displays news about the recent releases".into(),
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

impl Default for CmdNews {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdNews {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdNews::execute`: not yet implemented; ported by milestone 8 (remaining commands).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
