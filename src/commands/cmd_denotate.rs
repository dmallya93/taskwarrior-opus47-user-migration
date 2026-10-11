//! Rust translation of `src/commands/CmdDenotate.cpp`.
//!
//! `task <filter> denotate <pattern>`: the MISCELLANEOUS words, joined by
//! spaces, form a pattern. For every filtered task the first annotation whose
//! text equals the pattern is removed; failing that, the first annotation
//! containing it (honouring `rc.search.case.sensitive`). Each change is
//! confirmed with the task diff and stored with `TDB2::modify`.

use crate::commands::cmd_modify::ProjectChanges;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::feedback::{feedback_affected, feedback_affected_count, on_project_change};
use crate::filter::Filter;
use crate::shared::format::format;
use crate::shared::shared::{find, with_console};
use crate::tdb2;

const STRING_CMD_DENO_NO: &str = "Task not denotated.";
const STRING_CMD_DENO_1: &str = "Denotated {1} task.";
const STRING_CMD_DENO_N: &str = "Denotated {1} tasks.";

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdDenotate` (`denotate`).
pub struct CmdDenotate {
    meta: CommandMeta,
}

impl CmdDenotate {
    /// C++ `CmdDenotate::CmdDenotate`.
    pub fn new() -> Self {
        CmdDenotate {
            meta: CommandMeta {
                keyword: "denotate".into(),
                usage: "task <filter> denotate <pattern>".into(),
                description: "Deletes an annotation".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdDenotate {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdDenotate {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdDenotate::execute`.
    fn execute(&self, context: &mut Context, _output: &mut String) -> Result<i32> {
        let mut rc = 0;
        let mut count = 0;
        let sensitive = context.config.get_boolean("search.case.sensitive");

        // Apply filter.
        let mut filtered = Vec::new();
        Filter::new().subset(context, &mut filtered)?;
        if filtered.is_empty() {
            context.footnote("No tasks specified.");
            return Ok(1);
        }

        // Extract all the ORIGINAL MODIFICATION args as simple text patterns.
        let mut pattern = String::new();
        for a in &context.cli2.args {
            if a.has_tag("MISCELLANEOUS") {
                if !pattern.is_empty() {
                    pattern.push(' ');
                }

                pattern += &a.attribute("raw");
            }
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

            let mut annotations = task.get_annotations();

            if annotations.is_empty() {
                return Err(TwError::Message(
                    "The specified task has no annotations that can be deleted.".into(),
                ));
            }

            let mut anno = String::new();
            let mut matched = None;
            for (key, text) in &annotations {
                if *text == pattern {
                    matched = Some(key.clone());
                    anno = text.clone();
                    break;
                }
            }

            if matched.is_none() {
                for (key, text) in &annotations {
                    if find(text, &pattern, sensitive).is_some() {
                        matched = Some(key.clone());
                        anno = text.clone();
                        break;
                    }
                }
            }

            if let Some(key) = matched {
                annotations.remove(&key);
                task.set_annotations(&annotations);
            }

            if before.get_annotations() != task.get_annotations() {
                let question = format(
                    "Denotate task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );

                let diff = before.diff(&task, &context.columns, &context.config);
                if self.permission(context, &(diff + &question), quantity) {
                    count += 1;
                    tdb2::modify(context, &mut task)?;
                    feedback_affected(
                        context,
                        &format("Found annotation '{1}' and deleted it.", &[&anno]),
                    );
                    if context.verbose("project") {
                        let message = on_project_change(context, &task, false)?;
                        project_changes.insert(task.get("project"), message);
                    }
                } else {
                    let line = format!("{STRING_CMD_DENO_NO}\n");
                    with_console(|_, stdout| {
                        let _ = stdout.write_all(line.as_bytes());
                        let _ = stdout.flush();
                    });
                    rc = 1;
                    if self.meta.permission_quit.get() {
                        break;
                    }
                }
            } else {
                let line = format(
                    "Did not find any matching annotation to be deleted for '{1}'.\n",
                    &[&pattern],
                );
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
                STRING_CMD_DENO_1
            } else {
                STRING_CMD_DENO_N
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

    /// ID 1 has the annotations "foo bar" (older) and "foo" (newer); ID 2
    /// has none; ID 3 has "foo".
    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z","project":"home","annotations":[{"entry":"20260101T000000Z","description":"foo bar"},{"entry":"20260102T000000Z","description":"foo"}]}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"two","status":"pending","entry":"20260101T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000003","description":"three","status":"pending","entry":"20260101T000000Z","annotations":[{"entry":"20260101T000000Z","description":"foo"}]}"#,
    ];

    fn fixture(rc_lines: &str, args: &[&str]) -> CommandFixture {
        CommandFixture::new(SEED, &format!("confirmation=off\n{rc_lines}"), args)
    }

    fn annotations(f: &mut CommandFixture, uuid: &str) -> Vec<String> {
        f.task(uuid).get_annotations().values().cloned().collect()
    }

    #[test]
    fn exact_match_wins_over_earlier_partial_match() {
        let mut f = fixture("", &["1", "denotate", "foo"]);
        assert_eq!(f.run(&CmdDenotate::new()).unwrap(), 0);
        assert_eq!(annotations(&mut f, ONE), vec!["foo bar".to_string()]);
    }

    #[test]
    fn partial_match_removes_first_containing_annotation() {
        let mut f = fixture("", &["1", "denotate", "bar"]);
        assert_eq!(f.run(&CmdDenotate::new()).unwrap(), 0);
        assert_eq!(annotations(&mut f, ONE), vec!["foo".to_string()]);
    }

    #[test]
    fn words_are_joined_into_one_pattern() {
        let mut f = fixture("", &["1", "denotate", "foo", "bar"]);
        assert_eq!(f.run(&CmdDenotate::new()).unwrap(), 0);
        assert_eq!(annotations(&mut f, ONE), vec!["foo".to_string()]);
    }

    #[test]
    fn case_insensitive_partial_match() {
        let mut f = fixture("search.case.sensitive=no\n", &["1", "denotate", "BAR"]);
        assert_eq!(f.run(&CmdDenotate::new()).unwrap(), 0);
        assert_eq!(annotations(&mut f, ONE), vec!["foo".to_string()]);
    }

    #[test]
    fn case_sensitive_no_match() {
        let mut f = fixture("search.case.sensitive=yes\n", &["1", "denotate", "BAR"]);
        begin_console_override("");
        let rc = f.run(&CmdDenotate::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert_eq!(
            shown,
            "Did not find any matching annotation to be deleted for 'BAR'.\n"
        );
        assert_eq!(annotations(&mut f, ONE).len(), 2);
    }

    #[test]
    fn task_without_annotations_is_an_error() {
        let mut f = fixture("", &["2", "denotate", "foo"]);
        let err = f.run(&CmdDenotate::new()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "The specified task has no annotations that can be deleted."
        );
        assert!(annotations(&mut f, TWO).is_empty());
    }

    #[test]
    fn no_tasks_specified() {
        let mut f = fixture("", &["99", "denotate", "foo"]);
        assert_eq!(f.run(&CmdDenotate::new()).unwrap(), 1);
        assert!(
            f.ctx.footnotes.iter().any(|m| m == "No tasks specified."),
            "{:?}",
            f.ctx.footnotes
        );
    }

    #[test]
    fn declined_permission_returns_1() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1", "3", "denotate", "foo"]);
        begin_console_override("no\nno\n");
        let rc = f.run(&CmdDenotate::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert!(
            shown.contains("Denotate task 1 'one'? (yes/no/all/quit) Task not denotated.\n"),
            "{shown}"
        );
        assert_eq!(shown.matches("Task not denotated.").count(), 2, "{shown}");
        assert_eq!(annotations(&mut f, ONE).len(), 2);
        assert_eq!(annotations(&mut f, THREE).len(), 1);
    }

    #[test]
    fn bulk_quit_stops_the_loop() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1", "3", "denotate", "foo"]);
        begin_console_override("quit\n");
        assert_eq!(f.run(&CmdDenotate::new()).unwrap(), 1);
        let shown = end_console_override();
        assert_eq!(shown.matches("Task not denotated.").count(), 1, "{shown}");
        assert_eq!(annotations(&mut f, THREE).len(), 1);
    }

    #[test]
    fn bulk_all_denotates_every_task() {
        let mut f = fixture("confirmation=on\nbulk=2\n", &["1", "3", "denotate", "foo"]);
        begin_console_override("all\n");
        assert_eq!(f.run(&CmdDenotate::new()).unwrap(), 0);
        end_console_override();
        assert_eq!(annotations(&mut f, ONE), vec!["foo bar".to_string()]);
        assert!(annotations(&mut f, THREE).is_empty());
    }

    #[test]
    fn project_change_footnoted() {
        let mut f = fixture("verbose=project\n", &["1", "denotate", "foo"]);
        assert_eq!(f.run(&CmdDenotate::new()).unwrap(), 0);
        assert!(
            f.ctx
                .footnotes
                .iter()
                .any(|m| m == "Project 'home' is 0% complete (1 task remaining)."),
            "{:?}",
            f.ctx.footnotes
        );
    }
}
