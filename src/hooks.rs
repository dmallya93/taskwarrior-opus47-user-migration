//! Rust translation of `src/Hooks.h` / `src/Hooks.cpp`.
//!
//! [`Hooks`] holds the C++ members (`_enabled`, `_debug`, `_scripts`) and
//! lives in `Context::hooks`. The C++ methods reach `Context::getContext ()`
//! for the configuration, the CLI, TDB2, the message queues and the timing
//! counters. Here every entry point that needs them is an associated function
//! taking `ctx: &mut Context` explicitly ([`Hooks::initialize`],
//! [`Hooks::on_launch`], [`Hooks::on_exit`], [`Hooks::on_add`],
//! [`Hooks::on_modify`]). They run on a clone of `ctx.hooks` (a flag, an
//! integer and the script list), so `ctx` is never borrowed twice; the C++
//! methods are `const`, so the clone cannot diverge from the original.
//!
//! The C++ `throw 0` ("This is how hooks silently terminate processing.")
//! becomes [`TwError::Status`]`(0)`, which `Context::run` turns into exit
//! status 4 after printing the queued errors, as the C++ `catch (int)` does.
//! Strings thrown by `execute`, the file system or `Task` parsing stay
//! [`TwError::Message`] (status 2).

use crate::context::Context;
use crate::dom;
use crate::error::{Result, TwError};
use crate::lexer::Lexer;
use crate::shared::format::format;
use crate::shared::fs::{Directory, File, Path};
use crate::shared::json::{self, JType};
use crate::shared::shared::{execute, split};
use crate::shared::timer::Timer;
use crate::task::Task;
use crate::util::nontrivial;
use crate::version::VERSION;

const STRING_HOOK_ERROR_OBJECT: &str =
    "Hook Error: JSON Object '{...}' expected from hook script: {1}";
const STRING_HOOK_ERROR_NODESC: &str =
    "Hook Error: JSON Object missing 'description' attribute from hook script: {1}";
const STRING_HOOK_ERROR_NOUUID: &str =
    "Hook Error: JSON Object missing 'uuid' attribute from hook script: {1}";
const STRING_HOOK_ERROR_SYNTAX: &str = "Hook Error: JSON syntax error in: {1}";
const STRING_HOOK_ERROR_JSON: &str = "Hook Error: JSON ";
const STRING_HOOK_ERROR_NOPARSE: &str = "Hook Error: JSON failed to parse: ";
const STRING_HOOK_ERROR_BAD_NUM: &str =
    "Hook Error: Expected {1} JSON task(s), found {2}, in hook script: {3}";
const STRING_HOOK_ERROR_SAME1: &str =
    "Hook Error: JSON must be for the same task: {1}, in hook script: {2}";
const STRING_HOOK_ERROR_SAME2: &str =
    "Hook Error: JSON must be for the same task: {1} != {2}, in hook script: {3}";
const STRING_HOOK_ERROR_NOFEEDBACK: &str =
    "Hook Error: Expected feedback from failing hook script: {1}";

/// The C++ `throw 0`.
fn terminate() -> TwError {
    TwError::Status(0)
}

/// C++ `class Hooks`.
#[derive(Debug, Clone)]
pub struct Hooks {
    /// C++ `_enabled`.
    enabled: bool,
    /// C++ `_debug` (`rc.debug.hooks`).
    debug: i32,
    /// C++ `_scripts`: every entry of the hook directory, sorted.
    scripts: Vec<String>,
}

impl Default for Hooks {
    /// C++ `Hooks () = default`: `_enabled{true}`, `_debug{0}`, no scripts.
    fn default() -> Self {
        Hooks {
            enabled: true,
            debug: 0,
            scripts: Vec::new(),
        }
    }
}

/// The C++ `Path (script).name ()`.
fn script_name(script: &str) -> String {
    Path::new(script).name()
}

