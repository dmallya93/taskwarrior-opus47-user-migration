//! Rust translation of `src/commands/CmdCount.cpp`.
//!
//! `task <filter> count`: the number of tasks matching the filter
//! (`Filter::subset`, pending-only optimisation included), skipping recurring
//! parent tasks, followed by a newline.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::filter::Filter;
use crate::shared::format::format;
use crate::task::{Status, Task};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCount` (`count`).
pub struct CmdCount {
    meta: CommandMeta,
}

impl CmdCount {
    /// C++ `CmdCount::CmdCount`.
    pub fn new() -> Self {
        CmdCount {
            meta: CommandMeta {
                keyword: "count".into(),
                usage: "task <filter> count".into(),
                description: "Counts matching tasks".into(),
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

impl Default for CmdCount {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCount {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCount::execute`.
    ///
    /// # Errors
    /// Those of `Filter::subset`, and an invalid stored status (C++
    /// `getStatus` throws out of the `count_if` predicate).
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        // Find number of matching tasks.  Skip recurring parent tasks.
        let mut count: i32 = 0;
        for task in &filtered {
            if task.get_status()? != Status::Recurring {
                count += 1;
            }
        }
        *output = format("{1}", &[&count]) + "\n";
        Ok(0)
    }
}
