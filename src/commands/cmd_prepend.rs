//! Rust translation of `src/commands/CmdPrepend.cpp`.
//!
//! `task <filter> prepend <mods>`: for every filtered task, prepends the modification text to the description
//! (`Task::modify (Task::modPrepend, true)`, which fails with "Additional text
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
/// C++ `CmdPrepend` (`prepend`).
pub struct CmdPrepend {
    meta: CommandMeta,
}

impl CmdPrepend {
    /// C++ `CmdPrepend::CmdPrepend`.
    pub fn new() -> Self {
        CmdPrepend {
            meta: CommandMeta {
                keyword: "prepend".into(),
                usage: "task <filter> prepend <mods>".into(),
                description: "Prepends text to an existing task description".into(),
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

    /// C++ `(config.get ("recurrence.confirmation") == "prompt" && confirm
    /// (...)) || config.getBoolean ("recurrence.confirmation")`.
    fn recurrence_confirmed(ctx: &Context) -> bool {
        (ctx.config.get("recurrence.confirmation") == "prompt"
            && confirm(
                "This is a recurring task.  Do you want to prepend to all pending recurrences of this same task?",
            ))
            || ctx.config.get_boolean("recurrence.confirmation")
    }
}

impl Default for CmdPrepend {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdPrepend {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdPrepend::execute`.
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

            // Prepend to the specified task.
            let question = format(
                "Prepend to task {1} '{2}'?",
                &[&task.identifier(true), &task.get("description")],
            );

            task.modify(context, ModType::Prepend, true)?;

            let diff = before.diff(&task, &context.columns, &context.config);
            if self.permission(context, &(diff + &question), quantity) {
                tdb2::modify(context, &mut task)?;
                count += 1;
                feedback_affected_task(context, "Prepending to task {1} '{2}'.", &task);
                if context.verbose("project") {
                    let message = on_project_change(context, &task, false)?;
                    project_changes.insert(task.get("project"), message);
                }

                // Prepend to siblings.
                if task.has("parent") && Self::recurrence_confirmed(context) {
                    let siblings = context.tdb2.siblings(&task)?;
                    for mut sibling in siblings {
                        sibling.modify(context, ModType::Prepend, true)?;
                        tdb2::modify(context, &mut sibling)?;
                        count += 1;
                        feedback_affected_task(context, "Prepending to recurring task {1} '{2}'.", &sibling);
                    }

                    // Prepend to the parent
                    let mut parent = context
                        .tdb2
                        .get_by_uuid(task.get_ref("parent"))?
                        .unwrap_or_default();
                    parent.modify(context, ModType::Prepend, true)?;
                    tdb2::modify(context, &mut parent)?;
                }
            } else {
                with_console(|_, stdout| {
                    let _ = stdout.write_all(b"Task not prepended.\n");
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
                "Prepended {1} task."
            } else {
                "Prepended {1} tasks."
            },
            count,
        );
        Ok(rc)
    }
}
