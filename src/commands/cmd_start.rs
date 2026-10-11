//! Rust translation of `src/commands/CmdStart.cpp`.
//!
//! `task <filter> start <mods>`: stamp `start` on every filtered task that is
//! not already started (reopening completed/deleted tasks as pending), apply
//! annotation modifications and the optional `rc.journal.time` annotation,
//! ask permission with the task diff, store it with `TDB2::modify`, run the
//! dependency chain check, then nag about more urgent tasks.

use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::dependency::dependency_chain_on_start;
use crate::error::Result;
use crate::feedback::{feedback_affected_count, feedback_affected_task, on_project_change};
use crate::filter::Filter;
use crate::nag::nag;
use crate::recur::update_recurrence_mask;
use crate::shared::format::format;
use crate::shared::shared::with_console;
use crate::task::{ModType, Status};
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdStart` (`start`).
pub struct CmdStart {
    meta: CommandMeta,
}

impl CmdStart {
    /// C++ `CmdStart::CmdStart`.
    pub fn new() -> Self {
        CmdStart {
            meta: CommandMeta {
                keyword: "start".into(),
                usage: "task <filter> start <mods>".into(),
                description: "Marks specified task as started".into(),
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

impl Default for CmdStart {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdStart {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdStart::execute`.
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
        let mut modified = Vec::new();
        for mut task in filtered {
            if !task.has("start") {
                let before = task.clone();

                // Start the specified task.
                let question = format(
                    "Start task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );
                task.set_as_now("start");

                let status = task.get_status()?;
                if status == Status::Completed || status == Status::Deleted {
                    // "waiting" handled by Task::validate(), no special care needed here.
                    task.set_status(Status::Pending);
                }

                task.modify(context, ModType::Annotate, false)?;
                if context.config.get_boolean("journal.time") {
                    let annotation = context.config.get("journal.time.start.annotation");
                    task.add_annotation(&annotation);
                }

                let diff = before.diff(&task, &context.columns, &context.config);
                if self.permission(context, &(diff + &question), quantity) {
                    update_recurrence_mask(context, &task)?;
                    tdb2::modify(context, &mut task)?;
                    count += 1;
                    feedback_affected_task(context, "Starting task {1} '{2}'.", &task);
                    dependency_chain_on_start(context, &task)?;
                    if context.verbose("project") {
                        let message = on_project_change(context, &task, false)?;
                        project_changes.insert(task.get("project"), message);
                    }

                    // Save unmodified task for potential nagging later
                    modified.push(before);
                } else {
                    with_console(|_, stdout| {
                        let _ = stdout.write_all(b"Task not started.\n");
                        let _ = stdout.flush();
                    });
                    rc = 1;
                    if self.meta.permission_quit.get() {
                        break;
                    }
                }
            } else {
                // C++ uses the raw `task.id` here (0 for non-pending tasks),
                // not `identifier`.
                let line = format(
                    "Task {1} '{2}' already started.",
                    &[&task.id, &task.get("description")],
                ) + "\n";
                with_console(|_, stdout| {
                    let _ = stdout.write_all(line.as_bytes());
                    let _ = stdout.flush();
                });
                rc = 1;
            }
        }

        nag(context, &mut modified)?;

        // Now list the project changes.
        for (project, message) in &project_changes {
            if !project.is_empty() {
                context.footnote(message);
            }
        }

        feedback_affected_count(
            context,
            if count == 1 {
                "Started {1} task."
            } else {
                "Started {1} tasks."
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
    const THREE: &str = "aaaaaaaa-0000-4000-8000-000000000003";
    const FOUR: &str = "aaaaaaaa-0000-4000-8000-000000000004";

    /// ID 1 started with a project, ID 2 plain with a project, a completed
    /// task, and a completed task that is still started.
    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z","project":"home","start":"20260101T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"two","status":"pending","entry":"20260101T000000Z","project":"work"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000003","description":"three","status":"completed","entry":"20260101T000000Z","end":"20260102T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000004","description":"four","status":"completed","entry":"20260101T000000Z","end":"20260102T000000Z","start":"20260101T000000Z"}"#,
    ];

    fn fixture(rc_lines: &str, args: &[&str]) -> CommandFixture {
        CommandFixture::new(SEED, &format!("confirmation=off\n{rc_lines}"), args)
    }

    #[test]
    fn starts_and_stamps_start() {
        let mut f = fixture("", &["2", "start", "+go"]);
        assert_eq!(f.run(&CmdStart::new()).unwrap(), 0);
        let t = f.task(TWO);
        assert!(t.has("start"));
        assert_eq!(t.get("status"), "pending");
        assert!(t.get_tags().contains(&"go".to_string()));
    }

    #[test]
    fn already_started_uses_raw_id() {
        let mut f = fixture("", &["1", "start"]);
        begin_console_override("");
        let rc = f.run(&CmdStart::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert_eq!(shown, "Task 1 'one' already started.\n");
        assert_eq!(f.task(ONE).get("start"), "1767225600");

        // A completed task has id 0, which C++ prints verbatim.
        drop(f);
        let mut f2 = fixture("", &[FOUR, "start"]);
        begin_console_override("");
        let rc = f2.run(&CmdStart::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert_eq!(shown, "Task 0 'four' already started.\n");
    }

    #[test]
    fn completed_task_is_reopened() {
        let mut f = fixture("", &[THREE, "start"]);
        assert_eq!(f.run(&CmdStart::new()).unwrap(), 0);
        let t = f.task(THREE);
        assert_eq!(t.get("status"), "pending");
        assert!(t.has("start"));
    }

    #[test]
    fn journal_time_start_annotation() {
        let mut f = fixture(
            "journal.time=on\njournal.time.start.annotation=Go go\n",
            &["2", "start"],
        );
        assert_eq!(f.run(&CmdStart::new()).unwrap(), 0);
        let annotations: Vec<String> = f.task(TWO).get_annotations().values().cloned().collect();
        assert_eq!(annotations, vec!["Go go".to_string()]);
    }

    #[test]
    fn no_journal_no_annotation() {
        let mut f = fixture("", &["2", "start"]);
        assert_eq!(f.run(&CmdStart::new()).unwrap(), 0);
        assert!(f.task(TWO).get_annotations().is_empty());
    }

    #[test]
    fn no_tasks_specified() {
        let mut f = fixture("", &["99", "start"]);
        assert_eq!(f.run(&CmdStart::new()).unwrap(), 1);
        assert!(
            f.ctx.footnotes.iter().any(|m| m == "No tasks specified."),
            "{:?}",
            f.ctx.footnotes
        );
    }

    #[test]
    fn single_task_needs_no_confirmation() {
        let mut f = fixture("confirmation=on\n", &["2", "start"]);
        begin_console_override("");
        assert_eq!(f.run(&CmdStart::new()).unwrap(), 0);
        let shown = end_console_override();
        assert!(!shown.contains("(yes/no)"), "{shown}");
        assert!(f.task(TWO).has("start"));
    }

    #[test]
    fn declined_permission_returns_1() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &[THREE, "2", "start"]);
        begin_console_override("no\nno\n");
        let rc = f.run(&CmdStart::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert!(
            shown.contains("Start task 2 'two'? (yes/no/all/quit) Task not started.\n"),
            "{shown}"
        );
        assert_eq!(shown.matches("Task not started.").count(), 2, "{shown}");
        assert!(!f.task(TWO).has("start"));
    }

    #[test]
    fn bulk_quit_stops_the_loop() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &[THREE, "2", "start"]);
        begin_console_override("quit\n");
        let rc = f.run(&CmdStart::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert_eq!(shown.matches("Task not started.").count(), 1, "{shown}");
        assert!(!f.task(TWO).has("start"));
        assert!(!f.task(THREE).has("start"));
    }

    #[test]
    fn bulk_all_starts_every_task() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &[THREE, "2", "start"]);
        begin_console_override("all\n");
        assert_eq!(f.run(&CmdStart::new()).unwrap(), 0);
        end_console_override();
        assert!(f.task(TWO).has("start"));
        assert!(f.task(THREE).has("start"));
    }

    #[test]
    fn nag_when_more_urgent_task_remains() {
        let mut f = fixture("nag=Be good.\n", &["2", "start"]);
        assert_eq!(f.run(&CmdStart::new()).unwrap(), 0);
        assert!(
            f.ctx.footnotes.iter().any(|m| m == "Be good."),
            "{:?}",
            f.ctx.footnotes
        );
    }

    #[test]
    fn project_change_footnoted() {
        let mut f = fixture("verbose=project\n", &["2", "start"]);
        assert_eq!(f.run(&CmdStart::new()).unwrap(), 0);
        assert!(
            f.ctx.footnotes.iter().any(|m| m
                == "Project 'work' is 0% complete (1 task remaining)."),
            "{:?}",
            f.ctx.footnotes
        );
    }
}
