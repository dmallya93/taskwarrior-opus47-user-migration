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
