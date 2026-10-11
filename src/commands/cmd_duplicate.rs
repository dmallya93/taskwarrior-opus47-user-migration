//! Rust translation of `src/commands/CmdDuplicate.cpp`.
//!
//! `task <filter> duplicate <mods>`: copy every filtered task under a new
//! UUID (without `start`, `end`, `entry`; a recurring child becomes a plain
//! task, a template stays a template without its `mask`), apply the
//! annotation modifications, ask permission, and store it with `TDB2::add`
//! (which runs the on-add hooks).


use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::feedback::{feedback_affected_count, feedback_affected_task, on_project_change};
use crate::filter::Filter;
use crate::shared::format::format;
use crate::shared::shared::with_console;
use crate::task::{ModType, Status};
use crate::tdb2;
use crate::util::uuid;

/// C++ `std::cout << text`, flushed so it interleaves with the other
/// standard output writers.
fn cout(text: &str) {
    with_console(|_, stdout| {
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    });
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDuplicate` (`duplicate`).
pub struct CmdDuplicate {
    meta: CommandMeta,
}

impl CmdDuplicate {
    /// C++ `CmdDuplicate::CmdDuplicate`.
    pub fn new() -> Self {
        CmdDuplicate {
            meta: CommandMeta {
                keyword: "duplicate".into(),
                usage: "task <filter> duplicate <mods>".into(),
                description: "Duplicates the specified tasks".into(),
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

impl Default for CmdDuplicate {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDuplicate {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDuplicate::execute`.
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

        let quantity = filtered.len() as u32;
        for task in &filtered {
            // Duplicate the specified task.
            let mut dup = task.clone();
            dup.id = 0; // Reset, and TDB2::add will set.
            dup.set("uuid", &uuid()); // Needs a new UUID.
            dup.remove("start"); // Does not inherit start date.
            dup.remove("end"); // Does not inherit end date.
            dup.remove("entry"); // Does not inherit entry date.

            // When duplicating a child task, downgrade it to a plain task.
            if dup.has("parent") {
                dup.remove("parent");
                dup.remove("recur");
                dup.remove("until");
                dup.remove("imask");
                cout(
                    &(format(
                        "Note: task {1} was a recurring task.  The duplicated task is not.",
                        &[&task.identifier(false)],
                    ) + "\n"),
                );
            }
            // When duplicating a parent task, create a new parent task.
            else if dup.get_status()? == Status::Recurring {
                dup.remove("mask");
                cout(
                    &(format(
                        "Note: task {1} was a parent recurring task.  The duplicated task is too.",
                        &[&task.identifier(false)],
                    ) + "\n"),
                );
            }

            dup.set_status(Status::Pending); // Does not inherit status.
                                             // Must occur after Task::recurring check.

            dup.modify(context, ModType::Annotate, false)?;

            let question = format(
                "Duplicate task {1} '{2}'?",
                &[&task.identifier(true), &task.get("description")],
            );
            if self.permission(context, &question, quantity) {
                tdb2::add(context, &mut dup)?;
                count += 1;
                feedback_affected_task(context, "Duplicated task {1} '{2}'.", task);

                let status = dup.get_status()?;
                if context.verbose("new-id")
                    && (status == Status::Pending || status == Status::Waiting)
                {
                    cout(&format("Created task {1}.\n", &[&dup.id]));
                } else if context.verbose("new-uuid") && status != Status::Recurring {
                    cout(&format("Created task {1}.\n", &[&dup.get("uuid")]));
                }

                if context.verbose("project") {
                    let message = on_project_change(context, task, true)?;
                    project_changes.insert(task.get("project"), message);
                }
            } else {
                cout("Task not duplicated.\n");
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
                "Duplicated {1} task."
            } else {
                "Duplicated {1} tasks."
            },
            count,
        );

        Ok(rc)
    }
}
