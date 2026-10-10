//! Rust translation of `src/commands/CmdAdd.cpp`.
//!
//! `task add <mods>`: build a new task from the command-line modifications
//! (`Task::modify (modReplace, true)`), validate it for addition, store it
//! with `TDB2::add`, then report `Created task N.` / `Created task <uuid>.`
//! according to the `new-id` / `new-uuid` verbosity tokens, and footnote the
//! project-change summary under the `project` token.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::feedback::on_project_change;
use crate::shared::format::format;
use crate::task::{ModType, Status, Task};
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdAdd` (`add`).
pub struct CmdAdd {
    meta: CommandMeta,
}

impl CmdAdd {
    /// C++ `CmdAdd::CmdAdd`.
    pub fn new() -> Self {
        CmdAdd {
            meta: CommandMeta {
                keyword: "add".into(),
                usage: "task          add <mods>".into(),
                description: "Adds a new task".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: false,
                accepts_modifications: true,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdAdd {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdAdd {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdAdd::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply the command line modifications to the new task.
        let mut task = Task::new();

        // the task is empty, but DOM references can refer to earlier parts of the
        // command line, e.g., `task add due:20110101 wait:due`.
        task.modify(context, ModType::Replace, true)?;

        // Validate a task for addition. This is stricter than `task.validate`, as any
        // inconsistency is probably user error.
        task.validate_add(context)?;

        tdb2::add(context, &mut task)?;

        // Do not display ID 0, users cannot query by that
        let status = task.get_status()?;

        // We may have a situation where both new-id and new-uuid config
        // variables are set. In that case, we'll show the new-uuid, as
        // it's enduring and never changes, and it's unlikely the caller
        // asked for this if they just wanted a human-friendly number.

        if context.verbose("new-uuid") && status == Status::Recurring {
            output.push_str(&format(
                "Created task {1} (recurrence template).\n",
                &[&task.get("uuid")],
            ));
        } else if context.verbose("new-uuid")
            || (context.verbose("new-id") && (status == Status::Completed || status == Status::Deleted))
        {
            output.push_str(&format("Created task {1}.\n", &[&task.get("uuid")]));
        } else if context.verbose("new-id") && (status == Status::Pending || status == Status::Waiting) {
            output.push_str(&format("Created task {1}.\n", &[&task.id]));
        } else if context.verbose("new-id") && status == Status::Recurring {
            output.push_str(&format("Created task {1} (recurrence template).\n", &[&task.id]));
        }

        if context.verbose("project") {
            let message = on_project_change(context, &task, true)?;
            context.footnote(&message);
        }

        Ok(0)
    }
}
