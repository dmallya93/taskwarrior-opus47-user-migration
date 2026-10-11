//! Rust translation of `src/commands/CmdModify.cpp`.
//!
//! `task <filter> modify <mods>`: apply the command-line modifications
//! (`Task::modify (modReplace)`) to every filtered task, check the result
//! for logical consistency, ask permission with the task diff, store it with
//! `TDB2::modify` (which runs the on-modify hooks), and propagate the
//! modifications to recurrence siblings / children according to
//! `rc.recurrence.confirmation`.
//!
//! [`CmdModify::check_consistency`] and [`CmdModify::modify_and_update`] are
//! public because `CmdEdit` reuses them, as the C++ does.

use std::collections::BTreeMap;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::feedback::{
    feedback_affected_count, feedback_affected_task, feedback_unblocked, on_project_change_pair,
};
use crate::filter::Filter;
use crate::recur::update_recurrence_mask;
use crate::shared::format::format;
use crate::shared::shared::{confirm, with_console};
use crate::task::{ModType, Status, Task};
use crate::tdb2;

const STRING_CMD_MODIFY_TASK_R: &str = "Modifying recurring task {1} '{2}'.";
const STRING_CMD_MODIFY_RECUR: &str =
    "This is a recurring task.  Do you want to modify all pending recurrences of this same task?";

