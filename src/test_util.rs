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
