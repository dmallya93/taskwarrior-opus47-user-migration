//! Rust translation of `src/feedback.cpp`.
//!
//! Every function of the C++ file is here, under its snake_case name:
//!
//! | C++                                                 | Rust                          |
//! |-----------------------------------------------------|-------------------------------|
//! | `renderAttribute (name, value, format = "")`        | [`render_attribute`]          |
//! | `feedback_affected (effect)`                        | [`feedback_affected`]         |
//! | `feedback_affected (effect, int quantity)`          | [`feedback_affected_count`]   |
//! | `feedback_affected (effect, const Task&)`           | [`feedback_affected_task`]    |
//! | `feedback_reserved_tags (tag)`                      | [`feedback_reserved_tags`]    |
//! | `feedback_special_tags (task, tag)`                 | [`feedback_special_tags`]     |
//! | `feedback_unblocked (task)`                         | [`feedback_unblocked`]        |
//! | `feedback_backlog ()`                               | [`feedback_backlog`]          |
//! | `onProjectChange (Task&, bool scope = true)`        | [`on_project_change`]         |
//! | `onProjectChange (Task&, Task&)`                    | [`on_project_change_pair`]    |
//! | `onExpiration (Task&)`                              | [`on_expiration`]             |
//! | `static countTasks (all, project, pending, done)`   | `count_tasks` (private)       |
//!
//! The functions that write to C++ `std::cout` have a `_to` variant taking
//! the output stream (as `dependency.rs` does), so tests can capture it; the
//! plain variant writes to the process stdout. `feedback_backlog` goes to the
//! Context footnote sink, as in C++. `onProjectChange`/`onExpiration` return
//! the text, which the commands route themselves.

use std::io::Write;

use crate::columns::column::ColumnRegistry;
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::configuration::Configuration;
use crate::shared::datetime::Datetime;
use crate::shared::format::format;
use crate::task::{Status, Task};

