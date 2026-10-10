//! Rust translation of `src/dependency.h` / `src/dependency.cpp`.
//!
//! API mapping (no global Context):
//! * `dependencyIsCircular (const Task&)` -> [`dependency_is_circular`],
//!   which takes the [`Tdb2`] whose pending-task cache it traverses.
//! * `dependencyChainOnComplete (Task&)` -> [`dependency_chain_on_complete`]
//!   and `dependencyChainOnStart (Task&)` -> [`dependency_chain_on_start`]
//!   take the [`Context`] (config, `tdb2`, footnotes, `TDB2::modify`). C++
//!   writes their messages to `std::cout`; the `_to` variants write to a
//!   given stream instead (the plain functions use stdout).
//! * Neither function modifies the task it is given, so they take `&Task`
//!   (a `&mut Task` coerces).

use std::collections::HashSet;
use std::io::Write;

use crate::context::Context;
use crate::shared::format::format;
use crate::shared::shared::confirm;
use crate::task::Task;
use crate::tdb2::{self, Tdb2};
use crate::Result;

/// C++ `#define STRING_DEPEND_BLOCKED "Task {1} is blocked by:"`.
pub const STRING_DEPEND_BLOCKED: &str = "Task {1} is blocked by:";

////////////////////////////////////////////////////////////////////////////////
/// C++ `bool dependencyIsCircular (const Task&)`: returns true if the
/// supplied task adds a cycle to the dependency chain. Only dependencies
/// within the pending task cache are traversed.
pub fn dependency_is_circular(tdb2: &mut Tdb2, task: &Task) -> Result<bool> {
    // A new task has no UUID assigned yet, and therefore cannot be part of any
    // dependency chain.
    if !task.has("uuid") {
        return Ok(false);
    }

    let task_uuid = task.get_ref("uuid");

    let mut visited: HashSet<String> = HashSet::from([task_uuid.to_string()]);
    let mut stack: Vec<String> = Vec::new();

    // We start with the current task, as it may contain changes which are not
    // cached.
    for dep in task.get_dependency_uuids() {
        if dep == task_uuid {
            return Ok(true);
        }
        if visited.insert(dep.clone()) {
            stack.push(dep);
        }
    }

    while let Some(dep_uuid) = stack.pop() {
        // Dependencies outside the pending task cache terminate the traversal.
        // We do not load the full database.
        let deps = match tdb2.find_pending(&dep_uuid)? {
            Some(dep_task) => dep_task.get_dependency_uuids(),
            None => continue,
        };

        // Marks UUIDs so each dependency is only checked once.
        for dep in deps {
            if dep == task_uuid {
                return Ok(true);
            }
            if visited.insert(dep.clone()) {
                stack.push(dep);
            }
        }
    }

    Ok(false)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void dependencyChainOnComplete (Task&)`: determine whether a
/// dependency chain is being broken, assuming that `task` is either
/// completed or deleted. Messages go to stdout.
///
/// ```text
///   blocked task blocking action
///   ------- ---- -------- -----------------------------
///           [1]  2        Chain broken
///                         Nag message generated
///                         Repair offered:  1 dep:-2
///
///           [1]  2        Chain broken
///                3        Nag message generated
///                         Repair offered:  1 dep:-2,-3
///
///   1       [2]           -
///
///   1,3     [2]           -
///
///   1       [2]  3        Chain broken
///                         Nag message generated
///                         Repair offered:  2 dep:-3
///                                          1 dep:-2,3
///
///   1,4     [2]  3,5      Chain broken
///                         Nag message generated
///                         Repair offered:  2 dep:-3,-5
///                                          1 dep:3,5
///                                          4 dep:3,5
/// ```
pub fn dependency_chain_on_complete(ctx: &mut Context, task: &Task) -> Result<()> {
    let mut stdout = std::io::stdout();
    dependency_chain_on_complete_to(ctx, task, &mut stdout)
}

/// [`dependency_chain_on_complete`] writing its messages to `out` (C++
/// `std::cout`). The confirmation prompt, if any, still uses the terminal.
pub fn dependency_chain_on_complete_to(ctx: &mut Context, task: &Task, out: &mut dyn Write) -> Result<()> {
    let blocking = task.get_dependency_tasks(&mut ctx.tdb2)?;

    // If the task is anything but the tail end of a dependency chain.
    if !blocking.is_empty() {
        let blocked = task.get_blocked_tasks(&mut ctx.tdb2)?;

        // Nag about broken chain.
        if ctx.config.get_boolean("dependency.reminder") {
            let _ = writeln!(out, "{}", format(STRING_DEPEND_BLOCKED, &[&task.identifier(false)]));

            for b in &blocking {
                let _ = writeln!(out, "  {} {}", b.id, b.get_ref("description"));
            }
        }

        // If there are both blocking and blocked tasks, the chain is broken.
        if !blocked.is_empty() {
            if ctx.config.get_boolean("dependency.reminder") {
                let _ = writeln!(out, "and is blocking:");

                for b in &blocked {
                    let _ = writeln!(out, "  {} {}", b.id, b.get_ref("description"));
                }
            }

            // The prompt shares std::cout with the messages above.
            let _ = out.flush();
            if !ctx.config.get_boolean("dependency.confirmation")
                || confirm("Would you like the dependency chain fixed?")
            {
                let mut blocked = blocked;
                let mut blocking = blocking;

                // Repair the chain - everything in blocked should now depend on
                // everything in blocking, instead of task.id.
                for left in blocked.iter_mut() {
                    left.remove_dependency_id(ctx, task.id)?;

                    for right in &blocking {
                        left.add_dependency_id(ctx, right.id)?;
                    }
                }

                // Now update TDB2, now that the updates have all occurred.
                for left in blocked.iter_mut() {
                    tdb2::modify(ctx, left)?;
                }

                for right in blocking.iter_mut() {
                    tdb2::modify(ctx, right)?;
                }
            }
        }
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void dependencyChainOnStart (Task&)`: with `rc.dependency.reminder`,
/// nag (on stdout) when a started task is still blocked.
pub fn dependency_chain_on_start(ctx: &mut Context, task: &Task) -> Result<()> {
    let mut stdout = std::io::stdout();
    dependency_chain_on_start_to(ctx, task, &mut stdout)
}

/// [`dependency_chain_on_start`] writing its messages to `out` (C++
/// `std::cout`).
pub fn dependency_chain_on_start_to(ctx: &mut Context, task: &Task, out: &mut dyn Write) -> Result<()> {
    if ctx.config.get_boolean("dependency.reminder") {
        let blocking = task.get_dependency_tasks(&mut ctx.tdb2)?;

        // If the task is anything but the tail end of a dependency chain, nag about
        // broken chain.
        if !blocking.is_empty() {
            let _ = writeln!(out, "{}", format(STRING_DEPEND_BLOCKED, &[&task.identifier(false)]));

            for b in &blocking {
                let _ = writeln!(out, "  {} {}", b.id, b.get_ref("description"));
            }
        }
    }

    Ok(())
}
