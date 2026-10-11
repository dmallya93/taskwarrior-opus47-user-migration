//! Rust translation of `src/commands/CmdStop.cpp`.
//!
//! `task <filter> stop <mods>`: remove `start` from every filtered started
//! task, apply annotation modifications and the optional `rc.journal.time`
//! annotation, ask permission with the task diff and store it with
//! `TDB2::modify`.

use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::dependency::dependency_chain_on_start;
use crate::error::Result;
use crate::feedback::{feedback_affected_count, feedback_affected_task, on_project_change};
use crate::filter::Filter;
use crate::recur::update_recurrence_mask;
use crate::shared::format::format;
use crate::shared::shared::with_console;
use crate::task::ModType;
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdStop` (`stop`).
pub struct CmdStop {
    meta: CommandMeta,
}

impl CmdStop {
    /// C++ `CmdStop::CmdStop`.
    pub fn new() -> Self {
        CmdStop {
            meta: CommandMeta {
                keyword: "stop".into(),
                usage: "task <filter> stop <mods>".into(),
                description: "Removes the 'start' time from a task".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: true,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdStop {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdStop {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdStop::execute`.
    ///
    /// Like the C++, a stopped task also runs the dependency chain check
    /// (`dependencyChainOnStart`).
    fn execute(&self, context: &mut Context, _output: &mut String) -> Result<i32> {
        let mut rc = 0;
        let mut count = 0;

        // Apply filter.
        let mut filtered = Vec::new();
        Filter::new().subset(context, &mut filtered)?;
        if filtered.is_empty() {
            context.footnote("No tasks specified.");
            return Ok(1);
        }

        // Accumulated project change notifications.
        let mut project_changes = ProjectChanges::new();

        if filtered.len() > 1 {
            feedback_affected_count(
                context,
                "This command will alter {1} tasks.",
                filtered.len() as i32,
            );
        }

        let quantity = filtered.len() as u32;
        for mut task in filtered {
            if task.has("start") {
                let before = task.clone();

                // Stop the specified task.
                let question = format(
                    "Stop task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );

                task.modify(context, ModType::Annotate, false)?;
                task.remove("start");

                if context.config.get_boolean("journal.time") {
                    let annotation = context.config.get("journal.time.stop.annotation");
                    task.add_annotation(&annotation);
                }

                let diff = before.diff(&task, &context.columns, &context.config);
                if self.permission(context, &(diff + &question), quantity) {
                    update_recurrence_mask(context, &task)?;
                    tdb2::modify(context, &mut task)?;
                    count += 1;
                    feedback_affected_task(context, "Stopping task {1} '{2}'.", &task);
                    dependency_chain_on_start(context, &task)?;
                    if context.verbose("project") {
                        let message = on_project_change(context, &task, false)?;
                        project_changes.insert(task.get("project"), message);
                    }
                } else {
                    with_console(|_, stdout| {
                        let _ = stdout.write_all(b"Task not stopped.\n");
                        let _ = stdout.flush();
                    });
                    rc = 1;
                    if self.meta.permission_quit.get() {
                        break;
                    }
                }
            } else {
                let line = format(
                    "Task {1} '{2}' not started.",
                    &[&task.identifier(true), &task.get("description")],
                ) + "\n";
                with_console(|_, stdout| {
                    let _ = stdout.write_all(line.as_bytes());
                    let _ = stdout.flush();
                });
                rc = 1;
            }
        }

        // Now list the project changes.
        for (project, message) in &project_changes {
            if !project.is_empty() {
                context.footnote(message);
            }
        }

        feedback_affected_count(
            context,
            if count == 1 {
                "Stopped {1} task."
            } else {
                "Stopped {1} tasks."
            },
            count,
        );
        Ok(rc)
    }
}
