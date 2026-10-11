//! Rust translation of `src/commands/CmdDelete.cpp`.
//!
//! `task <filter> delete <mods>`: mark every filtered, not yet deleted task
//! as deleted (annotation modifications, `end` stamp), ask permission, store
//! it with `TDB2::modify` (which runs the on-modify hooks), and delete the
//! pending recurrence siblings and parent (child task) or children
//! (template) according to `rc.recurrence.confirmation`.

use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::dependency::dependency_chain_on_complete;
use crate::error::Result;
use crate::feedback::{feedback_affected_count, feedback_affected_task, feedback_unblocked, on_project_change};
use crate::filter::Filter;
use crate::recur::update_recurrence_mask;
use crate::shared::format::format;
use crate::shared::shared::{confirm, with_console};
use crate::task::{ModType, Status, Task};
use crate::tdb2;

const STRING_CMD_DELETE_TASK_R: &str = "Deleting recurring task {1} '{2}'.";
const STRING_CMD_DELETE_CONFIRM_R: &str =
    "This is a recurring task.  Do you want to delete all pending recurrences of this same task?";

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDelete` (`delete`).
pub struct CmdDelete {
    meta: CommandMeta,
}

impl CmdDelete {
    /// C++ `CmdDelete::CmdDelete`.
    pub fn new() -> Self {
        CmdDelete {
            meta: CommandMeta {
                keyword: "delete".into(),
                usage: "task <filter> delete <mods>".into(),
                description: "Deletes the specified task".into(),
                read_only: false,
                displays_id: false,
                needs_confirm: true,
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

impl CmdDelete {
    /// C++ `(config.get ("recurrence.confirmation") == "prompt" && confirm
    /// (STRING_CMD_DELETE_CONFIRM_R)) || config.getBoolean
    /// ("recurrence.confirmation")`.
    fn recurrence_confirmed(ctx: &mut Context) -> bool {
        (ctx.config.get("recurrence.confirmation") == "prompt" && confirm(STRING_CMD_DELETE_CONFIRM_R))
            || ctx.config.get_boolean("recurrence.confirmation")
    }

    /// The C++ loop body shared by the sibling and child deletions:
    /// annotate, mark deleted, stamp `end`, store and report.
    fn delete_recurrence(ctx: &mut Context, mut task: Task) -> Result<()> {
        task.modify(ctx, ModType::Annotate, false)?;
        task.set_status(Status::Deleted);
        if !task.has("end") {
            task.set_as_now("end");
        }

        update_recurrence_mask(ctx, &task)?;
        tdb2::modify(ctx, &mut task)?;
        feedback_affected_task(ctx, STRING_CMD_DELETE_TASK_R, &task);
        feedback_unblocked(ctx, &task)?;
        Ok(())
    }
}

impl Default for CmdDelete {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDelete {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDelete::execute`.
    ///
    /// C++ quirk kept: the parent of a deleted child is loaded with
    /// `TDB2::get`, which leaves an empty task when the parent is missing;
    /// that empty task is still marked deleted and stored.
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
        for mut task in filtered {
            if task.get_status()? != Status::Deleted {
                // Delete the specified task.
                let question = format(
                    "Delete task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );

                task.modify(context, ModType::Annotate, false)?;
                task.set_status(Status::Deleted);
                if !task.has("end") {
                    task.set_as_now("end");
                }

                if self.permission(context, &question, quantity) {
                    update_recurrence_mask(context, &task)?;
                    count += 1;
                    tdb2::modify(context, &mut task)?;
                    feedback_affected_task(context, "Deleting task {1} '{2}'.", &task);
                    feedback_unblocked(context, &task)?;
                    dependency_chain_on_complete(context, &task)?;
                    if context.verbose("project") {
                        let message = on_project_change(context, &task, true)?;
                        project_changes.insert(task.get("project"), message);
                    }

                    // Delete siblings.
                    if task.has("parent") {
                        if Self::recurrence_confirmed(context) {
                            let siblings = context.tdb2.siblings(&task)?;
                            for sibling in siblings {
                                Self::delete_recurrence(context, sibling)?;
                                count += 1;
                            }

                            // Delete the parent
                            let mut parent = context.tdb2.get_by_uuid(task.get_ref("parent"))?.unwrap_or_default();
                            parent.set_status(Status::Deleted);
                            if !parent.has("end") {
                                parent.set_as_now("end");
                            }

                            tdb2::modify(context, &mut parent)?;
                        }
                    }
                    // Task potentially has child tasks - optionally delete them.
                    else {
                        let children = context.tdb2.children(&task)?;
                        if !children.is_empty() && Self::recurrence_confirmed(context) {
                            for child in children {
                                Self::delete_recurrence(context, child)?;
                                count += 1;
                            }
                        }
                    }
                } else {
                    with_console(|_, stdout| {
                        let _ = stdout.write_all(b"Task not deleted.\n");
                        let _ = stdout.flush();
                    });
                    rc = 1;
                    if self.meta.permission_quit.get() {
                        break;
                    }
                }
            } else {
                let line = format(
                    "Task {1} '{2}' is not deletable.",
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
            if count == 1 { "Deleted {1} task." } else { "Deleted {1} tasks." },
            count,
        );

        Ok(rc)
    }
}
