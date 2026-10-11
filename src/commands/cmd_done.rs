//! Rust translation of `src/commands/CmdDone.cpp`.
//!
//! `task <filter> done <mods>`: complete every filtered pending or waiting
//! task (annotation modifications, `end` stamp, stop with the optional
//! `rc.journal.time` annotation), ask permission with the task diff, store it
//! with `TDB2::modify` (which runs the on-modify hooks), report unblocked
//! tasks and dependency chain repairs, then nag about more urgent tasks.

use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::dependency::dependency_chain_on_complete;
use crate::error::Result;
use crate::feedback::{feedback_affected_count, feedback_affected_task, feedback_unblocked, on_project_change};
use crate::filter::Filter;
use crate::nag::nag;
use crate::recur::update_recurrence_mask;
use crate::shared::format::format;
use crate::shared::shared::with_console;
use crate::task::{ModType, Status};
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDone` (`done`).
pub struct CmdDone {
    meta: CommandMeta,
}

impl CmdDone {
    /// C++ `CmdDone::CmdDone`.
    pub fn new() -> Self {
        CmdDone {
            meta: CommandMeta {
                keyword: "done".into(),
                usage: "task <filter> done <mods>".into(),
                description: "Marks the specified task as completed".into(),
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

impl Default for CmdDone {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDone {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDone::execute`.
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
            feedback_affected_count(context, "This command will alter {1} tasks.", filtered.len() as i32);
        }

        let quantity = filtered.len() as u32;
        let mut modified = Vec::new();
        for mut task in filtered {
            let before = task.clone();

            let status = task.get_status()?;
            if status == Status::Pending || status == Status::Waiting {
                // Complete the specified task.
                let question = format(
                    "Complete task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );

                task.modify(context, ModType::Annotate, false)?;
                task.set_status(Status::Completed);
                if !task.has("end") {
                    task.set_as_now("end");
                }

                // Stop the task, if started.
                if task.has("start") {
                    task.remove("start");
                    if context.config.get_boolean("journal.time") {
                        let annotation = context.config.get("journal.time.stop.annotation");
                        task.add_annotation(&annotation);
                    }
                }

                let diff = before.diff(&task, &context.columns, &context.config);
                if self.permission(context, &(diff + &question), quantity) {
                    update_recurrence_mask(context, &task)?;
                    tdb2::modify(context, &mut task)?;
                    count += 1;
                    feedback_affected_task(context, "Completed task {1} '{2}'.", &task);
                    feedback_unblocked(context, &task)?;
                    dependency_chain_on_complete(context, &task)?;
                    if context.verbose("project") {
                        let message = on_project_change(context, &task, true)?;
                        project_changes.insert(task.get("project"), message);
                    }

                    // Save unmodified task for potential nagging later
                    modified.push(before);
                } else {
                    with_console(|_, stdout| {
                        let _ = stdout.write_all(b"Task not completed.\n");
                        let _ = stdout.flush();
                    });
                    rc = 1;
                    if self.meta.permission_quit.get() {
                        break;
                    }
                }
            } else {
                let line = format(
                    "Task {1} '{2}' is neither pending nor waiting.",
                    &[&task.identifier(true), &task.get("description")],
                ) + "\n";
                with_console(|_, stdout| {
                    let _ = stdout.write_all(line.as_bytes());
                    let _ = stdout.flush();
                });
                rc = 1;
            }
        }

        nag(context, &mut modified)?;

        // Now list the project changes.
        for (project, message) in &project_changes {
            if !project.is_empty() {
                context.footnote(message);
            }
        }

        feedback_affected_count(
            context,
            if count == 1 { "Completed {1} task." } else { "Completed {1} tasks." },
            count,
        );
        Ok(rc)
    }
}
