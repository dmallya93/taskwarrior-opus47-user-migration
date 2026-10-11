//! Rust translation of `src/commands/CmdStop.cpp`.
//!
//! `task <filter> stop <mods>`: remove `start` from every filtered started
//! task, apply annotation modifications and the optional `rc.journal.time`
//! annotation, ask permission with the task diff and store it with
//! `TDB2::modify`.

use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::dependency::dependency_chain_on_start;
use crate::error::Result;
use crate::feedback::{feedback_affected_count, feedback_affected_task, on_project_change};
use crate::filter::Filter;
use crate::recur::update_recurrence_mask;
use crate::shared::format::format;
use crate::shared::shared::with_console;
use crate::task::ModType;
use crate::tdb2;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdStop` (`stop`).
pub struct CmdStop {
    meta: CommandMeta,
}

impl CmdStop {
    /// C++ `CmdStop::CmdStop`.
    pub fn new() -> Self {
        CmdStop {
            meta: CommandMeta {
                keyword: "stop".into(),
                usage: "task <filter> stop <mods>".into(),
                description: "Removes the 'start' time from a task".into(),
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

impl Default for CmdStop {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdStop {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdStop::execute`.
    ///
    /// Like the C++, a stopped task also runs the dependency chain check
    /// (`dependencyChainOnStart`).
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
            if task.has("start") {
                let before = task.clone();

                // Stop the specified task.
                let question = format(
                    "Stop task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );

                task.modify(context, ModType::Annotate, false)?;
                task.remove("start");

                if context.config.get_boolean("journal.time") {
                    let annotation = context.config.get("journal.time.stop.annotation");
                    task.add_annotation(&annotation);
                }

                let diff = before.diff(&task, &context.columns, &context.config);
                if self.permission(context, &(diff + &question), quantity) {
                    update_recurrence_mask(context, &task)?;
                    tdb2::modify(context, &mut task)?;
                    count += 1;
                    feedback_affected_task(context, "Stopping task {1} '{2}'.", &task);
                    dependency_chain_on_start(context, &task)?;
                    if context.verbose("project") {
                        let message = on_project_change(context, &task, false)?;
                        project_changes.insert(task.get("project"), message);
                    }
                } else {
                    with_console(|_, stdout| {
                        let _ = stdout.write_all(b"Task not stopped.\n");
                        let _ = stdout.flush();
                    });
                    rc = 1;
                    if self.meta.permission_quit.get() {
                        break;
                    }
                }
            } else {
                let line = format(
                    "Task {1} '{2}' not started.",
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
            if count == 1 {
                "Stopped {1} task."
            } else {
                "Stopped {1} tasks."
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

    /// ID 1 and 2 started (1 with a project), ID 3 not started.
    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z","project":"home","start":"20260101T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"two","status":"pending","entry":"20260101T000000Z","start":"20260101T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000003","description":"three","status":"pending","entry":"20260101T000000Z"}"#,
    ];

    fn fixture(rc_lines: &str, args: &[&str]) -> CommandFixture {
        CommandFixture::new(SEED, &format!("confirmation=off\n{rc_lines}"), args)
    }

    #[test]
    fn stops_and_removes_start() {
        let mut f = fixture("", &["2", "stop", "+halt"]);
        assert_eq!(f.run(&CmdStop::new()).unwrap(), 0);
        let t = f.task(TWO);
        assert!(!t.has("start"));
        assert!(t.get_tags().contains(&"halt".to_string()));
        assert!(t.get_annotations().is_empty());
        assert!(f.task(ONE).has("start"));
    }

    #[test]
    fn not_started_message() {
        let mut f = fixture("", &["3", "stop"]);
        begin_console_override("");
        let rc = f.run(&CmdStop::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert_eq!(shown, "Task 3 'three' not started.\n");
        assert!(!f.task(THREE).has("start"));
    }

    #[test]
    fn journal_time_stop_annotation() {
        let mut f = fixture(
            "journal.time=on\njournal.time.stop.annotation=Halted\n",
            &["2", "stop"],
        );
        assert_eq!(f.run(&CmdStop::new()).unwrap(), 0);
        let annotations: Vec<String> = f.task(TWO).get_annotations().values().cloned().collect();
        assert_eq!(annotations, vec!["Halted".to_string()]);
    }

    #[test]
    fn no_tasks_specified() {
        let mut f = fixture("", &["99", "stop"]);
        assert_eq!(f.run(&CmdStop::new()).unwrap(), 1);
        assert!(
            f.ctx.footnotes.iter().any(|m| m == "No tasks specified."),
            "{:?}",
            f.ctx.footnotes
        );
    }

    #[test]
    fn declined_permission_and_quit() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1-2", "stop"]);
        begin_console_override("no\nquit\n");
        let rc = f.run(&CmdStop::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert!(
            shown.contains("Stop task 1 'one'? (yes/no/all/quit) Task not stopped.\n"),
            "{shown}"
        );
        assert!(
            shown.ends_with("Stop task 2 'two'? (yes/no/all/quit) Task not stopped.\n"),
            "{shown}"
        );
        assert!(f.task(ONE).has("start"));
        assert!(f.task(TWO).has("start"));
    }

    #[test]
    fn bulk_quit_stops_the_loop() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1-2", "stop"]);
        begin_console_override("quit\n");
        assert_eq!(f.run(&CmdStop::new()).unwrap(), 1);
        let shown = end_console_override();
        assert!(!shown.contains("'two'"), "{shown}");
    }

    #[test]
    fn project_change_footnoted() {
        let mut f = fixture("verbose=project\n", &["1", "stop"]);
        assert_eq!(f.run(&CmdStop::new()).unwrap(), 0);
        assert!(
            f.ctx.footnotes.iter().any(|m| m
                == "Project 'home' is 0% complete (1 task remaining)."),
            "{:?}",
            f.ctx.footnotes
        );
    }
}
