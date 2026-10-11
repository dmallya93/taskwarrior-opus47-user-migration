//! Rust translation of `src/commands/CmdAttributes.cpp`.
//!
//! `task _zshattributes`: every column name (built-in and UDA), sorted, as
//! `name:name` lines for zsh completion.  This is the only class in the C++
//! file; 3.5.0 has no `_attributes` command.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdZshAttributes` (`_zshattributes`).
pub struct CmdZshAttributes {
    meta: CommandMeta,
}

impl CmdZshAttributes {
    /// C++ `CmdZshAttributes::CmdZshAttributes`.
    pub fn new() -> Self {
        CmdZshAttributes {
            meta: CommandMeta {
                keyword: "_zshattributes".into(),
                usage: "task          _zshattributes".into(),
                description: "Generates a list of all attributes, for zsh autocompletion purposes"
                    .into(),
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

impl Default for CmdZshAttributes {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdZshAttributes {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdZshAttributes::execute`.  Cannot fail.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Get a list of all columns, sort them.
        let mut columns = context.get_columns();
        columns.sort();

        let mut out = String::new();
        for col in &columns {
            out += &format!("{col}:{col}\n");
        }

        *output = out;
        Ok(0)
    }
}
