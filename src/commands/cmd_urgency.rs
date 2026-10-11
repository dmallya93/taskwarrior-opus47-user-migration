//! Rust translation of `src/commands/CmdUrgency.cpp`.
//!
//! `task <filter> _urgency`: the urgency of each selected task.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::filter::Filter;
use crate::shared::format::format;
use crate::task::Task;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUrgency` (`_urgency`).
pub struct CmdUrgency {
    meta: CommandMeta,
}

impl CmdUrgency {
    /// C++ `CmdUrgency::CmdUrgency`.
    pub fn new() -> Self {
        CmdUrgency {
            meta: CommandMeta {
                keyword: "_urgency".into(),
                usage: "task <filter> _urgency".into(),
                description: "Displays the urgency measure of a task".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: false,
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

impl Default for CmdUrgency {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUrgency {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUrgency::execute`: one `task <id|uuid> urgency <value>` line
    /// per filtered task; footnote "No tasks specified." and return 1 when
    /// the filter selects nothing.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        if filtered.is_empty() {
            context.footnote("No tasks specified.");
            return Ok(1);
        }

        // Display urgency for the selected tasks.
        let mut out = String::new();
        for task in &mut filtered {
            let urgency = task.urgency(context)?;
            out.push_str(&format(
                "task {1} urgency {2}",
                &[&task.identifier(false), &urgency],
            ));
            out.push('\n');
        }

        *output = out;
        Ok(0)
    }
}
