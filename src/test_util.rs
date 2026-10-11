//! Helpers shared by the crate's unit tests (compiled only for `cargo test`).
//!
//! Tests that change process-wide state (environment variables such as
//! `HOME`, `TASKRC`, `TASKDATA`, `XDG_CONFIG_HOME`, or the umask) must hold
//! [`env_lock`] for their whole duration, so they never interleave with each
//! other when the test harness runs tests in parallel threads.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

/// The crate-wide lock serializing tests that mutate process-wide state.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Acquires [`ENV_LOCK`], ignoring poisoning by an earlier failed test.
pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Saves environment variables and restores them (set or unset) when dropped.
pub(crate) struct EnvGuard(Vec<(&'static str, Option<OsString>)>);

impl EnvGuard {
    /// Remembers the current values of `names`.
    pub(crate) fn save(names: &[&'static str]) -> Self {
        EnvGuard(names.iter().map(|&n| (n, std::env::var_os(n))).collect())
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

/// Restores the thread-local statics `Context::initialize` writes (the CLI2
/// abbreviation length) and the stdout-TTY override.
struct ContextStatics(i32);

impl Drop for ContextStatics {
    fn drop(&mut self) {
        crate::cli2::set_minimum_match_length(self.0);
        crate::context::set_stdout_tty_override(None);
    }
}

/// A sandboxed, env-locked Context for unit-testing a command's `execute`:
/// a temporary `HOME`/`TASKRC`/`TASKDATA`, seeded with JSON tasks, then
/// initialized for `task <args...>`.
///
/// Fields drop in declaration order: Context and data directory first, then
/// statics and environment, the lock last.
pub(crate) struct CommandFixture {
    pub(crate) ctx: crate::context::Context,
    _dir: tempfile::TempDir,
    _statics: ContextStatics,
    _env: EnvGuard,
    _lock: MutexGuard<'static, ()>,
}

/// `Context::initialize` for `task <list...>`, asserting it succeeds.
fn initialized(list: &[&str]) -> crate::context::Context {
    let args: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    let mut ctx = crate::context::Context::new();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let rc = ctx.initialize_with(&args, &mut out, &mut err).unwrap();
    assert_eq!(rc, 0, "{}", String::from_utf8_lossy(&err));
    ctx
}

impl CommandFixture {
    /// Writes a taskrc (`recurrence=off`, `verbose=nothing`, then
    /// `rc_lines`), adds the `seed` JSON tasks, then initializes a Context
    /// for `task <args...>`.
    pub(crate) fn new(seed: &[&str], rc_lines: &str, args: &[&str]) -> Self {
        let lock = env_lock();
        let env = EnvGuard::save(&["HOME", "TASKRC", "TASKDATA", "XDG_CONFIG_HOME"]);
        let statics = ContextStatics(crate::cli2::minimum_match_length());
        let dir = tempfile::TempDir::new().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let home = format!("{base}/home");
        std::fs::create_dir(&home).unwrap();
        let rc = format!("{base}/taskrc");
        std::fs::write(&rc, format!("recurrence=off\nverbose=nothing\n{rc_lines}")).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("TASKRC", &rc);
        std::env::set_var("TASKDATA", format!("{base}/data"));
        std::env::remove_var("XDG_CONFIG_HOME");
        crate::context::set_stdout_tty_override(Some(false));

        {
            let mut ctx = initialized(&["task", "add", "seed"]);
            for json in seed {
                let mut task = crate::task::Task::from_input(&mut ctx, json).unwrap();
                crate::tdb2::add(&mut ctx, &mut task).unwrap();
            }
        }

        let mut full = vec!["task"];
        full.extend_from_slice(args);
        let ctx = initialized(&full);
        CommandFixture { ctx, _dir: dir, _statics: statics, _env: env, _lock: lock }
    }

    /// Runs `command`, asserting it writes nothing to the output buffer.
    pub(crate) fn run(&mut self, command: &dyn crate::commands::command::Command) -> crate::Result<i32> {
        let mut output = String::new();
        let result = command.execute(&mut self.ctx, &mut output);
        assert_eq!(output, "");
        result
    }

    /// The stored task with `uuid`.
    pub(crate) fn task(&mut self, uuid: &str) -> crate::task::Task {
        self.ctx.tdb2.get_by_uuid(uuid).unwrap().unwrap()
    }
}