/// C `strtoll (value.c_str (), nullptr, 10)`.
fn c_strtoll(text: &str) -> i64 {
    let Ok(text) = std::ffi::CString::new(text) else {
        // An interior NUL ends the C string; parse the prefix.
        let prefix = text.split('\0').next().unwrap_or("");
        return c_strtoll(prefix);
    };
    // SAFETY: `text` is a valid NUL-terminated C string for the duration of
    // the call, and a null `endptr` is allowed.
    unsafe { libc::strtoll(text.as_ptr(), std::ptr::null_mut(), 10) }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `std::string renderAttribute (const std::string& name, const
/// std::string& value, const std::string& format = "")`: a non-empty value
/// of a `date` column (from `columns`) is rendered with `format`, or
/// `rc.dateformat` when `format` is empty. Any other value, and a date value
/// whose `strtoll` is 0 (un-parseable), is returned unchanged.
pub fn render_attribute(
    columns: &ColumnRegistry,
    config: &Configuration,
    name: &str,
    value: &str,
    format_: &str,
) -> String {
    if let Some(col) = columns.get(name) {
        if col.type_() == "date" && !value.is_empty() {
            let epoch = c_strtoll(value);
            // Do not try to render an un-parseable value.
            if epoch == 0 {
                return value.to_string();
            }
            let d = Datetime::from_epoch(epoch);
            if format_.is_empty() {
                return d.to_string(&config.get("dateformat"));
            }

            return d.to_string(format_);
        }
    }

    value.to_string()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void feedback_affected (const std::string& effect)`: with the
/// `affected` verbosity token, prints `effect` on stdout.
pub fn feedback_affected(ctx: &mut Context, effect: &str) {
    feedback_affected_to(ctx, effect, &mut std::io::stdout());
}

/// [`feedback_affected`] writing to `out` (C++ `std::cout`).
pub fn feedback_affected_to(ctx: &mut Context, effect: &str, out: &mut dyn Write) {
    if ctx.verbose("affected") {
        let _ = writeln!(out, "{effect}");
    }
}

/// C++ `void feedback_affected (const std::string& effect, int quantity)`:
/// e.g. `Deleted 3 tasks`; `effect` contains `{1}` for the quantity.
pub fn feedback_affected_count(ctx: &mut Context, effect: &str, quantity: i32) {
    feedback_affected_count_to(ctx, effect, quantity, &mut std::io::stdout());
}

/// [`feedback_affected_count`] writing to `out` (C++ `std::cout`).
pub fn feedback_affected_count_to(ctx: &mut Context, effect: &str, quantity: i32, out: &mut dyn Write) {
    if ctx.verbose("affected") {
        let _ = writeln!(out, "{}", format(effect, &[&quantity]));
    }
}

/// C++ `void feedback_affected (const std::string& effect, const Task&
/// task)`: e.g. `Deleting task 123 'This is a test'`; `effect` contains
/// `{1}` for `task.identifier (true)` and `{2}` for the description.
pub fn feedback_affected_task(ctx: &mut Context, effect: &str, task: &Task) {
    feedback_affected_task_to(ctx, effect, task, &mut std::io::stdout());
}

/// [`feedback_affected_task`] writing to `out` (C++ `std::cout`).
pub fn feedback_affected_task_to(ctx: &mut Context, effect: &str, task: &Task, out: &mut dyn Write) {
    if ctx.verbose("affected") {
        let _ = writeln!(out, "{}", format(effect, &[&task.identifier(true), &task.get("description")]));
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void feedback_reserved_tags (const std::string& tag)`: error when
/// adding or removing a virtual tag name.
///
/// # Errors
/// `Virtual tags (including '<tag>') are reserved and may not be added or
/// removed.` for a virtual tag name (the C++ `throw std::string`).
pub fn feedback_reserved_tags(tag: &str) -> Result<()> {
    // Note: This list must match that in Task::hasTag.
    // Note: This list must match that in CmdInfo::execute.
    const RESERVED: [&str; 32] = [
        "ACTIVE",
        "ANNOTATED",
        "BLOCKED",
        "BLOCKING",
        "CHILD", // Deprecated 2.6.0
        "COMPLETED",
        "DELETED",
        "DUE",
        "DUETODAY",
        "INSTANCE",
        "LATEST",
        "MONTH",
        "ORPHAN",
        "OVERDUE",
        "PARENT", // Deprecated 2.6.0
        "PENDING",
        "PRIORITY",
        "PROJECT",
        "QUARTER",
        "READY",
        "SCHEDULED",
        "TAGGED",
        "TEMPLATE",
        "TODAY",
        "TOMORROW",
        "UDA",
        "UNBLOCKED",
        "UNTIL",
        "WAITING",
        "WEEK",
        "YEAR",
        "YESTERDAY",
    ];
    if RESERVED.contains(&tag) {
        return Err(TwError::Message(format(
            "Virtual tags (including '{1}') are reserved and may not be added or removed.",
            &[&tag],
        )));
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void feedback_special_tags (const Task& task, const std::string&
/// tag)`: with `rc.verbose` including `special`, explains the special tags
/// `nocolor`, `nonag`, `nocal` and `next` on stdout.
pub fn feedback_special_tags(ctx: &mut Context, task: &Task, tag: &str) {
    feedback_special_tags_to(ctx, task, tag, &mut std::io::stdout());
}

/// [`feedback_special_tags`] writing to `out` (C++ `std::cout`).
pub fn feedback_special_tags_to(ctx: &mut Context, task: &Task, tag: &str, out: &mut dyn Write) {
    if ctx.verbose("special") {
        let msg = if tag == "nocolor" {
            "The 'nocolor' special tag will disable color rules for this task."
        } else if tag == "nonag" {
            "The 'nonag' special tag will prevent nagging when this task is modified."
        } else if tag == "nocal" {
            "The 'nocal' special tag will keep this task off the 'calendar' report."
        } else if tag == "next" {
            "The 'next' special tag will boost the urgency of this task so it appears on the 'next' report."
        } else {
            ""
        };

        if !msg.is_empty() {
            // C++ `std::cout << format (msg, task.identifier ()) << "\n";`
            let _ = writeln!(out, "{}", format(msg, &[&task.identifier(false)]));
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void feedback_unblocked (const Task& task)`: called on completion,
/// deletion and update. With the `affected` verbosity token, every pending
/// task blocked by `task` that now has no pending dependency left is
/// reported as `Unblocked <id> '<description>'.` (the uuid when it has no
/// ID).
///
/// # Errors
/// As [`Task::get_blocked_tasks`] / [`Task::get_dependency_tasks`].
pub fn feedback_unblocked(ctx: &mut Context, task: &Task) -> Result<()> {
    feedback_unblocked_to(ctx, task, &mut std::io::stdout())
}

/// [`feedback_unblocked`] writing to `out` (C++ `std::cout`).
///
/// # Errors
/// As [`feedback_unblocked`].
pub fn feedback_unblocked_to(ctx: &mut Context, task: &Task, out: &mut dyn Write) -> Result<()> {
    if ctx.verbose("affected") {
        // Get a list of tasks that depended on this task.
        let blocked = task.get_blocked_tasks(&mut ctx.tdb2)?;

        // Scan all the tasks that were blocked by this task
        for i in &blocked {
            let blocking = i.get_dependency_tasks(&mut ctx.tdb2)?;
            if blocking.is_empty() {
                if i.id != 0 {
                    let _ = writeln!(out, "{}", format("Unblocked {1} '{2}'.", &[&i.id, &i.get("description")]));
                } else {
                    let _ =
                        writeln!(out, "{}", format("Unblocked {1} '{2}'.", &[&i.get("uuid"), &i.get("description")]));
                }
            }
        }
    }

    Ok(())
}

///////////////////////////////////////////////////////////////////////////////
/// C++ `void feedback_backlog ()`: when sync is set up
/// (`rc.sync.encryption_secret` non-empty) and the `sync` verbosity token is
/// on, footnotes the number of local changes awaiting sync.
///
/// # Errors
/// Replica errors from `TDB2::num_local_changes`.
pub fn feedback_backlog(ctx: &mut Context) -> Result<()> {
    // If non-local sync is not set up, do not provide this feedback.
    if ctx.config.get("sync.encryption_secret").is_empty() {
        return Ok(());
    }

    if ctx.verbose("sync") {
        let count = ctx.tdb2.num_local_changes()?;
        if count != 0 {
            let msg = format(
                if count > 1 {
                    "There are {1} local changes.  Sync required."
                } else {
                    "There is {1} local change.  Sync required."
                },
                &[&count],
            );
            ctx.footnote(&msg);
        }
    }

    Ok(())
}

///////////////////////////////////////////////////////////////////////////////
/// C++ `std::string onProjectChange (Task& task, bool scope = true)`: for a
/// task with a project, `The project '<p>' has changed.  ` (only when
/// `scope`) followed by `Project '<p>' is <n>% complete (<pending> of
/// <total> tasks remaining).` (or `(1 task remaining).`), counted over all
/// tasks. Empty for a task without a project.
///
/// # Errors
/// Replica errors from `TDB2::all_tasks`, or an invalid stored status (C++
/// `getStatus` throws).
pub fn on_project_change(ctx: &mut Context, task: &Task, scope: bool) -> Result<String> {
    let mut msg = String::new();
    let project = task.get("project");

    if !project.is_empty() {
        if scope {
            msg.push_str(&format("The project '{1}' has changed.", &[&project]));
            msg.push_str("  ");
        }

        // Count pending and done tasks, for this project.
        let all = ctx.tdb2.all_tasks()?;
        let (count_pending, count_done) = count_tasks(&all, &project)?;

        // count_done  count_pending  percentage
        // ----------  -------------  ----------
        //          0              0          0%
        //         >0              0        100%
        //          0             >0          0%
        //         >0             >0  calculated
        let percentage = if count_done == 0 {
            0
        } else if count_pending == 0 {
            100
        } else {
            count_done * 100 / (count_done + count_pending)
        };

        msg.push_str(&format("Project '{1}' is {2}% complete", &[&project, &percentage]));
        msg.push(' ');

        if count_pending == 1 && count_done == 0 {
            msg.push_str(&format("({1} task remaining).", &[&count_pending]));
        } else {
            msg.push_str(&format("({1} of {2} tasks remaining).", &[&count_pending, &(count_pending + count_done)]));
        }
    }

    Ok(msg)
}

///////////////////////////////////////////////////////////////////////////////
/// C++ `std::string onProjectChange (Task& task1, Task& task2)`: the
/// unscoped summary when both tasks have the same project, otherwise both
/// scoped summaries joined by a newline (or whichever is non-empty).
///
/// # Errors
/// As [`on_project_change`].
pub fn on_project_change_pair(ctx: &mut Context, task1: &Task, task2: &Task) -> Result<String> {
    if task1.get_ref("project") == task2.get_ref("project") {
        return on_project_change(ctx, task1, false);
    }

    let messages1 = on_project_change(ctx, task1, true)?;
    let messages2 = on_project_change(ctx, task2, true)?;

    if !messages1.is_empty() && !messages2.is_empty() {
        return Ok(messages1 + "\n" + &messages2);
    }

    Ok(messages1 + &messages2)
}

///////////////////////////////////////////////////////////////////////////////
/// C++ `std::string onExpiration (Task& task)`: with the `affected`
/// verbosity token, `Task <id> '<description>' expired and was deleted.`;
/// otherwise empty.
pub fn on_expiration(ctx: &mut Context, task: &Task) -> String {
    let mut msg = String::new();

    if ctx.verbose("affected") {
        msg.push_str(&format(
            "Task {1} '{2}' expired and was deleted.",
            &[&task.identifier(true), &task.get("description")],
        ));
    }

    msg
}

///////////////////////////////////////////////////////////////////////////////
/// C++ `static void countTasks (const std::vector<Task>& all, const
/// std::string& project, int& count_pending, int& count_done)`, returning
/// `(count_pending, count_done)`: pending and waiting tasks count as
/// pending, completed as done; deleted and recurring are ignored.
fn count_tasks(all: &[Task], project: &str) -> Result<(i32, i32)> {
    let mut count_pending = 0;
    let mut count_done = 0;
    for it in all {
        if it.get_ref("project") == project {
            match it.get_status()? {
                Status::Pending | Status::Waiting => count_pending += 1,
                Status::Completed => count_done += 1,
                _ => {}
            }
        }
    }

    Ok((count_pending, count_done))
}

#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;
