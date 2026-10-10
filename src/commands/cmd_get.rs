//! Rust translation of `src/commands/CmdGet.cpp`.
//!
//! `task _get <DOM> [<DOM> ...]`: resolve each DOM argument with
//! `getDOM (raw, NULL, value)` (an unresolvable reference yields an empty
//! string) and print the results joined by single spaces, plus a newline.
//! Non-DOM words/identifiers that are not the binary or the command are
//! rejected.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::dom::get_dom_task;
use crate::error::{Result, TwError};
use crate::lexer::Type as LexerType;
use crate::shared::format::format;
use crate::shared::shared::join;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGet` (`_get`).
pub struct CmdGet {
    meta: CommandMeta,
}

impl CmdGet {
    /// C++ `CmdGet::CmdGet`.
    pub fn new() -> Self {
        CmdGet {
            meta: CommandMeta {
                keyword: "_get".into(),
                usage: "task          _get <DOM> [<DOM> ...]".into(),
                description: "DOM Accessor".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdGet {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGet {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdGet::execute`.
    ///
    /// # Errors
    /// `'<arg>' is not a DOM reference.`, `No DOM reference specified.`, and
    /// those of `getDOM`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut results: Vec<String> = Vec::new();
        let args = context.cli2.args.clone();
        for arg in &args {
            match arg.lextype {
                LexerType::dom => {
                    if let Some(result) = get_dom_task(context, &arg.attribute("raw"), None)? {
                        results.push(result.to_string());
                    } else {
                        results.push(String::new());
                    }
                }

                // Look for non-refs to complain about.
                LexerType::word | LexerType::identifier => {
                    if !arg.has_tag("BINARY") && !arg.has_tag("CMD") {
                        return Err(TwError::Message(format(
                            "'{1}' is not a DOM reference.",
                            &[&arg.attribute("raw")],
                        )));
                    }
                }

                _ => {}
            }
        }

        if results.is_empty() {
            return Err(TwError::Message("No DOM reference specified.".to_string()));
        }

        *output = join(" ", &results);
        output.push('\n');
        Ok(0)
    }
}
