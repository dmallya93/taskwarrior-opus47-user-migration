//! Rust translation of `src/nag.cpp` / `src/nag.h`.
//!
//! C++ `void nag (std::vector<Task>& tasks)` reads `Context::getContext ()`
//! for `rc.nag`, the pending tasks and the footnote sink, so the Rust
//! [`nag`] takes the [`Context`] explicitly.

use crate::context::Context;
use crate::task::Task;
use crate::Result;

////////////////////////////////////////////////////////////////////////////////
/// C++ `void nag (std::vector<Task>& tasks)`: generates a nag message (the
/// `rc.nag` text, as a footnote) when there are READY tasks of a higher
/// urgency than one of `tasks` (ignoring those tagged `nonag`).
///
/// `tasks` is mutable as in C++: `urgency ()` caches its value in each task.
///
/// # Errors
/// Those C++ throws from `TDB2::pending_tasks`, `hasTag` and `urgency`.
pub fn nag(ctx: &mut Context, tasks: &mut [Task]) -> Result<()> {
    let msg = ctx.config.get("nag");
    if msg.is_empty() {
        return Ok(());
    }

    // C++ `auto pending = ...pending_tasks ()` copies the vector.
    let mut pending: Vec<Task> = ctx.tdb2.pending_tasks()?.to_vec();
    for t1 in tasks.iter_mut() {
        if t1.has_tag("nonag", ctx)? {
            continue;
        }

        for t2 in &mut pending {
            if t1.get("uuid") != t2.get("uuid")
                && t2.has_tag("READY", ctx)?
                && t1.urgency(ctx)? < t2.urgency(ctx)?
            {
                ctx.footnote(&msg);
                return Ok(());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tdb2;
    use crate::test_util::env_lock;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    const NAG: &str = "You have more urgent tasks.";

    /// Restores the thread-local user urgency coefficients.
    struct CoefficientsGuard;

    impl Drop for CoefficientsGuard {
        fn drop(&mut self) {
            Task::set_coefficients(BTreeMap::new());
        }
    }

    /// Fields drop in declaration order: the TempDir is removed before the
    /// lock is released.
    struct Fixture {
        ctx: Context,
        _dir: TempDir,
        _coefficients: CoefficientsGuard,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    /// A Context on a fresh replica holding a plain task "low" and a task
    /// "high" tagged `hot` (urgency 10).
    fn fixture(nag: &str) -> Fixture {
        // Context::new reads $HOME; serialize with env-mutating tests.
        let lock = env_lock();
        let dir = TempDir::new().unwrap();
        let mut ctx = Context::new();
        ctx.tdb2
            .open_replica_at(dir.path().to_str().unwrap(), true, true)
            .unwrap();
        ctx.config.set("nag", nag);
        let guard = CoefficientsGuard;
        Task::set_coefficients(BTreeMap::from([(
            "urgency.user.tag.hot.coefficient".to_string(),
            10.0,
        )]));

        let mut low = Task::new();
        low.set("description", "low");
        tdb2::add(&mut ctx, &mut low).unwrap();
        let mut high = Task::new();
        high.set("description", "high");
        high.add_tag("hot");
        tdb2::add(&mut ctx, &mut high).unwrap();

        Fixture {
            ctx,
            _dir: dir,
            _coefficients: guard,
            _lock: lock,
        }
    }

    fn task(ctx: &mut Context, description: &str) -> Task {
        ctx.tdb2
            .pending_tasks()
            .unwrap()
            .iter()
            .find(|t| t.get("description") == description)
            .unwrap()
            .clone()
    }

    #[test]
    fn nags_when_a_ready_task_is_more_urgent() {
        let mut f = fixture(NAG);
        let mut tasks = vec![task(&mut f.ctx, "low")];
        nag(&mut f.ctx, &mut tasks).unwrap();
        assert_eq!(f.ctx.footnotes, vec![NAG.to_string()]);
        // urgency () cached its value in the caller's task.
        assert_eq!(tasks[0].urgency(&mut f.ctx).unwrap(), 0.0);
    }

    #[test]
    fn no_nag_for_the_most_urgent_task() {
        let mut f = fixture(NAG);
        // Only "low" is less urgent, and the same task is never compared.
        let mut tasks = vec![task(&mut f.ctx, "high")];
        nag(&mut f.ctx, &mut tasks).unwrap();
        assert!(f.ctx.footnotes.is_empty());
    }

    #[test]
    fn no_nag_when_message_is_empty() {
        let mut f = fixture("");
        let mut tasks = vec![task(&mut f.ctx, "low")];
        nag(&mut f.ctx, &mut tasks).unwrap();
        assert!(f.ctx.footnotes.is_empty());
    }

    #[test]
    fn nonag_tasks_are_skipped() {
        let mut f = fixture(NAG);
        let mut low = task(&mut f.ctx, "low");
        low.add_tag("nonag");
        let mut tasks = vec![low];
        nag(&mut f.ctx, &mut tasks).unwrap();
        assert!(f.ctx.footnotes.is_empty());

        // A later task without nonag still nags; the footnote appears once.
        let mut tasks = vec![
            tasks.remove(0),
            task(&mut f.ctx, "low"),
            task(&mut f.ctx, "low"),
        ];
        nag(&mut f.ctx, &mut tasks).unwrap();
        nag(&mut f.ctx, &mut tasks).unwrap();
        assert_eq!(f.ctx.footnotes, vec![NAG.to_string()]);
    }

    #[test]
    fn only_ready_tasks_count() {
        let mut f = fixture(NAG);
        // A waiting task is not READY, however urgent.
        let mut high = task(&mut f.ctx, "high");
        high.set("wait", "4102444800");
        tdb2::modify(&mut f.ctx, &mut high).unwrap();
        let mut tasks = vec![task(&mut f.ctx, "low")];
        nag(&mut f.ctx, &mut tasks).unwrap();
        assert!(f.ctx.footnotes.is_empty());
    }
}
