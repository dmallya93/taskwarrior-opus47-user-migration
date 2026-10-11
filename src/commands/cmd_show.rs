//! Rust translation of `src/commands/CmdShow.cpp`.
//!
//! Milestone 1 ports `_show` (raw config output); `show` is still a stub,
//! completed by milestone 8.
//!
//! Each struct below is one C++ class from `CmdShow.cpp`, carrying the exact
//! metadata of its C++ constructor; `CmdShow::execute` is a stub returning
//! [`TwError::NotImplemented`] until its port lands (foundation DD4).

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

    /// C++ `CmdShowRaw::execute`: every setting as `name=value`, sorted by
    /// name.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Get all the settings and sort alphabetically by name.
        let mut all = context.config.all();
        all.sort();

        // Display them all.
        let mut out = String::new();
        for i in &all {
            out.push_str(i);
            out.push('=');
            out.push_str(&context.config.get(i));
            out.push('\n');
        }

        *output = out;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_raw_sorted() {
        let mut context = Context::new();
        for (key, value) in [
            ("zeta", "last"),
            ("alpha", "1"),
            ("Beta", ""),
            ("a.b", "x=y"),
        ] {
            context.config.set(key, value);
        }
        let mut output = String::new();
        assert_eq!(
            CmdShowRaw::new()
                .execute(&mut context, &mut output)
                .unwrap(),
            0
        );
        assert_eq!(output, "Beta=\na.b=x=y\nalpha=1\nzeta=last\n");
    }

    #[test]
    fn show_raw_empty() {
        let mut context = Context::new();
        context.config = crate::shared::configuration::Configuration::new();
        let mut output = String::from("stale");
        CmdShowRaw::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert_eq!(output, "");
    }
}
