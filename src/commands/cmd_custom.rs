//! Rust translation of `src/commands/CmdCustom.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 6 (reports and columns).
//!
//! [`CmdCustom`] is the C++ class `CmdCustom`: one instance per custom
//! report (`report.<name>.columns`), constructed by
//! [`factory`](crate::commands::command::factory) with the C++ keyword, usage
//! and description arguments. `execute` is a stub returning
//! [`TwError::NotImplemented`] until the port lands (foundation DD4).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCustom` (a custom report).
///
/// The C++ class also overrides `uses_context` to honour
/// `report.<name>.context`; that needs the configuration, which the
/// [`Command::uses_context`] signature does not take yet, so the override
/// arrives with the port (milestone 6). Until then the constructor value is
/// returned.
pub struct CmdCustom {
    meta: CommandMeta,
}

impl CmdCustom {
    /// C++ `CmdCustom::CmdCustom (keyword, usage, description)`.
    pub fn new(keyword: &str, usage: &str, description: &str) -> Self {
        CmdCustom {
            meta: CommandMeta {
                keyword: keyword.into(),
                usage: usage.into(),
                description: description.into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Report,
                ..CommandMeta::default()
            },
        }
    }
}

impl Command for CmdCustom {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCustom::execute`: not yet implemented; ported by milestone 6 (reports and columns).
    fn execute(&self, _context: &mut Context, _output: &mut String) -> Result<i32> {
        Err(TwError::NotImplemented(self.meta.keyword.clone()))
    }
}
