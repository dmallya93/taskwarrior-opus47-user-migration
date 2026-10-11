//! Rust translation of `src/commands/CmdLog.cpp`.
//!
//! `task log <mods>`: add a task that is already completed. The task is
//! built from the command-line modifications, given status `completed`
//! (`TDB2::add`'s validation then supplies the `end` date), and refused if it
//! is recurring or waiting. Only the `new-uuid` verbosity token prints
//! `Logged task <uuid>.`; the `project` token footnotes the project summary.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::feedback::on_project_change;
use crate::shared::format::format;
use crate::task::{ModType, Status, Task};
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdLog` (`log`).
pub struct CmdLog {
    meta: CommandMeta,
}

impl CmdLog {
    /// C++ `CmdLog::CmdLog`.
    pub fn new() -> Self {
        CmdLog {
            meta: CommandMeta {
                keyword: "log".into(),
                usage: "task          log <mods>".into(),
                description: "Adds a new task that is already completed".into(),
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

impl Default for CmdLog {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdLog {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdLog::execute`.
    ///
    /// # Errors
    /// `You cannot log recurring tasks.`, `You cannot log waiting tasks.`,
    /// and those of `Task::modify` and `TDB2::add`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Apply the command line modifications to the new task.
        let mut task = Task::new();
        task.modify(context, ModType::Replace, true)?;
        task.set_status(Status::Completed);

        // Cannot log recurring tasks.
        if task.has("recur") {
            return Err(TwError::Message(
                "You cannot log recurring tasks.".to_string(),
            ));
        }

        // Cannot log waiting tasks.
        if task.has("wait") {
            return Err(TwError::Message(
                "You cannot log waiting tasks.".to_string(),
            ));
        }

        tdb2::add(context, &mut task)?;

        if context.verbose("project") {
            let message = on_project_change(context, &task, true)?;
            context.footnote(&message);
        }

        if context.verbose("new-uuid") {
            *output = format("Logged task {1}.\n", &[&task.get("uuid")]);
        }

        Ok(0)
    }
}
