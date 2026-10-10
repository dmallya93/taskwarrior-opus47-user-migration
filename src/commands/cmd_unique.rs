//! Rust translation of `src/commands/CmdUnique.cpp`.
//!
//! `task <filter> _unique <attribute>`: the sorted, distinct values of the
//! first word's attribute (canonicalized through CLI2, so abbreviations and
//! UDAs work) over the tasks matching the filter (safety disabled), one per
//! line.  Tasks lacking the attribute contribute nothing, except `id`, which
//! is synthesized for non-deleted, non-completed tasks.

use std::collections::BTreeSet;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::filter::Filter;
use crate::task::{Status, Task};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUnique` (`_unique`).
pub struct CmdUnique {
    meta: CommandMeta,
}

impl CmdUnique {
    /// C++ `CmdUnique::CmdUnique`.
    pub fn new() -> Self {
        CmdUnique {
            meta: CommandMeta {
                keyword: "_unique".into(),
                usage: "task <filter> _unique <attribute>".into(),
                description: "Generates lists of unique attribute values".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdUnique {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUnique {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUnique::execute`.
    ///
    /// # Errors
    /// `An attribute must be specified.  See 'task _columns'.` without a
    /// word, `You must specify an attribute or UDA.` when the word is not an
    /// attribute, and those of `Filter::subset` and `Task::getStatus`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply filter.
        let mut filter = Filter::new();
        filter.disable_safety();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        // Find <attribute>.
        // Just the first arg.
        let words = context.cli2.get_words(&context.config, &mut context.debug_messages);
        let Some(attribute) = words.first() else {
            return Err(TwError::Message(
                "An attribute must be specified.  See 'task _columns'.".to_string(),
            ));
        };

        let Some(canonical) = context.cli2.canonicalize("attribute", attribute) else {
            return Err(TwError::Message("You must specify an attribute or UDA.".to_string()));
        };

        // Find the unique set of matching tasks (C++ `std::set`: byte order).
        let mut values: BTreeSet<String> = BTreeSet::new();
        for task in &filtered {
            if task.has(&canonical) {
                values.insert(task.get(&canonical));
            } else if canonical == "id" {
                let status = task.get_status()?;
                if status != Status::Deleted && status != Status::Completed {
                    values.insert(task.id.to_string());
                }
            }
        }

        // Generate list of values.
        for value in &values {
            *output += value;
            output.push('\n');
        }

        context.headers.clear();
        Ok(0)
    }
}
