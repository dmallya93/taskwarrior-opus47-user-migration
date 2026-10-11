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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::shared::{begin_console_override, end_console_override};
    use crate::task::Task;
    use crate::test_util::CommandFixture;

    const ONE: &str = "aaaaaaaa-0000-4000-8000-000000000001";
    const PARENT: &str = "bbbbbbbb-0000-4000-8000-000000000001";
    const CHILD1: &str = "bbbbbbbb-0000-4000-8000-000000000002";

    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z","start":"20260102T000000Z","project":"home"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"two","status":"completed","entry":"20260101T000000Z","end":"20260103T000000Z"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000001","description":"rec","status":"recurring","entry":"20260101T000000Z","due":"20260101T000000Z","recur":"daily","mask":"--"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000002","description":"rec","status":"pending","entry":"20260101T000000Z","due":"20260101T000000Z","recur":"daily","until":"20270101T000000Z","parent":"bbbbbbbb-0000-4000-8000-000000000001","imask":"0"}"#,
    ];

    fn fixture(rc_lines: &str, args: &[&str]) -> CommandFixture {
        CommandFixture::new(SEED, &format!("confirmation=off\n{rc_lines}"), args)
    }

    /// The tasks with `description` other than the seeded ones.
    fn duplicates(f: &mut CommandFixture, description: &str) -> Vec<Task> {
        let seeded = [ONE, "aaaaaaaa-0000-4000-8000-000000000002", PARENT, CHILD1];
        f.ctx
            .tdb2
            .all_tasks()
            .unwrap()
            .into_iter()
            .filter(|t| {
                !seeded.contains(&t.get_ref("uuid")) && t.get_ref("description") == description
            })
            .collect()
    }

    #[test]
    fn copies_without_start_end_entry_and_applies_modifications() {
        let mut f = fixture("", &["1", "duplicate", "project:work", "+tagged"]);
        begin_console_override("");
        assert_eq!(f.run(&CmdDuplicate::new()).unwrap(), 0);
        assert_eq!(end_console_override(), "");
        let dups = duplicates(&mut f, "one");
        assert_eq!(dups.len(), 1);
        let dup = &dups[0];
        assert_ne!(dup.get_ref("uuid"), ONE);
        assert!(!dup.has("start"));
        assert!(!dup.has("end"));
        // TDB2::add gives a new entry date.
        assert_ne!(dup.get_ref("entry"), f.task(ONE).get_ref("entry"));
        assert_eq!(dup.get_ref("project"), "work");
        assert!(dup.get_tags().contains(&"tagged".to_string()));
        assert_eq!(dup.get_status().unwrap(), Status::Pending);
        // The original is untouched.
        assert_eq!(f.task(ONE).get_ref("project"), "home");
    }

    #[test]
    fn completed_task_becomes_pending() {
        let mut f = fixture("", &["aaaaaaaa-0000-4000-8000-000000000002", "duplicate"]);
        assert_eq!(f.run(&CmdDuplicate::new()).unwrap(), 0);
        let dups = duplicates(&mut f, "two");
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].get_status().unwrap(), Status::Pending);
        assert!(!dups[0].has("end"));
    }

    #[test]
    fn child_becomes_a_plain_task() {
        let mut f = fixture("", &[CHILD1, "duplicate"]);
        begin_console_override("");
        assert_eq!(f.run(&CmdDuplicate::new()).unwrap(), 0);
        let id = f.task(CHILD1).id;
        assert_eq!(
            end_console_override(),
            format!("Note: task {id} was a recurring task.  The duplicated task is not.\n")
        );
        let dups: Vec<Task> = duplicates(&mut f, "rec")
            .into_iter()
            .filter(|t| t.get_ref("status") == "pending")
            .collect();
        assert_eq!(dups.len(), 1);
        for attribute in ["parent", "recur", "until", "imask"] {
            assert!(!dups[0].has(attribute), "{attribute}");
        }
        assert!(dups[0].has("due"));
    }

    #[test]
    fn template_stays_a_template_without_mask() {
        let mut f = fixture("", &[PARENT, "duplicate"]);
        begin_console_override("");
        assert_eq!(f.run(&CmdDuplicate::new()).unwrap(), 0);
        let id = f.task(PARENT).id;
        assert_eq!(
            end_console_override(),
            format!("Note: task {id} was a parent recurring task.  The duplicated task is too.\n")
        );
        let dups = duplicates(&mut f, "rec");
        assert_eq!(dups.len(), 1);
        // Status::Pending, then TDB2::add validation restores `recurring`
        // for a task with `recur` and `due`.
        assert!(!dups[0].has("mask"));
        assert_eq!(dups[0].get_ref("recur"), "daily");
    }

    #[test]
    fn verbose_feedback_new_id_and_project() {
        let mut f = fixture("verbose=affected,new-id,project\n", &["1", "duplicate"]);
        begin_console_override("");
        assert_eq!(f.run(&CmdDuplicate::new()).unwrap(), 0);
        let shown = end_console_override();
        let dup = duplicates(&mut f, "one").remove(0);
        assert_eq!(shown, format!("Created task {}.\n", dup.id));
        assert!(
            f.ctx
                .footnotes
                .iter()
                .any(|m| m.starts_with("The project 'home' has changed.")),
            "{:?}",
            f.ctx.footnotes
        );
    }

    #[test]
    fn verbose_feedback_new_uuid() {
        let mut f = fixture("verbose=new-uuid\n", &["1", "duplicate"]);
        begin_console_override("");
        assert_eq!(f.run(&CmdDuplicate::new()).unwrap(), 0);
        let shown = end_console_override();
        let dup = duplicates(&mut f, "one").remove(0);
        assert_eq!(shown, format!("Created task {}.\n", dup.get_ref("uuid")));
    }

    #[test]
    fn declined_permission_is_rc_1() {
        let mut f =
            CommandFixture::new(SEED, "confirmation=on\n", &["1", "duplicate", "/one/uno/"]);
        // A modification other than annotation needs no bulk prompt for one
        // task, so permission is granted without asking.
        assert_eq!(f.run(&CmdDuplicate::new()).unwrap(), 0);
        drop(f);

        let mut f = CommandFixture::new(SEED, "confirmation=on\nbulk=1\n", &["1,2", "duplicate"]);
        begin_console_override("no\nno\n");
        assert_eq!(f.run(&CmdDuplicate::new()).unwrap(), 1);
        let shown = end_console_override();
        assert!(shown.contains("Task not duplicated.\n"), "{shown}");
        assert!(duplicates(&mut f, "one").is_empty());
    }

    #[test]
    fn no_tasks_specified() {
        let mut f = fixture("", &["99", "duplicate"]);
        assert_eq!(f.run(&CmdDuplicate::new()).unwrap(), 1);
        assert!(f.ctx.footnotes.iter().any(|m| m == "No tasks specified."));
    }
}
