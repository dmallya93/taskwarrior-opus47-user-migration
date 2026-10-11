//! Rust translation of `src/commands/CmdAnnotate.cpp`.
//!
//! `task <filter> annotate <mods>`: for every filtered task, adds the modification text as a new annotation
//! (`Task::modify (Task::modAnnotate, true)`, which fails with "Additional text
//! must be provided." when there is none), ask permission with the task
//! diff, store it with `TDB2::modify`, and for a recurring child optionally
//! apply the same change to its pending siblings and its parent according
//! to `rc.recurrence.confirmation`.

use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::feedback::{feedback_affected_count, feedback_affected_task, on_project_change};
use crate::filter::Filter;
use crate::shared::format::format;
use crate::shared::shared::{confirm, with_console};
use crate::task::ModType;
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdAnnotate` (`annotate`).
pub struct CmdAnnotate {
    meta: CommandMeta,
}

impl CmdAnnotate {
    /// C++ `CmdAnnotate::CmdAnnotate`.
    pub fn new() -> Self {
        CmdAnnotate {
            meta: CommandMeta {
                keyword: "annotate".into(),
                usage: "task <filter> annotate <mods>".into(),
                description: "Adds an annotation to an existing task".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: true,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }

    /// C++ `(config.get ("recurrence.confirmation") == "prompt" && confirm
    /// (...)) || config.getBoolean ("recurrence.confirmation")`.
    fn recurrence_confirmed(ctx: &Context) -> bool {
        (ctx.config.get("recurrence.confirmation") == "prompt"
            && confirm(
                "This is a recurring task.  Do you want to annotate all pending recurrences of this same task?",
            ))
            || ctx.config.get_boolean("recurrence.confirmation")
    }
}

impl Default for CmdAnnotate {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdAnnotate {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdAnnotate::execute`.
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
            let before = task.clone();

            // Annotate the specified task.
            let question = format(
                "Annotate task {1} '{2}'?",
                &[&task.identifier(true), &task.get("description")],
            );

            task.modify(context, ModType::Annotate, true)?;

            let diff = before.diff(&task, &context.columns, &context.config);
            if self.permission(context, &(diff + &question), quantity) {
                tdb2::modify(context, &mut task)?;
                count += 1;
                feedback_affected_task(context, "Annotating task {1} '{2}'.", &task);
                if context.verbose("project") {
                    let message = on_project_change(context, &task, false)?;
                    project_changes.insert(task.get("project"), message);
                }

                // Annotate siblings.
                if task.has("parent") && Self::recurrence_confirmed(context) {
                    let siblings = context.tdb2.siblings(&task)?;
                    for mut sibling in siblings {
                        sibling.modify(context, ModType::Annotate, true)?;
                        tdb2::modify(context, &mut sibling)?;
                        count += 1;
                        feedback_affected_task(context, "Annotating recurring task {1} '{2}'.", &sibling);
                    }

                    // Annotate the parent
                    let mut parent = context
                        .tdb2
                        .get_by_uuid(task.get_ref("parent"))?
                        .unwrap_or_default();
                    parent.modify(context, ModType::Annotate, true)?;
                    tdb2::modify(context, &mut parent)?;
                }
            } else {
                with_console(|_, stdout| {
                    let _ = stdout.write_all(b"Task not annotated.\n");
                    let _ = stdout.flush();
                });
                rc = 1;
                if self.meta.permission_quit.get() {
                    break;
                }
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
                "Annotated {1} task."
            } else {
                "Annotated {1} tasks."
            },
            count,
        );
        Ok(rc)
    }
}
