//! Rust translation of `src/commands/CmdStart.cpp`.
//!
//! `task <filter> start <mods>`: stamp `start` on every filtered task that is
//! not already started (reopening completed/deleted tasks as pending), apply
//! annotation modifications and the optional `rc.journal.time` annotation,
//! ask permission with the task diff, store it with `TDB2::modify`, run the
//! dependency chain check, then nag about more urgent tasks.

use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::dependency::dependency_chain_on_start;
use crate::error::Result;
use crate::feedback::{feedback_affected_count, feedback_affected_task, on_project_change};
use crate::filter::Filter;
use crate::nag::nag;
use crate::recur::update_recurrence_mask;
use crate::shared::format::format;
use crate::shared::shared::with_console;
use crate::task::{ModType, Status};
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdStart` (`start`).
pub struct CmdStart {
    meta: CommandMeta,
}

impl CmdStart {
    /// C++ `CmdStart::CmdStart`.
    pub fn new() -> Self {
        CmdStart {
            meta: CommandMeta {
                keyword: "start".into(),
                usage: "task <filter> start <mods>".into(),
                description: "Marks specified task as started".into(),
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

impl Default for CmdStart {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdStart {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdStart::execute`.
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
        let mut modified = Vec::new();
        for mut task in filtered {
            if !task.has("start") {
                let before = task.clone();

                // Start the specified task.
                let question = format(
                    "Start task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );
                task.set_as_now("start");

                let status = task.get_status()?;
                if status == Status::Completed || status == Status::Deleted {
                    // "waiting" handled by Task::validate(), no special care needed here.
                    task.set_status(Status::Pending);
                }

                task.modify(context, ModType::Annotate, false)?;
                if context.config.get_boolean("journal.time") {
                    let annotation = context.config.get("journal.time.start.annotation");
                    task.add_annotation(&annotation);
                }

                let diff = before.diff(&task, &context.columns, &context.config);
                if self.permission(context, &(diff + &question), quantity) {
                    update_recurrence_mask(context, &task)?;
                    tdb2::modify(context, &mut task)?;
                    count += 1;
                    feedback_affected_task(context, "Starting task {1} '{2}'.", &task);
                    dependency_chain_on_start(context, &task)?;
                    if context.verbose("project") {
                        let message = on_project_change(context, &task, false)?;
                        project_changes.insert(task.get("project"), message);
                    }

                    // Save unmodified task for potential nagging later
                    modified.push(before);
                } else {
                    with_console(|_, stdout| {
                        let _ = stdout.write_all(b"Task not started.\n");
                        let _ = stdout.flush();
                    });
                    rc = 1;
                    if self.meta.permission_quit.get() {
                        break;
                    }
                }
            } else {
                // C++ uses the raw `task.id` here (0 for non-pending tasks),
                // not `identifier`.
                let line = format(
                    "Task {1} '{2}' already started.",
                    &[&task.id, &task.get("description")],
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
            if count == 1 {
                "Started {1} task."
            } else {
                "Started {1} tasks."
            },
            count,
        );
        Ok(rc)
    }
}
