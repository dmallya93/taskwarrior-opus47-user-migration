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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::shared::{begin_console_override, end_console_override};
    use crate::test_util::CommandFixture;

    const PENDING: &str = "aaaaaaaa-0000-4000-8000-000000000001";
    const GONE: &str = "aaaaaaaa-0000-4000-8000-000000000002";
    const GONE2: &str = "aaaaaaaa-0000-4000-8000-000000000003";
    const PARENT: &str = "bbbbbbbb-0000-4000-8000-000000000001";
    const CHILD1: &str = "bbbbbbbb-0000-4000-8000-000000000002";
    const CHILD2: &str = "bbbbbbbb-0000-4000-8000-000000000003";

    /// A pending task depending on a deleted task, a second deleted task,
    /// and a deleted recurrence template with two deleted children.
    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"pending","status":"pending","entry":"20260101T000000Z","depends":"aaaaaaaa-0000-4000-8000-000000000002"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"gone","status":"deleted","entry":"20260101T000000Z","end":"20260102T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000003","description":"gone2","status":"deleted","entry":"20260101T000000Z","end":"20260102T000000Z"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000001","description":"rec","status":"deleted","entry":"20260101T000000Z","end":"20260102T000000Z","due":"20260101T000000Z","recur":"daily","mask":"XX"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000002","description":"rec","status":"deleted","entry":"20260101T000000Z","end":"20260102T000000Z","due":"20260101T000000Z","recur":"daily","parent":"bbbbbbbb-0000-4000-8000-000000000001","imask":"0"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000003","description":"rec","status":"deleted","entry":"20260101T000000Z","end":"20260102T000000Z","due":"20260102T000000Z","recur":"daily","parent":"bbbbbbbb-0000-4000-8000-000000000001","imask":"1"}"#,
    ];

    fn fixture(rc_lines: &str, args: &[&str]) -> CommandFixture {
        CommandFixture::new(SEED, &format!("confirmation=off\n{rc_lines}"), args)
    }

    fn exists(f: &mut CommandFixture, uuid: &str) -> bool {
        f.ctx.tdb2.get_by_uuid(uuid).unwrap().is_some()
    }

    #[test]
    fn purges_deleted_task_and_removes_dependency() {
        let mut f = fixture("", &[GONE, "purge"]);
        assert_eq!(f.run(&CmdPurge::new()).unwrap(), 0);
        assert!(!exists(&mut f, GONE));
        assert!(!f.task(PENDING).has_dependency(GONE));
        assert!(exists(&mut f, GONE2));
    }

    #[test]
    fn no_tasks_specified() {
        let mut f = fixture("", &["99", "purge"]);
        assert_eq!(f.run(&CmdPurge::new()).unwrap(), 1);
        assert_eq!(f.ctx.footnotes, vec!["No tasks specified.".to_string()]);
    }

    #[test]
    fn non_deleted_task_is_not_purged() {
        let mut f = fixture("", &[PENDING, "purge"]);
        assert_eq!(f.run(&CmdPurge::new()).unwrap(), 0);
        assert!(exists(&mut f, PENDING));
        assert_eq!(
            f.ctx.footnotes,
            vec!["No deleted tasks specified. Maybe you forgot to delete tasks first?".to_string()]
        );
    }

    #[test]
    fn confirmation_declined_keeps_task() {
        let mut f = fixture("confirmation=on\n", &[GONE, "purge"]);
        begin_console_override("no\n");
        let rc = f.run(&CmdPurge::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 0);
        assert_eq!(shown, "Permanently remove task aaaaaaaa 'gone'? (yes/no) ");
        assert!(exists(&mut f, GONE));
        assert!(f.task(PENDING).has_dependency(GONE));
    }

    #[test]
    fn confirmation_accepted_purges() {
        let mut f = fixture("confirmation=on\n", &[GONE, "purge"]);
        begin_console_override("yes\n");
        assert_eq!(f.run(&CmdPurge::new()).unwrap(), 0);
        end_console_override();
        assert!(!exists(&mut f, GONE));
    }

    #[test]
    fn template_purges_children_with_confirmation_yes() {
        let mut f = fixture("recurrence.confirmation=yes\n", &[PARENT, "purge"]);
        assert_eq!(f.run(&CmdPurge::new()).unwrap(), 0);
        for uuid in [PARENT, CHILD1, CHILD2] {
            assert!(!exists(&mut f, uuid), "{uuid}");
        }
    }

    #[test]
    fn template_prompt_accepted_purges_children() {
        let mut f = fixture("recurrence.confirmation=prompt\n", &[PARENT, "purge"]);
        begin_console_override("yes\n");
        assert_eq!(f.run(&CmdPurge::new()).unwrap(), 0);
        let shown = end_console_override();
        assert_eq!(
            shown,
            "Task 'rec' is a recurrence template. All its 2 deleted children tasks will be purged as well. Continue? (yes/no) "
        );
        for uuid in [PARENT, CHILD1, CHILD2] {
            assert!(!exists(&mut f, uuid), "{uuid}");
        }
    }

    #[test]
    fn template_prompt_declined_aborts() {
        let mut f = fixture("recurrence.confirmation=prompt\n", &[PARENT, "purge"]);
        begin_console_override("no\n");
        let err = f.run(&CmdPurge::new()).unwrap_err();
        end_console_override();
        assert_eq!(err.to_string(), "Purge operation aborted.");
        for uuid in [PARENT, CHILD1, CHILD2] {
            assert!(exists(&mut f, uuid), "{uuid}");
        }
    }

    #[test]
    fn template_confirmation_no_aborts() {
        let mut f = fixture("recurrence.confirmation=no\n", &[PARENT, "purge"]);
        let err = f.run(&CmdPurge::new()).unwrap_err();
        assert_eq!(err.to_string(), "Purge operation aborted.");
        assert!(exists(&mut f, PARENT));
    }

    #[test]
    fn template_with_pending_child_cannot_be_purged() {
        let mut f = fixture("recurrence.confirmation=yes\n", &[PARENT, "purge"]);
        let mut child = f.task(CHILD2);
        child.set_status(Status::Pending);
        child.remove("end");
        tdb2::modify(&mut f.ctx, &mut child).unwrap();
        let id = f.task(CHILD2).identifier(true);
        let err = f.run(&CmdPurge::new()).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Task 'rec' is a recurrence template. Its child task {id} must be deleted before it can be purged."
            )
        );
        assert!(exists(&mut f, PARENT));
        assert!(exists(&mut f, CHILD1));
    }

    #[test]
    fn bulk_quit_purges_nothing_further() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &[GONE, GONE2, "purge"]);
        begin_console_override("quit\n");
        assert_eq!(f.run(&CmdPurge::new()).unwrap(), 0);
        let shown = end_console_override();
        // One prompt only (in filter order); `quit` refuses the rest.
        assert!(
            shown.starts_with("Permanently remove task aaaaaaaa 'gone")
                && shown.ends_with("'? (yes/no/all/quit) ")
                && shown.matches('?').count() == 1,
            "{shown:?}"
        );
        assert!(exists(&mut f, GONE));
        assert!(exists(&mut f, GONE2));
    }
}
