//! Rust translation of `src/commands/CmdAliases.cpp`.
//!
//! `task _aliases`: every `alias.<name>` configuration key, without its
//! `alias.` prefix, one per line in configuration (key) order, for shell
//! completion.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionAliases` (`_aliases`).
pub struct CmdCompletionAliases {
    meta: CommandMeta,
}

impl CmdCompletionAliases {
    /// C++ `CmdCompletionAliases::CmdCompletionAliases`.
    pub fn new() -> Self {
        CmdCompletionAliases {
            meta: CommandMeta {
                keyword: "_aliases".into(),
                usage: "task          _aliases".into(),
                description: "Generates a list of all aliases, for autocompletion purposes".into(),
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

impl Default for CmdCompletionAliases {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionAliases {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionAliases::execute`: the name of every `alias.*`
    /// setting, in key order, one per line.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        for (key, _) in &context.config {
            // C++ `alias.first.substr (0, 6) == "alias."`, then `substr (6)`.
            if let Some(name) = key.strip_prefix("alias.") {
                output.push_str(name);
                output.push('\n');
            }
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_alias_names_in_key_order() {
        let mut context = Context::new();
        context.config.set("alias.zz", "list");
        context.config.set("alias.aa", "exec echo hi");
        context.config.set("alias.", "next");
        context.config.set("aliasx.no", "x");
        context.config.set("xalias.no", "x");
        let mut output = String::new();
        let rc = CmdCompletionAliases::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert_eq!(rc, 0);
        assert_eq!(output, "\naa\nzz\n");
    }

    #[test]
    fn no_aliases_no_output() {
        let mut context = Context::new();
        let mut output = String::new();
        let rc = CmdCompletionAliases::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert_eq!(rc, 0);
        assert_eq!(output, "");
    }
}