/// C++ `std::map<std::string, std::string>` of accumulated project change
/// notifications, keyed by project (ordered, as `std::map`).
pub type ProjectChanges = BTreeMap<String, String>;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdModify` (`modify`).
pub struct CmdModify {
    meta: CommandMeta,
}

impl CmdModify {
    /// C++ `CmdModify::CmdModify`.
    pub fn new() -> Self {
        CmdModify {
            meta: CommandMeta {
                keyword: "modify".into(),
                usage: "task <filter> modify <mods>".into(),
                description: "Modifies the existing task with provided arguments.".into(),
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

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void CmdModify::checkConsistency (Task& before, Task& after)`:
    /// logical consistency checks between the original and modified task.
    ///
    /// # Errors
    /// The C++ thrown strings, verbatim; an invalid stored status.
    pub fn check_consistency(before: &Task, after: &Task) -> Result<()> {
        // Perform some logical consistency checks.
        if after.has("recur") && !after.has("due") && !before.has("due") {
            return Err(TwError::Message("You cannot specify a recurring task without a due date.".into()));
        }

        if before.has("recur") && before.has("due") && (!after.has("due") || after.get_ref("due").is_empty()) {
            return Err(TwError::Message("You cannot remove the due date from a recurring task.".into()));
        }

        if before.has("recur") && (!after.has("recur") || after.get_ref("recur").is_empty()) {
            return Err(TwError::Message("You cannot remove the recurrence from a recurring task.".into()));
        }

        if before.get_status()? == Status::Pending
            && after.get_status()? == Status::Pending
            && before.get_ref("end").is_empty()
            && !after.get_ref("end").is_empty()
        {
            return Err(TwError::Message(format(
                "Could not modify task {1}. You cannot set an end date on a pending task.",
                &[&before.identifier(true)],
            )));
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `int CmdModify::modifyAndUpdate (Task& before, Task& after,
    /// std::map<std::string, std::string>* projectChanges = NULL)`: store
    /// `after`, then propagate the command-line modifications to its
    /// recurrence siblings (child task) or children (template). Returns the
    /// number of tasks modified.
    ///
    /// # Errors
    /// Storage, validation, hook and DOM errors.
    pub fn modify_and_update(
        ctx: &mut Context,
        before: &Task,
        after: &mut Task,
        mut project_changes: Option<&mut ProjectChanges>,
    ) -> Result<i32> {
        // This task.
        let mut count = 1;

        update_recurrence_mask(ctx, after)?;
        feedback_affected_task(ctx, "Modifying task {1} '{2}'.", after);
        feedback_unblocked(ctx, after)?;
        tdb2::modify(ctx, after)?;
        if ctx.verbose("project") {
            if let Some(changes) = project_changes.as_deref_mut() {
                let message = on_project_change_pair(ctx, before, after)?;
                changes.insert(after.get("project"), message);
            }
        }

        // Task has siblings - modify them.
        if after.has("parent") {
            count += Self::modify_recurrence_siblings(ctx, after, project_changes)?;
        }
        // Task has child tasks - modify them.
        else if after.get_ref("status") == "recurring" {
            count += Self::modify_recurrence_parent(ctx, after, project_changes)?;
        }

        Ok(count)
    }

    /// C++ `(config.get ("recurrence.confirmation") == "prompt" && confirm
    /// (STRING_CMD_MODIFY_RECUR)) || config.getBoolean
    /// ("recurrence.confirmation")`.
    fn recurrence_confirmed(ctx: &mut Context) -> bool {
        (ctx.config.get("recurrence.confirmation") == "prompt" && confirm(STRING_CMD_MODIFY_RECUR))
            || ctx.config.get_boolean("recurrence.confirmation")
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `int CmdModify::modifyRecurrenceSiblings (Task& task,
    /// std::map<std::string, std::string>* projectChanges = NULL)`: apply the
    /// modifications to every pending sibling and to the parent template.
    ///
    /// C++ quirk kept: the parent is loaded with `TDB2::get`, which leaves an
    /// empty task when the parent is missing; that empty task is still
    /// modified and stored.
    ///
    /// # Errors
    /// Storage, validation, hook and DOM errors.
    pub fn modify_recurrence_siblings(
        ctx: &mut Context,
        task: &Task,
        mut project_changes: Option<&mut ProjectChanges>,
    ) -> Result<i32> {
        let mut count = 0;

        if Self::recurrence_confirmed(ctx) {
            let siblings = ctx.tdb2.siblings(task)?;
            for mut sibling in siblings {
                let alternate = sibling.clone();
                sibling.modify(ctx, ModType::Replace, false)?;
                update_recurrence_mask(ctx, &sibling)?;
                count += 1;
                feedback_affected_task(ctx, STRING_CMD_MODIFY_TASK_R, &sibling);
                feedback_unblocked(ctx, &sibling)?;
                tdb2::modify(ctx, &mut sibling)?;
                if ctx.verbose("project") {
                    if let Some(changes) = project_changes.as_deref_mut() {
                        let message = on_project_change_pair(ctx, &alternate, &sibling)?;
                        changes.insert(sibling.get("project"), message);
                    }
                }
            }

            // Modify the parent
            let mut parent = ctx.tdb2.get_by_uuid(task.get_ref("parent"))?.unwrap_or_default();
            parent.modify(ctx, ModType::Replace, false)?;
            tdb2::modify(ctx, &mut parent)?;
        }

        Ok(count)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `int CmdModify::modifyRecurrenceParent (Task& task,
    /// std::map<std::string, std::string>* projectChanges = NULL)`: apply the
    /// modifications to every pending child of a recurring template.
    ///
    /// # Errors
    /// Storage, validation, hook and DOM errors.
    pub fn modify_recurrence_parent(
        ctx: &mut Context,
        task: &Task,
        mut project_changes: Option<&mut ProjectChanges>,
    ) -> Result<i32> {
        let mut count = 0;

        let children = ctx.tdb2.children(task)?;
        if !children.is_empty() && Self::recurrence_confirmed(ctx) {
            for mut child in children {
                let alternate = child.clone();
                child.modify(ctx, ModType::Replace, false)?;
                update_recurrence_mask(ctx, &child)?;
                tdb2::modify(ctx, &mut child)?;
                if ctx.verbose("project") {
                    if let Some(changes) = project_changes.as_deref_mut() {
                        let message = on_project_change_pair(ctx, &alternate, &child)?;
                        changes.insert(child.get("project"), message);
                    }
                }
                count += 1;
                feedback_affected_task(ctx, STRING_CMD_MODIFY_TASK_R, &child);
            }
        }

        Ok(count)
    }
}

impl Default for CmdModify {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdModify {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdModify::execute`.
    fn execute(&self, context: &mut Context, _output: &mut String) -> Result<i32> {
        let mut rc = 0;

        // Apply filter.
        let mut filtered = Vec::new();
        Filter::new().subset(context, &mut filtered)?;
        if filtered.is_empty() {
            context.footnote("No tasks specified.");
            return Ok(1);
        }

        // Accumulated project change notifications.
        let mut project_changes = ProjectChanges::new();

        let mut count = 0;
        if filtered.len() > 1 {
            feedback_affected_count(context, "This command will alter {1} tasks.", filtered.len() as i32);
        }
        let quantity = filtered.len() as u32;
        for mut task in filtered {
            let before = task.clone();
            task.modify(context, ModType::Replace, false)?;

            if before != task {
                // Abort if change introduces inconsistencies.
                Self::check_consistency(&before, &task)?;

                let question = format(
                    "Modify task {1} '{2}'?",
                    &[&task.identifier(true), &task.get("description")],
                );

                let diff = before.diff(&task, &context.columns, &context.config);
                if self.permission(context, &(diff + &question), quantity) {
                    count += Self::modify_and_update(context, &before, &mut task, Some(&mut project_changes))?;
                } else {
                    with_console(|_, stdout| {
                        let _ = stdout.write_all(b"Task not modified.\n");
                        let _ = stdout.flush();
                    });
                    rc = 1;
                    if self.meta.permission_quit.get() {
                        break;
                    }
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
            if count == 1 { "Modified {1} task." } else { "Modified {1} tasks." },
            count,
        );
        Ok(rc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli2;
    use crate::context::set_stdout_tty_override;
    use crate::shared::shared::{begin_console_override, end_console_override};
    use crate::test_util::{env_lock, EnvGuard};
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    const PARENT: &str = "bbbbbbbb-0000-4000-8000-000000000001";

    /// ID 1 and 2 plain pending tasks; a recurring template (UUID only) with
    /// two pending children.
    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z"}"#,
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"two","status":"pending","entry":"20260101T000000Z","project":"home"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000001","description":"rec","status":"recurring","entry":"20260101T000000Z","due":"20260101T000000Z","recur":"daily","mask":"--"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000002","description":"rec","status":"pending","entry":"20260101T000000Z","due":"20260101T000000Z","recur":"daily","parent":"bbbbbbbb-0000-4000-8000-000000000001","imask":"0"}"#,
        r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000003","description":"rec","status":"pending","entry":"20260101T000000Z","due":"20260102T000000Z","recur":"daily","parent":"bbbbbbbb-0000-4000-8000-000000000001","imask":"1"}"#,
    ];

    /// Restores the CLI2 abbreviation length `Context::initialize` sets.
    struct MinimumMatchLength(i32);

    impl Drop for MinimumMatchLength {
        fn drop(&mut self) {
            cli2::set_minimum_match_length(self.0);
            set_stdout_tty_override(None);
        }
    }

    /// Fields drop in declaration order: Context and data directory first,
    /// then statics and environment, the lock last.
    struct Fixture {
        ctx: Context,
        _dir: TempDir,
        _mml: MinimumMatchLength,
        _env: EnvGuard,
        _lock: MutexGuard<'static, ()>,
    }

    fn initialized(list: &[&str]) -> Context {
        let args: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        let mut ctx = Context::new();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let rc = ctx.initialize_with(&args, &mut out, &mut err).unwrap();
        assert_eq!(rc, 0, "{}", String::from_utf8_lossy(&err));
        ctx
    }

    impl Fixture {
        /// Seeds [`SEED`] (taskrc `rc_lines`), then initializes a Context
        /// for `task <list...>`.
        fn new(rc_lines: &str, list: &[&str]) -> Self {
            let lock = env_lock();
            let env = EnvGuard::save(&["HOME", "TASKRC", "TASKDATA", "XDG_CONFIG_HOME"]);
            let mml = MinimumMatchLength(cli2::minimum_match_length());
            let dir = TempDir::new().unwrap();
            let base = dir.path().to_str().unwrap().to_string();
            let home = format!("{base}/home");
            std::fs::create_dir(&home).unwrap();
            let rc = format!("{base}/taskrc");
            std::fs::write(&rc, format!("recurrence=off\nverbose=nothing\n{rc_lines}")).unwrap();
            std::env::set_var("HOME", &home);
            std::env::set_var("TASKRC", &rc);
            std::env::set_var("TASKDATA", format!("{base}/data"));
            std::env::remove_var("XDG_CONFIG_HOME");
            set_stdout_tty_override(Some(false));

            {
                let mut seed = initialized(&["task", "add", "seed"]);
                for json in SEED {
                    let mut task = Task::from_input(&mut seed, json).unwrap();
                    crate::tdb2::add(&mut seed, &mut task).unwrap();
                }
            }

            let mut full = vec!["task"];
            full.extend_from_slice(list);
            let ctx = initialized(&full);
            Fixture { ctx, _dir: dir, _mml: mml, _env: env, _lock: lock }
        }

        fn run(&mut self) -> Result<i32> {
            let mut output = String::new();
            let result = CmdModify::new().execute(&mut self.ctx, &mut output);
            assert_eq!(output, "");
            result
        }

        fn task(&mut self, uuid: &str) -> Task {
            self.ctx.tdb2.get_by_uuid(uuid).unwrap().unwrap()
        }
    }


    #[test]
    fn check_consistency_messages() {
        let mut plain = Task::new();
        plain.set("uuid", "aaaaaaaa-0000-4000-8000-000000000001");
        plain.set("status", "pending");
        plain.set("description", "x");
        let mut recur = plain.clone();
        recur.set("recur", "daily");
        assert_eq!(
            CmdModify::check_consistency(&plain, &recur).unwrap_err().to_string(),
            "You cannot specify a recurring task without a due date."
        );

        let mut before = recur.clone();
        before.set("due", "1700000000");
        let mut after = before.clone();
        after.set("due", "");
        assert_eq!(
            CmdModify::check_consistency(&before, &after).unwrap_err().to_string(),
            "You cannot remove the due date from a recurring task."
        );

        let mut after = before.clone();
        after.remove("recur");
        assert_eq!(
            CmdModify::check_consistency(&before, &after).unwrap_err().to_string(),
            "You cannot remove the recurrence from a recurring task."
        );

        let mut ended = plain.clone();
        ended.set("end", "1700000000");
        assert_eq!(
            CmdModify::check_consistency(&plain, &ended).unwrap_err().to_string(),
            "Could not modify task aaaaaaaa. You cannot set an end date on a pending task."
        );

        assert!(CmdModify::check_consistency(&before, &before).is_ok());
    }

    #[test]
    fn modifies_selected_task() {
        let mut f = Fixture::new("", &["1", "modify", "changed", "+tag"]);
        assert_eq!(f.run().unwrap(), 0);
        let t = f.task("aaaaaaaa-0000-4000-8000-000000000001");
        assert_eq!(t.get("description"), "changed");
        assert!(t.get_tags().contains(&"tag".to_string()));
    }

    #[test]
    fn no_tasks_specified() {
        let mut f = Fixture::new("", &["99", "modify", "x"]);
        assert_eq!(f.run().unwrap(), 1);
        assert!(f.ctx.footnotes.iter().any(|m| m == "No tasks specified."), "{:?}", f.ctx.footnotes);
    }

    #[test]
    fn inconsistent_change_aborts() {
        let mut f = Fixture::new("", &["1", "modify", "recur:daily"]);
        assert_eq!(f.run().unwrap_err().to_string(), "You cannot specify a recurring task without a due date.");
        assert!(!f.task("aaaaaaaa-0000-4000-8000-000000000001").has("recur"));
    }

    #[test]
    fn bulk_quit_stops_and_returns_1() {
        let mut f = Fixture::new("bulk=2\nconfirmation=on\n", &["1-2", "modify", "project:work"]);
        begin_console_override("quit\n");
        let rc = f.run().unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 1);
        assert!(shown.contains("Modify task 1 'one'?"), "{shown}");
        assert!(shown.ends_with("Task not modified.\n"), "{shown}");
        assert!(!f.task("aaaaaaaa-0000-4000-8000-000000000001").has("project"));
        assert_eq!(f.task("aaaaaaaa-0000-4000-8000-000000000002").get("project"), "home");
    }

    #[test]
    fn bulk_all_modifies_every_task() {
        let mut f = Fixture::new("bulk=2\nconfirmation=on\n", &["1-2", "modify", "project:work"]);
        begin_console_override("all\n");
        let rc = f.run().unwrap();
        end_console_override();
        assert_eq!(rc, 0);
        assert_eq!(f.task("aaaaaaaa-0000-4000-8000-000000000001").get("project"), "work");
        assert_eq!(f.task("aaaaaaaa-0000-4000-8000-000000000002").get("project"), "work");
    }

    #[test]
    fn project_changes_footnoted() {
        let mut f = Fixture::new("verbose=project\n", &["2", "modify", "project:work"]);
        assert_eq!(f.run().unwrap(), 0);
        assert!(f.ctx.footnotes.iter().any(|m| m.contains("Project 'work'")), "{:?}", f.ctx.footnotes);
    }

    #[test]
    fn recurrence_confirmation_yes_modifies_siblings_and_parent() {
        let mut f = Fixture::new("recurrence.confirmation=yes\n", &["bbbbbbbb-0000-4000-8000-000000000002", "modify", "project:x"]);
        assert_eq!(f.run().unwrap(), 0);
        for uuid in [PARENT, "bbbbbbbb-0000-4000-8000-000000000002", "bbbbbbbb-0000-4000-8000-000000000003"] {
            assert_eq!(f.task(uuid).get("project"), "x", "{uuid}");
        }
    }

    #[test]
    fn recurrence_confirmation_no_modifies_only_the_task() {
        let mut f = Fixture::new("recurrence.confirmation=no\n", &["bbbbbbbb-0000-4000-8000-000000000002", "modify", "project:x"]);
        assert_eq!(f.run().unwrap(), 0);
        assert_eq!(f.task("bbbbbbbb-0000-4000-8000-000000000002").get("project"), "x");
        assert!(!f.task("bbbbbbbb-0000-4000-8000-000000000003").has("project"));
        assert!(!f.task(PARENT).has("project"));
    }

    #[test]
    fn recurrence_confirmation_prompt_asks() {
        let mut f = Fixture::new("recurrence.confirmation=prompt\n", &["bbbbbbbb-0000-4000-8000-000000000002", "modify", "project:x"]);
        begin_console_override("yes\n");
        assert_eq!(f.run().unwrap(), 0);
        let shown = end_console_override();
        assert!(shown.contains(STRING_CMD_MODIFY_RECUR), "{shown}");
        assert_eq!(f.task("bbbbbbbb-0000-4000-8000-000000000003").get("project"), "x");
    }

    #[test]
    fn template_modifies_children() {
        let mut f = Fixture::new("recurrence.confirmation=yes\n", &[PARENT, "modify", "project:y"]);
        assert_eq!(f.run().unwrap(), 0);
        assert_eq!(f.task(PARENT).get("project"), "y");
        assert_eq!(f.task("bbbbbbbb-0000-4000-8000-000000000002").get("project"), "y");
        assert_eq!(f.task("bbbbbbbb-0000-4000-8000-000000000003").get("project"), "y");
    }
}
