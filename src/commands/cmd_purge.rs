//! Rust translation of `src/commands/CmdPurge.cpp`.
//!
//! `task <filter> purge`: permanently remove the filtered deleted tasks from
//! the replica, after removing dependencies on them and (for recurrence
//! templates) purging their deleted children.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::feedback::feedback_affected_count;
use crate::filter::Filter;
use crate::shared::format::format;
use crate::shared::shared::confirm;
use crate::task::{Status, Task};
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdPurge` (`purge`).
pub struct CmdPurge {
    meta: CommandMeta,
}

impl CmdPurge {
    /// C++ `CmdPurge::CmdPurge`.
    pub fn new() -> Self {
        CmdPurge {
            meta: CommandMeta {
                keyword: "purge".into(),
                usage: "task <filter> purge".into(),
                description: "Removes the specified tasks from the data files. Causes permanent loss of data.".into(),
                read_only: false,
                displays_id: false,
                needs_confirm: true,
                needs_gc: true,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl CmdPurge {
    /// C++ `CmdPurge::handleRelations`: purges the task, while taking care of
    /// dependencies on this task and child tasks.
    fn handle_relations(ctx: &mut Context, task: &Task, tasks: &mut Vec<Task>) -> Result<()> {
        Self::handle_deps(ctx, task)?;
        Self::handle_children(ctx, task, tasks)?;
        tasks.push(task.clone());
        Ok(())
    }

    /// C++ `CmdPurge::handleDeps`: makes sure that any task having the
    /// dependency on the task being purged has that dependency removed, to
    /// preserve referential integrity.
    fn handle_deps(ctx: &mut Context, task: &Task) -> Result<()> {
        let uuid = task.get("uuid");

        for mut blocked in ctx.tdb2.all_tasks()? {
            if blocked.has_dependency(&uuid) {
                blocked.remove_dependency(&uuid)?;
                tdb2::modify(ctx, &mut blocked)?;
            }
        }
        Ok(())
    }

    /// C++ `CmdPurge::handleChildren`: makes sure that with any recurrence
    /// parent are all the child tasks removed as well. If user chooses not
    /// to, the whole command is aborted.
    fn handle_children(ctx: &mut Context, task: &Task, tasks: &mut Vec<Task>) -> Result<()> {
        // If this is not a recurrence parent, we have no job here
        if !task.has("mask") {
            return Ok(());
        }

        let uuid = task.get("uuid");
        let mut children = Vec::new();

        // Find all child tasks
        for child in ctx.tdb2.all_tasks()? {
            if child.get("parent") == uuid {
                if child.get_status()? != Status::Deleted {
                    // In case any child task is not deleted, bail out
                    return Err(TwError::Message(format(
                        "Task '{1}' is a recurrence template. Its child task {2} must be deleted before it can be purged.",
                        &[&task.get("description"), &child.identifier(true)],
                    )));
                } else {
                    children.push(child);
                }
            }
        }

        // If there are no children, our job is done
        if children.is_empty() {
            return Ok(());
        }

        // Ask for confirmation to purge them, if needed
        let question = format(
            "Task '{1}' is a recurrence template. All its {2} deleted children tasks will be purged as well. Continue?",
            &[&task.get("description"), &children.len()],
        );

        if ctx.config.get_boolean("recurrence.confirmation")
            || (ctx.config.get("recurrence.confirmation") == "prompt" && confirm(&question))
        {
            for child in &children {
                Self::handle_relations(ctx, child, tasks)?;
            }
            Ok(())
        } else {
            Err(TwError::Message("Purge operation aborted.".into()))
        }
    }
}

impl Default for CmdPurge {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdPurge {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdPurge::execute`.
    ///
    /// C++ quirk kept: a declined or quit prompt does not change the exit
    /// code, and the loop does not stop on `quit` (later prompts are simply
    /// refused by `permission`).
    fn execute(&self, context: &mut Context, _output: &mut String) -> Result<i32> {
        let rc = 0;
        let mut tasks: Vec<Task> = Vec::new();
        let mut matched_deleted = false;

        // Apply filter.
        let mut filtered = Vec::new();
        Filter::new().subset(context, &mut filtered)?;
        if filtered.is_empty() {
            context.footnote("No tasks specified.");
            return Ok(1);
        }

        let quantity = filtered.len() as u32;
        for task in &filtered {
            // Allow purging of deleted tasks only. Hence no need to deal with:
            // - unblocked tasks notifications (deleted tasks are not blocking)
            // - project changes (deleted tasks not included in progress)
            // It also has the nice property of being explicit - users need to
            // mark tasks as deleted before purging.
            if task.get_status()? == Status::Deleted {
                // Mark that at least one deleted task matched the filter
                matched_deleted = true;

                let question = format(
                    "Permanently remove task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );

                if self.permission(context, &question, quantity) {
                    Self::handle_relations(context, task, &mut tasks)?;
                }
            }
        }

        // Now that any exceptions are handled, actually purge the tasks.
        for task in &tasks {
            context.tdb2.purge(task)?;
        }

        if !filtered.is_empty() && !matched_deleted {
            context.footnote("No deleted tasks specified. Maybe you forgot to delete tasks first?");
        }

        feedback_affected_count(
            context,
            if tasks.len() == 1 {
                "Purged {1} task."
            } else {
                "Purged {1} tasks."
            },
            tasks.len() as i32,
        );
        Ok(rc)
    }
}
