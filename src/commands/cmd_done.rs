//! Rust translation of `src/commands/CmdDone.cpp`.
//!
//! `task <filter> done <mods>`: complete every filtered pending or waiting
//! task (annotation modifications, `end` stamp, stop with the optional
//! `rc.journal.time` annotation), ask permission with the task diff, store it
//! with `TDB2::modify` (which runs the on-modify hooks), report unblocked
//! tasks and dependency chain repairs, then nag about more urgent tasks.

use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::dependency::dependency_chain_on_complete;
use crate::error::Result;
use crate::feedback::{
    feedback_affected_count, feedback_affected_task, feedback_unblocked, on_project_change,
};
use crate::filter::Filter;
use crate::nag::nag;
use crate::recur::update_recurrence_mask;
use crate::shared::format::format;
use crate::shared::shared::with_console;
use crate::task::{ModType, Status};
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDone` (`done`).
pub struct CmdDone {
    meta: CommandMeta,
}

impl CmdDone {
    /// C++ `CmdDone::CmdDone`.
    pub fn new() -> Self {
        CmdDone {
            meta: CommandMeta {
                keyword: "done".into(),
                usage: "task <filter> done <mods>".into(),
                description: "Marks the specified task as completed".into(),
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

impl Default for CmdDone {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDone {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDone::execute`.
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
            let before = task.clone();

            let status = task.get_status()?;
            if status == Status::Pending || status == Status::Waiting {
                // Complete the specified task.
                let question = format(
                    "Complete task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );

                task.modify(context, ModType::Annotate, false)?;
                task.set_status(Status::Completed);
                if !task.has("end") {
                    task.set_as_now("end");
                }

                // Stop the task, if started.
                if task.has("start") {
                    task.remove("start");
                    if context.config.get_boolean("journal.time") {
                        let annotation = context.config.get("journal.time.stop.annotation");
                        task.add_annotation(&annotation);
                    }
                }

                let diff = before.diff(&task, &context.columns, &context.config);
                if self.permission(context, &(diff + &question), quantity) {
                    update_recurrence_mask(context, &task)?;
                    tdb2::modify(context, &mut task)?;
                    count += 1;
                    feedback_affected_task(context, "Completed task {1} '{2}'.", &task);
                    feedback_unblocked(context, &task)?;
                    dependency_chain_on_complete(context, &task)?;
                    if context.verbose("project") {
                        let message = on_project_change(context, &task, true)?;
                        project_changes.insert(task.get("project"), message);
                    }

                    // Save unmodified task for potential nagging later
                    modified.push(before);
                } else {
                    with_console(|_, stdout| {
                        let _ = stdout.write_all(b"Task not completed.\n");
                        let _ = stdout.flush();
                    });
                    rc = 1;
                    if self.meta.permission_quit.get() {
                        break;
                    }
                }
            } else {
                let line = format(
                    "Task {1} '{2}' is neither pending nor waiting.",
                    &[&task.identifier(true), &task.get("description")],
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
                "Completed {1} task."
            } else {
                "Completed {1} tasks."
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

    /// ID 1 started with a project, ID 2 plain, a completed task, ID 3
    /// waiting.
    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z","project":"home","start":"20260101T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"two","status":"pending","entry":"20260101T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000003","description":"three","status":"completed","entry":"20260101T000000Z","end":"20260102T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000004","description":"four","status":"pending","entry":"20260101T000000Z","wait":"20990101T000000Z"}"#,
    ];

    fn fixture(rc_lines: &str, args: &[&str]) -> CommandFixture {
        CommandFixture::new(SEED, &format!("confirmation=off\n{rc_lines}"), args)
    }

    #[test]
    fn completes_and_stamps_end() {
        let mut f = fixture("", &["2", "done"]);
        assert_eq!(f.run(&CmdDone::new()).unwrap(), 0);
        let t = f.task(TWO);
        assert_eq!(t.get("status"), "completed");
        assert!(t.has("end"));
        assert_eq!(f.task(ONE).get("status"), "pending");
    }

    #[test]
    fn existing_end_is_kept_and_modifications_apply() {
        let mut f = fixture("", &["2", "done", "end:20260105T000000Z", "+fin"]);
        assert_eq!(f.run(&CmdDone::new()).unwrap(), 0);
        let t = f.task(TWO);
        assert_eq!(t.get("end"), "1767571200");
        assert!(t.get_tags().contains(&"fin".to_string()));
    }

    #[test]
    fn started_task_is_stopped_with_journal_annotation() {
        let mut f = fixture(
            "journal.time=on\njournal.time.stop.annotation=Halted\n",
            &["1", "done"],
        );
        assert_eq!(f.run(&CmdDone::new()).unwrap(), 0);
        let t = f.task(ONE);
        assert!(!t.has("start"));
        let annotations: Vec<String> = t.get_annotations().values().cloned().collect();
        assert_eq!(annotations, vec!["Halted".to_string()]);
    }

    #[test]
    fn started_task_is_stopped_without_journal() {
        let mut f = fixture("", &["1", "done"]);
        assert_eq!(f.run(&CmdDone::new()).unwrap(), 0);
        let t = f.task(ONE);
        assert!(!t.has("start"));
        assert!(t.get_annotations().is_empty());
    }

    #[test]
    fn waiting_task_completes() {
        let mut f = fixture("", &["3", "done"]);
        assert_eq!(f.run(&CmdDone::new()).unwrap(), 0);
        assert_eq!(f.task(FOUR).get("status"), "completed");
    }

    #[test]
    fn completed_task_is_neither_pending_nor_waiting() {
        let mut f = fixture("", &[THREE, "done"]);
        begin_console_override("");
        let rc = f.run(&CmdDone::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert_eq!(
            shown,
            "Task aaaaaaaa 'three' is neither pending nor waiting.\n"
        );
        assert_eq!(f.task(THREE).get("end"), "1767312000");
    }

    #[test]
    fn no_tasks_specified() {
        let mut f = fixture("", &["99", "done"]);
        assert_eq!(f.run(&CmdDone::new()).unwrap(), 1);
        assert!(
            f.ctx.footnotes.iter().any(|m| m == "No tasks specified."),
            "{:?}",
            f.ctx.footnotes
        );
    }

    #[test]
    fn declined_permission_returns_1() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1-2", "done"]);
        begin_console_override("no\nquit\n");
        let rc = f.run(&CmdDone::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert!(
            shown.contains("Complete task 1 'one'? (yes/no/all/quit) Task not completed.\n"),
            "{shown}"
        );
        assert!(
            shown.ends_with("Complete task 2 'two'? (yes/no/all/quit) Task not completed.\n"),
            "{shown}"
        );
        assert_eq!(f.task(ONE).get("status"), "pending");
        assert_eq!(f.task(TWO).get("status"), "pending");
    }

    #[test]
    fn bulk_quit_stops_the_loop() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1-2", "done"]);
        begin_console_override("quit\n");
        let rc = f.run(&CmdDone::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert!(!shown.contains("'two'"), "{shown}");
    }

    #[test]
    fn bulk_all_completes_every_task() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1-2", "done"]);
        begin_console_override("all\n");
        assert_eq!(f.run(&CmdDone::new()).unwrap(), 0);
        end_console_override();
        assert_eq!(f.task(ONE).get("status"), "completed");
        assert_eq!(f.task(TWO).get("status"), "completed");
    }

    #[test]
    fn nag_when_more_urgent_task_remains() {
        let mut f = fixture("nag=Be good.\n", &["2", "done"]);
        assert_eq!(f.run(&CmdDone::new()).unwrap(), 0);
        assert!(
            f.ctx.footnotes.iter().any(|m| m == "Be good."),
            "{:?}",
            f.ctx.footnotes
        );
    }

    #[test]
    fn project_change_footnoted() {
        let mut f = fixture("verbose=project\n", &["1", "done"]);
        assert_eq!(f.run(&CmdDone::new()).unwrap(), 0);
        assert!(
            f.ctx.footnotes.iter().any(|m| m
                == "The project 'home' has changed.  Project 'home' is 100% complete (0 of 1 tasks remaining)."),
            "{:?}",
            f.ctx.footnotes
        );
    }
}