impl Hooks {
    /// C++ `Hooks::Hooks`.
    pub fn new() -> Self {
        Hooks::default()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void Hooks::initialize ()`: scans `<rc.hooks.location>` (or
    /// `<rc.data.location>/hooks`) for scripts and reads `rc.debug.hooks` and
    /// `rc.hooks`.
    ///
    /// # Errors
    /// The libshared `access`/`stat` errors of the file system checks.
    pub fn initialize(ctx: &mut Context) -> Result<()> {
        let debug = ctx.config.get_integer("debug.hooks");
        ctx.hooks.debug = debug;

        // Scan <rc.hooks.location>
        //      <rc.data.location>/hooks
        let d = if ctx.config.has("hooks.location") {
            Directory::new(&ctx.config.get("hooks.location"))
        } else {
            let mut d = Directory::new(&ctx.config.get("data.location"));
            **d += "hooks";
            d
        };

        if d.is_directory()? && d.readable()? {
            let mut scripts = d.list()?;
            scripts.sort();

            if debug >= 1 {
                for i in &scripts {
                    let p = Path::new(i);
                    if !p.is_directory()? {
                        let name = p.name();
                        if name.starts_with("on-add")
                            || name.starts_with("on-modify")
                            || name.starts_with("on-launch")
                            || name.starts_with("on-exit")
                        {
                            ctx.debug(&format!("Found hook script {i}"));
                        } else {
                            ctx.debug(&format!("Found misnamed hook script {i}"));
                        }
                    }
                }
            }

            ctx.hooks.scripts = scripts;
        } else if debug >= 1 {
            ctx.debug(&format!("Hook directory not readable: {}", d.data));
        }

        ctx.hooks.enabled = ctx.config.get_boolean("hooks");
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `bool Hooks::enable (bool)`: sets `_enabled`, returning the old
    /// value.
    pub fn enable(&mut self, value: bool) -> bool {
        let old_value = self.enabled;
        self.enabled = value;
        old_value
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void Hooks::onLaunch () const`.
    ///
    /// The on-launch event is triggered once, after initialization, before any
    /// processing occurs, i.e first
    ///
    /// Input:
    /// - none
    ///
    /// Output:
    /// - JSON not allowed.
    /// - all emitted non-JSON lines are considered feedback or error messages
    ///   depending on the status code.
    ///
    /// # Errors
    /// [`TwError::Status`]`(0)` when a script fails or misbehaves (the
    /// messages are queued as errors); the errors of `execute`.
    pub fn on_launch(ctx: &mut Context) -> Result<()> {
        let hooks = ctx.hooks.clone();
        hooks.on_launch_impl(ctx)
    }

    fn on_launch_impl(&self, ctx: &mut Context) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let timer = Timer::new();

        let matching_scripts = self.scripts("on-launch")?;
        for script in &matching_scripts {
            let input: Vec<String> = Vec::new();
            let (status, output) = self.call_hook_script(ctx, script, &input)?;

            let (output_json, output_feedback) = self.separate_output(&output);

            self.assert_n_tasks(ctx, &output_json, 0, script)?;

            Self::forward_feedback(ctx, status, &output_feedback, script)?;
        }

        ctx.time_hooks_us += timer.total_us() as i64;
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void Hooks::onExit () const`.
    ///
    /// The on-exit event is triggered once, after all processing is complete,
    /// i.e. last
    ///
    /// Input:
    /// - read-only line of JSON for each task added/modified
    ///
    /// Output:
    /// - all emitted JSON is ignored
    /// - all emitted non-JSON lines are considered feedback or error messages
    ///   depending on the status code.
    ///
    /// (Despite the comment, the C++ requires zero JSON lines.)
    ///
    /// # Errors
    /// As [`Hooks::on_launch`], plus those of `composeJSON`.
    pub fn on_exit(ctx: &mut Context) -> Result<()> {
        let hooks = ctx.hooks.clone();
        hooks.on_exit_impl(ctx)
    }

    fn on_exit_impl(&self, ctx: &mut Context) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let timer = Timer::new();

        let matching_scripts = self.scripts("on-exit")?;
        if !matching_scripts.is_empty() {
            // Get the set of changed tasks.
            let tasks = ctx.tdb2.get_changes();

            // Convert to a vector of strings.
            let mut input = Vec::with_capacity(tasks.len());
            for mut t in tasks {
                input.push(t.compose_json(ctx, false)?);
            }

            // Call the hook scripts, with the invariant input.
            for script in &matching_scripts {
                let (status, output) = self.call_hook_script(ctx, script, &input)?;

                let (output_json, output_feedback) = self.separate_output(&output);

                self.assert_n_tasks(ctx, &output_json, 0, script)?;

                Self::forward_feedback(ctx, status, &output_feedback, script)?;
            }
        }

        ctx.time_hooks_us += timer.total_us() as i64;
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void Hooks::onAdd (Task&) const`.
    ///
    /// The on-add event is triggered separately for each task added
    ///
    /// Input:
    /// - line of JSON for the task added
    ///
    /// Output:
    /// - emitted JSON for the input task is added, if the exit code is zero,
    ///   otherwise ignored.
    /// - all emitted non-JSON lines are considered feedback or error messages
    ///   depending on the status code.
    ///
    /// # Errors
    /// As [`Hooks::on_launch`], plus those of `composeJSON` and of parsing the
    /// returned task (`Task (std::string)`).
    pub fn on_add(ctx: &mut Context, task: &mut Task) -> Result<()> {
        let hooks = ctx.hooks.clone();
        hooks.on_add_impl(ctx, task)
    }

    fn on_add_impl(&self, ctx: &mut Context, task: &mut Task) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let timer = Timer::new();

        let matching_scripts = self.scripts("on-add")?;
        if !matching_scripts.is_empty() {
            // Convert task to a vector of strings.
            let mut input = vec![task.compose_json(ctx, false)?];

            // Call the hook scripts.
            for script in &matching_scripts {
                let (status, output) = self.call_hook_script(ctx, script, &input)?;

                let (output_json, output_feedback) = self.separate_output(&output);

                if status == 0 {
                    self.assert_n_tasks(ctx, &output_json, 1, script)?;
                    self.assert_valid_json(ctx, &output_json, script)?;
                    self.assert_same_task(ctx, &output_json, task, script)?;

                    // Propagate forward to the next script.
                    input[0] = output_json[0].clone();
                }

                Self::forward_feedback(ctx, status, &output_feedback, script)?;
            }

            // Transfer the modified task back to the original task.
            *task = Task::from_input(ctx, &input[0])?;
        }

        ctx.time_hooks_us += timer.total_us() as i64;
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void Hooks::onModify (Task& before, Task& after) const`.
    ///
    /// The on-modify event is triggered separately for each task added or
    /// modified
    ///
    /// Input:
    /// - line of JSON for the original task
    /// - line of JSON for the modified task, the diff being the modification
    ///
    /// Output:
    /// - emitted JSON for the input task is saved, if the exit code is zero,
    ///   otherwise ignored.
    /// - all emitted non-JSON lines are considered feedback or error messages
    ///   depending on the status code.
    ///
    /// `before` is a C++ non-const reference only because `composeJSON` is
    /// not `const`; it is never replaced.
    ///
    /// # Errors
    /// As [`Hooks::on_add`].
    pub fn on_modify(ctx: &mut Context, before: &mut Task, after: &mut Task) -> Result<()> {
        let hooks = ctx.hooks.clone();
        hooks.on_modify_impl(ctx, before, after)
    }

    fn on_modify_impl(&self, ctx: &mut Context, before: &mut Task, after: &mut Task) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let timer = Timer::new();

        let matching_scripts = self.scripts("on-modify")?;
        if !matching_scripts.is_empty() {
            // Convert vector of tasks to a vector of strings.
            let mut input = vec![
                before.compose_json(ctx, false)?, // [line 0] original, never changes
                after.compose_json(ctx, false)?,  // [line 1] modified
            ];

            // Call the hook scripts.
            for script in &matching_scripts {
                let (status, output) = self.call_hook_script(ctx, script, &input)?;

                let (output_json, output_feedback) = self.separate_output(&output);

                if status == 0 {
                    self.assert_n_tasks(ctx, &output_json, 1, script)?;
                    self.assert_valid_json(ctx, &output_json, script)?;
                    self.assert_same_task(ctx, &output_json, before, script)?;

                    // Propagate accepted changes forward to the next script.
                    input[1] = output_json[0].clone();
                }

                Self::forward_feedback(ctx, status, &output_feedback, script)?;
            }

            *after = Task::from_input(ctx, &input[1])?;
        }

        ctx.time_hooks_us += timer.total_us() as i64;
        Ok(())
    }

    /// The tail shared by every C++ event loop: a successful script's
    /// feedback becomes footnotes; a failing script must have given feedback,
    /// which becomes errors, and processing silently terminates (`throw 0`).
    fn forward_feedback(
        ctx: &mut Context,
        status: i32,
        output_feedback: &[String],
        script: &str,
    ) -> Result<()> {
        if status == 0 {
            for message in output_feedback {
                ctx.footnote(message);
            }
            Ok(())
        } else {
            Self::assert_feedback(ctx, output_feedback, script)?;
            for message in output_feedback {
                ctx.error(message);
            }

            Err(terminate()) // This is how hooks silently terminate processing.
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::vector<std::string> Hooks::list () const`.
    pub fn list(&self) -> Vec<String> {
        self.scripts.clone()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Hooks::scripts (event)`: the executable scripts whose path
    /// contains `/<event>`.
    ///
    /// # Errors
    /// The libshared `access` error of `File::executable`.
    fn scripts(&self, event: &str) -> Result<Vec<String>> {
        let mut matching = Vec::new();
        let needle = format!("/{event}");
        for i in &self.scripts {
            if i.contains(&needle) {
                let script = File::new(i);
                if script.executable()? {
                    matching.push(i.clone());
                }
            }
        }

        Ok(matching)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Hooks::separateOutput`: splits the lines into
    /// `(json, feedback)`.
    fn separate_output(&self, output: &[String]) -> (Vec<String>, Vec<String>) {
        let mut json = Vec::new();
        let mut feedback = Vec::new();
        for i in output {
            if self.is_json(i) {
                json.push(i.clone());
            } else {
                feedback.push(i.clone());
            }
        }
        (json, feedback)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Hooks::isJSON`: longer than two bytes, starting with `{`
    /// and ending with `}`.
    fn is_json(&self, input: &str) -> bool {
        input.len() > 2 && input.starts_with('{') && input.ends_with('}')
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Hooks::assertValidJSON`.
    ///
    /// In the C++, the `throw 0` for a non-object root, a missing
    /// `description` or a missing `uuid` happens inside the `try` block, so
    /// the trailing `catch (...)` also queues `Hook Error: JSON failed to
    /// parse: <line>` before rethrowing; that second error is reproduced. A
    /// `json::parse` failure (`throw std::string`) is caught by the
    /// `catch (const std::string&)` clause instead.
    fn assert_valid_json(&self, ctx: &mut Context, input: &[String], script: &str) -> Result<()> {
        for i in input {
            if i.len() < 3 || !i.starts_with('{') || !i.ends_with('}') {
                ctx.error(&format(STRING_HOOK_ERROR_OBJECT, &[&script_name(script)]));
                return Err(terminate());
            }

            let root = match json::parse(i) {
                Ok(root) => root,
                Err(e) => {
                    ctx.error(&format(STRING_HOOK_ERROR_SYNTAX, &[i]));
                    if self.debug != 0 {
                        ctx.error(&format!("{STRING_HOOK_ERROR_JSON}{e}"));
                    }
                    return Err(terminate());
                }
            };

            let message = if root.jtype() != JType::Object {
                Some(STRING_HOOK_ERROR_OBJECT)
            } else {
                let data = root.as_object();
                if !data.is_some_and(|d| d.contains_key("description")) {
                    Some(STRING_HOOK_ERROR_NODESC)
                } else if !data.is_some_and(|d| d.contains_key("uuid")) {
                    Some(STRING_HOOK_ERROR_NOUUID)
                } else {
                    None
                }
            };

            if let Some(message) = message {
                ctx.error(&format(message, &[&script_name(script)]));
                // catch (...)
                ctx.error(&format!("{STRING_HOOK_ERROR_NOPARSE}{i}"));
                return Err(terminate());
            }
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Hooks::assertNTasks`.
    fn assert_n_tasks(
        &self,
        ctx: &mut Context,
        input: &[String],
        n: u32,
        script: &str,
    ) -> Result<()> {
        if input.len() != n as usize {
            ctx.error(&format(
                STRING_HOOK_ERROR_BAD_NUM,
                &[&n, &(input.len() as i32), &script_name(script)],
            ));
            return Err(terminate());
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Hooks::assertSameTask`: every line must carry the uuid of
    /// `task` as a string.
    ///
    /// # Errors
    /// [`TwError::Status`]`(0)` on a mismatch; a `json::parse` error (only
    /// for input that [`Hooks::assert_valid_json`] did not check first).
    fn assert_same_task(
        &self,
        ctx: &mut Context,
        input: &[String],
        task: &Task,
        script: &str,
    ) -> Result<()> {
        let uuid = task.get("uuid");

        for i in input {
            let root = json::parse(i)?;

            // If there is no UUID at all.
            let u = root.as_object().and_then(|data| data.get("uuid"));
            let Some(up) = u.filter(|u| u.jtype() == JType::String) else {
                ctx.error(&format(
                    STRING_HOOK_ERROR_SAME1,
                    &[&uuid, &script_name(script)],
                ));
                return Err(terminate());
            };

            let mut text = up.dump();
            Lexer::dequote(&mut text);
            let json_uuid = json::decode(&text);
            if json_uuid != uuid {
                ctx.error(&format(
                    STRING_HOOK_ERROR_SAME2,
                    &[&uuid, &json_uuid, &script_name(script)],
                ));
                return Err(terminate());
            }
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Hooks::assertFeedback`: a failing script must emit at
    /// least one non-blank line.
    fn assert_feedback(ctx: &mut Context, input: &[String], script: &str) -> Result<()> {
        let found_something = input.iter().any(|i| nontrivial(i));

        if !found_something {
            ctx.error(&format(
                STRING_HOOK_ERROR_NOFEEDBACK,
                &[&script_name(script)],
            ));
            return Err(terminate());
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Hooks::buildHookScriptArgs`: `api:2`, `args:`,
    /// `command:`, `rc:`, `data:` and `version:`.
    ///
    /// # Errors
    /// Those of the `context.args` DOM lookup.
    fn build_hook_script_args(ctx: &mut Context) -> Result<Vec<String>> {
        let mut args = Vec::new();

        // Hooks API version.
        args.push("api:2".to_string());

        // Command line Taskwarrior was called with.
        let v = dom::get_dom(ctx, "context.args")?
            .map(|v| v.to_string())
            .unwrap_or_default();
        args.push(format!("args:{v}"));

        // Command to be executed.
        args.push(format!("command:{}", ctx.cli2.get_command()));

        // rc file used after applying all overrides.
        args.push(format!("rc:{}", ctx.rc_file.data));

        // Directory containing *.data files.
        args.push(format!("data:{}", ctx.data_dir.data));

        // Taskwarrior version, same as returned by "task --version"
        args.push(format!("version:{VERSION}"));

        Ok(args)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ private `Hooks::callHookScript`: runs `script` with the hook
    /// arguments, feeding `input` one line each on stdin. Returns the exit
    /// status and the stdout lines (libshared `split (output, '\n')`).
    ///
    /// # Errors
    /// Those of `execute` (`throw std::string`).
    fn call_hook_script(
        &self,
        ctx: &mut Context,
        script: &str,
        input: &[String],
    ) -> Result<(i32, Vec<String>)> {
        if self.debug >= 1 {
            ctx.debug(&format!("Hook: Calling {script}"));
        }

        if self.debug >= 2 {
            ctx.debug("Hook: input");
            for i in input {
                ctx.debug(&format!("  {i}"));
            }
        }

        let mut input_str = String::new();
        for i in input {
            input_str.push_str(i);
            input_str.push('\n');
        }

        let args = Self::build_hook_script_args(ctx)?;
        if self.debug >= 2 {
            ctx.debug("Hooks: args");
            for arg in &args {
                ctx.debug(&format!("  {arg}"));
            }
        }

        // Measure time for each hook if running in debug
        let (status, output_str) = if self.debug >= 2 {
            let timer = Timer::new();
            let result = execute(script, &args, &input_str)?;
            ctx.debug_timing(&format("Hooks::execute ({1})", &[&script]), &timer);
            result
        } else {
            execute(script, &args, &input_str)?
        };

        let output = split(&output_str, '\n');

        if self.debug >= 2 {
            ctx.debug("Hook: output");
            for i in &output {
                if !i.is_empty() {
                    ctx.debug(&format!("  {i}"));
                }
            }

            ctx.debug(&format("Hook: Completed with status {1}", &[&status]));
            ctx.debug(" "); // Blank line
        }

        Ok((status, output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::env_lock;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    /// Fields drop in declaration order: the Context and the TempDir go
    /// before the lock is released.
    struct Fixture {
        ctx: Context,
        hooks_dir: String,
        _dir: TempDir,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl Fixture {
        /// A Context on a fresh replica, with `rc.hooks.location` pointing at
        /// an empty `<tmp>/hooks` directory.
        fn new() -> Fixture {
            // Context::new reads $HOME; serialize with env-mutating tests.
            let lock = env_lock();
            let dir = TempDir::new().unwrap();
            let data = dir.path().join("data");
            std::fs::create_dir(&data).unwrap();
            let hooks_dir = dir.path().join("hooks");
            std::fs::create_dir(&hooks_dir).unwrap();

            let mut ctx = Context::new();
            ctx.tdb2
                .open_replica_at(data.to_str().unwrap(), true, true)
                .unwrap();
            ctx.config.set("hooks", "on");
            ctx.config
                .set("hooks.location", hooks_dir.to_str().unwrap());
            Fixture {
                ctx,
                hooks_dir: hooks_dir.to_str().unwrap().to_string(),
                _dir: dir,
                _lock: lock,
            }
        }

        /// Writes `<hooks>/<name>` with a `/bin/sh` body and the given mode.
        fn script(&self, name: &str, body: &str, mode: u32) -> String {
            let path = format!("{}/{name}", self.hooks_dir);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        }

        fn hook(&self, name: &str, body: &str) -> String {
            self.script(name, body, 0o755)
        }

        fn init(&mut self) {
            Hooks::initialize(&mut self.ctx).unwrap();
        }
    }

    fn new_task(description: &str) -> Task {
        let mut task = Task::new();
        task.set("description", description);
        task.set("uuid", "a0a0a0a0-0000-4000-8000-000000000001");
        task.set("status", "pending");
        task.set("entry", "1700000000");
        task
    }

    fn is_terminate(result: Result<()>) -> bool {
        matches!(result, Err(TwError::Status(0)))
    }

    #[test]
    fn default_is_enabled_without_scripts() {
        let hooks = Hooks::new();
        assert!(hooks.enabled);
        assert_eq!(hooks.debug, 0);
        assert!(hooks.list().is_empty());
    }

    #[test]
    fn enable_returns_the_old_value() {
        let mut hooks = Hooks::new();
        assert!(hooks.enable(false));
        assert!(!hooks.enable(true));
        assert!(hooks.enable(true));
    }

    #[test]
    fn is_json_and_separate_output() {
        let hooks = Hooks::new();
        assert!(!hooks.is_json("{}"));
        assert!(hooks.is_json("{ }"));
        assert!(!hooks.is_json(" {}}"));
        assert!(!hooks.is_json("{a} "));
        let output: Vec<String> = ["{\"a\":1}", "feedback", "", "{x}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (json, feedback) = hooks.separate_output(&output);
        assert_eq!(json, vec!["{\"a\":1}".to_string(), "{x}".to_string()]);
        assert_eq!(feedback, vec!["feedback".to_string(), String::new()]);
    }

    #[test]
    fn initialize_lists_sorted_entries_and_reports_them_in_debug() {
        let mut f = Fixture::new();
        let b = f.hook("on-launch-b", "exit 0");
        let a = f.hook("on-add-a", "exit 0");
        let misnamed = f.hook("misnamed", "exit 0");
        let sub = format!("{}/on-exit-dir", f.hooks_dir);
        std::fs::create_dir(&sub).unwrap();
        f.ctx.config.set("debug.hooks", 1);
        f.init();

        let mut expected = vec![a.clone(), b.clone(), misnamed.clone(), sub];
        expected.sort();
        assert_eq!(f.ctx.hooks.list(), expected);
        assert_eq!(f.ctx.hooks.debug, 1);
        // Directories are listed but not reported.
        assert_eq!(
            f.ctx.debug_messages,
            vec![
                format!("Found misnamed hook script {misnamed}"),
                format!("Found hook script {a}"),
                format!("Found hook script {b}"),
            ]
        );
        assert!(f.ctx.hooks.enabled);
    }

    #[test]
    fn initialize_without_debug_is_silent_and_honours_rc_hooks() {
        let mut f = Fixture::new();
        f.hook("on-add", "exit 0");
        f.ctx.config.set("hooks", "off");
        f.init();
        assert_eq!(f.ctx.hooks.list().len(), 1);
        assert!(f.ctx.debug_messages.is_empty());
        assert!(!f.ctx.hooks.enabled);
    }

    #[test]
    fn initialize_reports_an_unreadable_directory() {
        let mut f = Fixture::new();
        let missing = format!("{}/missing", f.hooks_dir);
        f.ctx.config.set("hooks.location", &missing);
        f.ctx.config.set("debug.hooks", 2);
        f.init();
        assert!(f.ctx.hooks.list().is_empty());
        assert_eq!(
            f.ctx.debug_messages,
            vec![format!("Hook directory not readable: {missing}")]
        );
    }

    #[test]
    fn initialize_defaults_to_the_data_location() {
        let mut f = Fixture::new();
        let data = f._dir.path().join("data");
        let data_hooks = data.join("hooks");
        std::fs::create_dir(&data_hooks).unwrap();
        let script = data_hooks.join("on-launch");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        f.ctx.config.remove("hooks.location");
        f.ctx.config.set("data.location", data.to_str().unwrap());
        f.init();
        assert_eq!(
            f.ctx.hooks.list(),
            vec![script.to_str().unwrap().to_string()]
        );
    }

    #[test]
    fn scripts_selects_executable_scripts_of_the_event() {
        let mut f = Fixture::new();
        let add1 = f.hook("on-add.1", "exit 0");
        f.script("on-add.2", "exit 0", 0o644);
        let add3 = f.hook("on-add3", "exit 0");
        f.hook("on-modify", "exit 0");
        f.hook("xon-add", "exit 0");
        f.init();
        assert_eq!(f.ctx.hooks.scripts("on-add").unwrap(), vec![add1, add3]);
        assert!(f.ctx.hooks.scripts("on-exit").unwrap().is_empty());
    }

    #[test]
    fn on_launch_feedback_becomes_footnotes() {
        let mut f = Fixture::new();
        f.hook("on-launch-1", "echo first; echo");
        f.hook("on-launch-2", "echo second");
        f.init();
        Hooks::on_launch(&mut f.ctx).unwrap();
        assert_eq!(
            f.ctx.footnotes,
            vec!["first".to_string(), "second".to_string()]
        );
        assert!(f.ctx.errors.is_empty());
    }

    #[test]
    fn on_launch_failure_becomes_errors_and_terminates() {
        let mut f = Fixture::new();
        f.hook("on-launch-1", "echo first; echo bad; exit 1");
        f.hook("on-launch-2", "echo never");
        f.init();
        assert!(is_terminate(Hooks::on_launch(&mut f.ctx)));
        assert_eq!(f.ctx.errors, vec!["first".to_string(), "bad".to_string()]);
        assert!(f.ctx.footnotes.is_empty());
    }

    #[test]
    fn on_launch_failure_needs_feedback() {
        let mut f = Fixture::new();
        f.hook("on-launch", "echo '  '; exit 1");
        f.init();
        assert!(is_terminate(Hooks::on_launch(&mut f.ctx)));
        assert_eq!(
            f.ctx.errors,
            vec!["Hook Error: Expected feedback from failing hook script: on-launch".to_string()]
        );
    }

    #[test]
    fn on_launch_rejects_json() {
        let mut f = Fixture::new();
        f.hook("on-launch", "echo '{\"a\":1}'; echo fine");
        f.init();
        assert!(is_terminate(Hooks::on_launch(&mut f.ctx)));
        assert_eq!(
            f.ctx.errors,
            vec![
                "Hook Error: Expected 0 JSON task(s), found 1, in hook script: on-launch"
                    .to_string()
            ]
        );
        assert!(f.ctx.footnotes.is_empty());
    }

    #[test]
    fn disabled_hooks_do_not_run() {
        let mut f = Fixture::new();
        f.hook("on-launch", "echo bad; exit 1");
        f.ctx.config.set("hooks", 0);
        f.init();
        Hooks::on_launch(&mut f.ctx).unwrap();
        assert!(f.ctx.errors.is_empty());
        assert_eq!(f.ctx.time_hooks_us, 0);
    }

    #[test]
    fn on_add_receives_compose_json_and_takes_the_returned_task() {
        let mut f = Fixture::new();
        let log = format!("{}/../input.log", f.hooks_dir);
        f.hook(
            "on-add-1",
            &format!("read line; echo \"$line\" > '{log}'; echo \"$line\" | sed 's/\"one\"/\"two\"/'; echo added"),
        );
        f.hook(
            "on-add-2",
            "read line; echo \"$line\" | sed 's/\"two\"/\"three\"/'",
        );
        f.init();

        let mut task = new_task("one");
        let expected_input = task.compose_json(&mut f.ctx, false).unwrap();
        Hooks::on_add(&mut f.ctx, &mut task).unwrap();

        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            format!("{expected_input}\n")
        );
        assert_eq!(task.get("description"), "three");
        assert_eq!(task.get("uuid"), "a0a0a0a0-0000-4000-8000-000000000001");
        assert_eq!(f.ctx.footnotes, vec!["added".to_string()]);
        assert!(f.ctx.errors.is_empty());
        assert!(f.ctx.time_hooks_us >= 0);
    }

    #[test]
    fn on_add_without_scripts_leaves_the_task_alone() {
        let mut f = Fixture::new();
        f.init();
        let mut task = new_task("one");
        let before = task.clone();
        Hooks::on_add(&mut f.ctx, &mut task).unwrap();
        assert_eq!(task, before);
    }

    #[test]
    fn on_add_failure_ignores_json_and_terminates() {
        let mut f = Fixture::new();
        f.hook("on-add", "read line; echo \"$line\"; echo rejected; exit 1");
        f.init();
        let mut task = new_task("one");
        assert!(is_terminate(Hooks::on_add(&mut f.ctx, &mut task)));
        assert_eq!(f.ctx.errors, vec!["rejected".to_string()]);
        assert_eq!(task.get("description"), "one");
    }

    #[test]
    fn on_add_needs_exactly_one_task() {
        let mut f = Fixture::new();
        f.hook("on-add", "cat >/dev/null; echo ok");
        f.init();
        let mut task = new_task("one");
        assert!(is_terminate(Hooks::on_add(&mut f.ctx, &mut task)));
        assert_eq!(
            f.ctx.errors,
            vec![
                "Hook Error: Expected 1 JSON task(s), found 0, in hook script: on-add".to_string()
            ]
        );
    }

    #[test]
    fn on_add_reports_json_syntax_errors() {
        let mut f = Fixture::new();
        f.hook("on-add", "cat >/dev/null; echo '{\"description\":}'");
        f.init();
        let mut task = new_task("one");
        assert!(is_terminate(Hooks::on_add(&mut f.ctx, &mut task)));
        assert_eq!(
            f.ctx.errors,
            vec!["Hook Error: JSON syntax error in: {\"description\":}".to_string()]
        );

        // With rc.debug.hooks the parser message follows.
        let mut f = {
            drop(f);
            Fixture::new()
        };
        f.hook("on-add", "cat >/dev/null; echo '{\"description\":}'");
        f.ctx.config.set("debug.hooks", 1);
        f.init();
        let mut task = new_task("one");
        assert!(is_terminate(Hooks::on_add(&mut f.ctx, &mut task)));
        assert_eq!(f.ctx.errors.len(), 2);
        assert!(
            f.ctx.errors[1].starts_with("Hook Error: JSON Error: "),
            "{:?}",
            f.ctx.errors
        );
    }

    #[test]
    fn on_add_requires_description_and_uuid() {
        let mut f = Fixture::new();
        f.hook("on-add", "cat >/dev/null; echo '{\"uuid\":\"x\"}'");
        f.init();
        let mut task = new_task("one");
        assert!(is_terminate(Hooks::on_add(&mut f.ctx, &mut task)));
        assert_eq!(
            f.ctx.errors,
            vec![
                "Hook Error: JSON Object missing 'description' attribute from hook script: on-add"
                    .to_string(),
                "Hook Error: JSON failed to parse: {\"uuid\":\"x\"}".to_string(),
            ]
        );
        drop(f);

        let mut f = Fixture::new();
        f.hook("on-add", "cat >/dev/null; echo '{\"description\":\"x\"}'");
        f.init();
        assert!(is_terminate(Hooks::on_add(&mut f.ctx, &mut task)));
        assert_eq!(
            f.ctx.errors,
            vec![
                "Hook Error: JSON Object missing 'uuid' attribute from hook script: on-add"
                    .to_string(),
                "Hook Error: JSON failed to parse: {\"description\":\"x\"}".to_string(),
            ]
        );
    }

    #[test]
    fn on_add_requires_the_same_task() {
        let mut f = Fixture::new();
        f.hook(
            "on-add",
            "cat >/dev/null; echo '{\"description\":\"x\",\"uuid\":\"other\"}'",
        );
        f.init();
        let mut task = new_task("one");
        assert!(is_terminate(Hooks::on_add(&mut f.ctx, &mut task)));
        assert_eq!(
            f.ctx.errors,
            vec![
                "Hook Error: JSON must be for the same task: a0a0a0a0-0000-4000-8000-000000000001 != other, in hook script: on-add"
                    .to_string()
            ]
        );
        drop(f);

        let mut f = Fixture::new();
        f.hook(
            "on-add",
            "cat >/dev/null; echo '{\"description\":\"x\",\"uuid\":12}'",
        );
        f.init();
        assert!(is_terminate(Hooks::on_add(&mut f.ctx, &mut task)));
        assert_eq!(
            f.ctx.errors,
            vec![
                "Hook Error: JSON must be for the same task: a0a0a0a0-0000-4000-8000-000000000001, in hook script: on-add"
                    .to_string()
            ]
        );
    }

    #[test]
    fn on_modify_receives_before_and_after() {
        let mut f = Fixture::new();
        let log = format!("{}/../input.log", f.hooks_dir);
        f.hook(
            "on-modify",
            &format!("read before; read after; printf '%s\\n%s\\n' \"$before\" \"$after\" > '{log}'; echo \"$after\" | sed 's/\"two\"/\"three\"/'; echo changed"),
        );
        f.init();
        let mut before = new_task("one");
        let mut after = new_task("two");
        let before_json = before.compose_json(&mut f.ctx, false).unwrap();
        let after_json = after.compose_json(&mut f.ctx, false).unwrap();
        Hooks::on_modify(&mut f.ctx, &mut before, &mut after).unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            format!("{before_json}\n{after_json}\n")
        );
        assert_eq!(after.get("description"), "three");
        assert_eq!(before.get("description"), "one");
        assert_eq!(f.ctx.footnotes, vec!["changed".to_string()]);
    }

    #[test]
    fn on_modify_requires_the_same_task_as_before() {
        let mut f = Fixture::new();
        f.hook(
            "on-modify",
            "read before; read after; echo '{\"description\":\"x\",\"uuid\":\"other\"}'",
        );
        f.init();
        let mut before = new_task("one");
        let mut after = new_task("two");
        assert!(is_terminate(Hooks::on_modify(
            &mut f.ctx,
            &mut before,
            &mut after
        )));
        assert_eq!(
            f.ctx.errors,
            vec![
                "Hook Error: JSON must be for the same task: a0a0a0a0-0000-4000-8000-000000000001 != other, in hook script: on-modify"
                    .to_string()
            ]
        );
        assert_eq!(after.get("description"), "two");
    }

    #[test]
    fn on_exit_receives_the_changed_tasks() {
        let mut f = Fixture::new();
        let log = format!("{}/../input.log", f.hooks_dir);
        f.hook("on-exit", &format!("cat > '{log}'; echo bye"));
        f.init();
        let mut task = new_task("one");
        crate::tdb2::add(&mut f.ctx, &mut task).unwrap();
        let mut expected = String::new();
        for mut t in f.ctx.tdb2.get_changes() {
            expected.push_str(&t.compose_json(&mut f.ctx, false).unwrap());
            expected.push('\n');
        }
        assert!(!expected.is_empty());
        Hooks::on_exit(&mut f.ctx).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), expected);
        assert_eq!(f.ctx.footnotes, vec!["bye".to_string()]);
    }

    #[test]
    fn on_exit_rejects_json() {
        let mut f = Fixture::new();
        f.hook("on-exit", "echo '{\"a\":1}'");
        f.init();
        assert!(is_terminate(Hooks::on_exit(&mut f.ctx)));
        assert_eq!(
            f.ctx.errors,
            vec![
                "Hook Error: Expected 0 JSON task(s), found 1, in hook script: on-exit".to_string()
            ]
        );
    }

    #[test]
    fn hook_scripts_get_the_api_arguments() {
        let mut f = Fixture::new();
        f.hook("on-launch", "for a in \"$@\"; do echo \"[$a]\"; done");
        f.init();
        Hooks::on_launch(&mut f.ctx).unwrap();
        assert_eq!(
            f.ctx.footnotes,
            vec![
                "[api:2]".to_string(),
                "[args:]".to_string(),
                "[command:]".to_string(),
                format!("[rc:{}]", f.ctx.rc_file.data),
                format!("[data:{}]", f.ctx.data_dir.data),
                format!("[version:{VERSION}]"),
            ]
        );
    }

    #[test]
    fn debug_level_two_dumps_input_args_and_output() {
        let mut f = Fixture::new();
        let script = f.hook("on-add", "read line; echo \"$line\"; echo; echo note");
        f.ctx.config.set("debug.hooks", 2);
        f.init();
        f.ctx.debug_messages.clear();
        let mut task = new_task("one");
        let json = task.compose_json(&mut f.ctx, false).unwrap();
        Hooks::on_add(&mut f.ctx, &mut task).unwrap();

        let messages = &f.ctx.debug_messages;
        let mut expected = vec![
            format!("Hook: Calling {script}"),
            "Hook: input".to_string(),
            format!("  {json}"),
            "Hooks: args".to_string(),
            "  api:2".to_string(),
            "  args:".to_string(),
            "  command:".to_string(),
            format!("  rc:{}", f.ctx.rc_file.data),
            format!("  data:{}", f.ctx.data_dir.data),
            format!("  version:{VERSION}"),
        ];
        assert_eq!(messages[..expected.len()], expected[..]);
        assert!(messages[expected.len()].starts_with(&format!("Timer Hooks::execute ({script}) ")));
        expected.clear();
        expected.extend([
            "Hook: output".to_string(),
            format!("  {json}"),
            "  note".to_string(),
            "Hook: Completed with status 0".to_string(),
            " ".to_string(),
        ]);
        // (A bare Context has no column registry, so `Task::parse` adds its
        // orphan-attribute debug lines after these.)
        let start = messages.iter().position(|m| m == "Hook: output").unwrap();
        assert_eq!(messages[start..start + expected.len()], expected[..]);
        // Blank output lines are feedback, but neither debug lines nor
        // footnotes (`Context::footnote` drops empty messages).
        assert_eq!(f.ctx.footnotes, vec!["note".to_string()]);
    }

    #[test]
    fn debug_level_one_only_names_the_script() {
        let mut f = Fixture::new();
        let script = f.hook("on-launch", "exit 0");
        f.ctx.config.set("debug.hooks", 1);
        f.init();
        f.ctx.debug_messages.clear();
        Hooks::on_launch(&mut f.ctx).unwrap();
        assert_eq!(
            f.ctx.debug_messages,
            vec![format!("Hook: Calling {script}")]
        );
    }
}
