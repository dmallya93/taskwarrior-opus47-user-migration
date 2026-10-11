//! Rust translation of `src/commands/CmdAppend.cpp`.
//!
//! `task <filter> append <mods>`: for every filtered task, appends the modification text to the description
//! (`Task::modify (Task::modAppend, true)`, which fails with "Additional text
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
/// C++ `CmdAppend` (`append`).
pub struct CmdAppend {
    meta: CommandMeta,
}

impl CmdAppend {
    /// C++ `CmdAppend::CmdAppend`.
    pub fn new() -> Self {
        CmdAppend {
            meta: CommandMeta {
                keyword: "append".into(),
                usage: "task <filter> append <mods>".into(),
                description: "Appends text to an existing task description".into(),
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
                "This is a recurring task.  Do you want to append to all pending recurrences of this same task?",
            ))
            || ctx.config.get_boolean("recurrence.confirmation")
    }
}

impl Default for CmdAppend {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdAppend {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdAppend::execute`.
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

            // Append to the specified task.
            let question = format(
                "Append to task {1} '{2}'?",
                &[&task.identifier(true), &task.get("description")],
            );

            task.modify(context, ModType::Append, true)?;

            let diff = before.diff(&task, &context.columns, &context.config);
            if self.permission(context, &(diff + &question), quantity) {
                tdb2::modify(context, &mut task)?;
                count += 1;
                feedback_affected_task(context, "Appending to task {1} '{2}'.", &task);
                if context.verbose("project") {
                    let message = on_project_change(context, &task, false)?;
                    project_changes.insert(task.get("project"), message);
                }

                // Append to siblings.
                if task.has("parent") && Self::recurrence_confirmed(context) {
                    let siblings = context.tdb2.siblings(&task)?;
                    for mut sibling in siblings {
                        sibling.modify(context, ModType::Append, true)?;
                        tdb2::modify(context, &mut sibling)?;
                        count += 1;
                        feedback_affected_task(context, "Appending to recurring task {1} '{2}'.", &sibling);
                    }

                    // Append to the parent
                    let mut parent = context
                        .tdb2
                        .get_by_uuid(task.get_ref("parent"))?
                        .unwrap_or_default();
                    parent.modify(context, ModType::Append, true)?;
                    tdb2::modify(context, &mut parent)?;
                }
            } else {
                with_console(|_, stdout| {
                    let _ = stdout.write_all(b"Task not appended.\n");
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
                "Appended {1} task."
            } else {
                "Appended {1} tasks."
            },
            count,
        );
        Ok(rc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::shared::{begin_console_override, end_console_override};
    use crate::test_util::CommandFixture;

    const ONE: &str = "aaaaaaaa-0000-4000-8000-000000000001";
    const TWO: &str = "aaaaaaaa-0000-4000-8000-000000000002";
    const PARENT: &str = "bbbbbbbb-0000-4000-8000-000000000001";
    const CHILD1: &str = "bbbbbbbb-0000-4000-8000-000000000002";
    const CHILD2: &str = "bbbbbbbb-0000-4000-8000-000000000003";

    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z","project":"home"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"two","status":"pending","entry":"20260101T000000Z"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000001","description":"rec","status":"recurring","entry":"20260101T000000Z","due":"20260101T000000Z","recur":"daily","mask":"--"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000002","description":"rec","status":"pending","entry":"20260101T000000Z","due":"20260101T000000Z","recur":"daily","parent":"bbbbbbbb-0000-4000-8000-000000000001","imask":"0"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000003","description":"rec","status":"pending","entry":"20260101T000000Z","due":"20260102T000000Z","recur":"daily","parent":"bbbbbbbb-0000-4000-8000-000000000001","imask":"1"}"#,
    ];

    fn fixture(rc_lines: &str, args: &[&str]) -> CommandFixture {
        CommandFixture::new(SEED, &format!("confirmation=off\n{rc_lines}"), args)
    }

    fn applied(f: &mut CommandFixture, uuid: &str) -> bool {
        f.task(uuid).get("description").ends_with(" note")
    }

    #[test]
    fn applies_the_text() {
        let mut f = fixture("", &["2", "append", "note"]);
        assert_eq!(f.run(&CmdAppend::new()).unwrap(), 0);
        assert!(applied(&mut f, TWO));
        assert!(!applied(&mut f, ONE));
    }

    #[test]
    fn missing_text_is_an_error() {
        let mut f = fixture("", &["2", "append"]);
        let err = f.run(&CmdAppend::new()).unwrap_err();
        assert_eq!(err.to_string(), "Additional text must be provided.");
        assert!(!applied(&mut f, TWO));
    }

    #[test]
    fn no_tasks_specified() {
        let mut f = fixture("", &["99", "append", "note"]);
        assert_eq!(f.run(&CmdAppend::new()).unwrap(), 1);
        assert!(
            f.ctx.footnotes.iter().any(|m| m == "No tasks specified."),
            "{:?}",
            f.ctx.footnotes
        );
    }

    #[test]
    fn recurrence_confirmation_yes_changes_siblings_and_parent() {
        let mut f = fixture("recurrence.confirmation=yes\n", &[CHILD1, "append", "note"]);
        assert_eq!(f.run(&CmdAppend::new()).unwrap(), 0);
        for uuid in [PARENT, CHILD1, CHILD2] {
            assert!(applied(&mut f, uuid), "{uuid}");
        }
    }

    #[test]
    fn recurrence_confirmation_no_changes_only_the_task() {
        let mut f = fixture("recurrence.confirmation=no\n", &[CHILD1, "append", "note"]);
        assert_eq!(f.run(&CmdAppend::new()).unwrap(), 0);
        assert!(applied(&mut f, CHILD1));
        assert!(!applied(&mut f, CHILD2));
        assert!(!applied(&mut f, PARENT));
    }

    #[test]
    fn recurrence_confirmation_prompt_asks() {
        let mut f = fixture("recurrence.confirmation=prompt\n", &[CHILD1, "append", "note"]);
        begin_console_override("yes\n");
        assert_eq!(f.run(&CmdAppend::new()).unwrap(), 0);
        let shown = end_console_override();
        assert_eq!(
            shown,
            "This is a recurring task.  Do you want to append to all pending recurrences of this same task? (yes/no) "
        );
        assert!(applied(&mut f, CHILD2));
        assert!(applied(&mut f, PARENT));
    }

    #[test]
    fn recurrence_confirmation_prompt_declined() {
        let mut f = fixture("recurrence.confirmation=prompt\n", &[CHILD1, "append", "note"]);
        begin_console_override("no\n");
        assert_eq!(f.run(&CmdAppend::new()).unwrap(), 0);
        end_console_override();
        assert!(applied(&mut f, CHILD1));
        assert!(!applied(&mut f, CHILD2));
        assert!(!applied(&mut f, PARENT));
    }

    #[test]
    fn declined_permission_returns_1() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1", "2", "append", "note"]);
        begin_console_override("no\nno\n");
        let rc = f.run(&CmdAppend::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert!(
            shown.contains("Append to task 2 'two'? (yes/no/all/quit) Task not appended.\n"),
            "{shown}"
        );
        assert_eq!(shown.matches("Task not appended.").count(), 2, "{shown}");
        assert!(!applied(&mut f, TWO));
    }

    #[test]
    fn bulk_quit_stops_the_loop() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1", "2", "append", "note"]);
        begin_console_override("quit\n");
        let rc = f.run(&CmdAppend::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert_eq!(shown.matches("Task not appended.").count(), 1, "{shown}");
        assert!(!applied(&mut f, ONE));
        assert!(!applied(&mut f, TWO));
    }

    #[test]
    fn bulk_all_changes_every_task() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1", "2", "append", "note"]);
        begin_console_override("all\n");
        assert_eq!(f.run(&CmdAppend::new()).unwrap(), 0);
        end_console_override();
        assert!(applied(&mut f, ONE));
        assert!(applied(&mut f, TWO));
    }

    #[test]
    fn project_change_footnoted() {
        let mut f = fixture("verbose=project\n", &["1", "append", "note"]);
        assert_eq!(f.run(&CmdAppend::new()).unwrap(), 0);
        assert!(
            f.ctx.footnotes.iter().any(|m| m
                == "Project 'home' is 0% complete (1 task remaining)."),
            "{:?}",
            f.ctx.footnotes
        );
    }
}
