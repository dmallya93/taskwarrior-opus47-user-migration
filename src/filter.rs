//! Rust translation of `src/Filter.h` / `src/Filter.cpp`: task selection by
//! evaluating the command line's `FILTER` arguments (compiled once by
//! [`Eval`]) against every candidate task.
//!
//! # API
//!
//! | C++ | Rust |
//! |-----|------|
//! | `Filter ()` | [`Filter::new`] / `Filter::default ()` |
//! | `void subset (const std::vector<Task>&, std::vector<Task>&)` | [`Filter::subset_of`]`(ctx, input, &mut output) -> Result<()>` |
//! | `void subset (std::vector<Task>&)` | [`Filter::subset`]`(ctx, &mut output) -> Result<()>` |
//! | `bool hasFilter () const` | [`Filter::has_filter`]`(ctx)` |
//! | `bool pendingOnly () const` | [`Filter::pending_only`]`(ctx)` |
//! | `void safety () const` | [`Filter::safety`]`(ctx) -> Result<()>` |
//! | `void disableSafety ()` | [`Filter::disable_safety`] |
//!
//! # Borrowing (milestone 5 Design Decision 2, option 1)
//!
//! The C++ reads `Context::getContext ()` for `cli2`, `config`, `tdb2`, the
//! debug sink and `time_filter_us`; here every method takes the Context
//! explicitly. The candidate tasks are always an owned `Vec<Task>` (or the
//! caller's slice): `tdb2.pending_tasks ()` / `completed_tasks ()` are copied
//! out of the TDB2 cache exactly as the C++ `auto pending = ...` copies the
//! returned reference. Each task is then evaluated with a
//! [`ContextResolver::with_task`]`(ctx, Some (&task))`, the equivalent of the
//! C++ `Context::getContext ().withCurrentTask (&task)` guard: DOM lookups,
//! `~`/`!~` and `_hastag_`/`_notag_` (virtual tags included) see that task and
//! the full Context. The compile step uses [`ContextResolver::new`], i.e.
//! C++ `domSource` with whatever `ctx.current_task` is (normally `None`).
//!
//! # Errors
//!
//! Everything C++ throws (CLI2 `prepareFilter`, Eval syntax and evaluation
//! errors, TDB2 errors, the [`safety`](Filter::safety) refusals) propagates
//! as `Err` with the C++ text.

use crate::cli2;
use crate::context::Context;
use crate::dom::ContextResolver;
use crate::error::{Result, TwError};
use crate::eval::{Eval, Token};
use crate::lexer::Type as LexerType;
use crate::shared::format::format;
use crate::shared::shared::confirm;
use crate::shared::timer::Timer;
use crate::task::Task;

/// C++ `class Filter`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// C++ `_startCount`.
    pub _start_count: i32,
    /// C++ `_endCount`.
    pub _end_count: i32,
    /// C++ `_safety` (default `true`).
    pub _safety: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            _start_count: 0,
            _end_count: 0,
            _safety: true,
        }
    }
}

/// C++ `Timer::total_us ()` as the `long` the Context timers accumulate.
fn timer_us(timer: &Timer) -> i64 {
    timer.total_us() as i64
}

/// C++ `Context::getContext ().cli2.prepareFilter ()` followed by the loop
/// gathering the `FILTER` arguments as `(getToken (), _lextype)` pairs.
fn prepare_and_precompile(ctx: &mut Context) -> Result<Vec<Token>> {
    cli2::with_cli2(ctx, |cli2, ctx| cli2.prepare_filter(ctx))?;

    let mut precompiled: Vec<Token> = Vec::new();
    for a in &ctx.cli2.args {
        if a.has_tag("FILTER") {
            precompiled.push((a.get_token(), a.lextype));
        }
    }

    Ok(precompiled)
}

/// C++ `Eval eval; eval.addSource (domSource); eval.debug (...);
/// eval.compileExpression (precompiled);`.
fn compile(ctx: &mut Context, precompiled: &[Token]) -> Result<Eval> {
    let mut eval = Eval::new();

    // Debug output from Eval during compilation is useful.  During evaluation
    // it is mostly noise.
    eval.debug(ctx.config.get_integer("debug.parser") >= 3);
    eval.compile_expression(precompiled, &mut ContextResolver::new(ctx))?;

    Ok(eval)
}

/// The C++ per-task loop body: with `task` as the current task, evaluate the
/// compiled filter and keep the task if the result is true.
fn evaluate_into(
    ctx: &mut Context,
    eval: &Eval,
    tasks: &[Task],
    output: &mut Vec<Task>,
) -> Result<()> {
    for task in tasks {
        // Set up context for any DOM references.
        let var =
            eval.evaluate_compiled_expression(&mut ContextResolver::with_task(ctx, Some(task)))?;
        if var.get_bool() {
            output.push(task.clone());
        }
    }

    Ok(())
}

impl Filter {
    /// C++ `Filter () = default`.
    pub fn new() -> Self {
        Filter::default()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Filter::subset (const std::vector<Task>& input,
    /// std::vector<Task>& output)`: take an input set of tasks and filter
    /// into a subset.
    ///
    /// As in C++, with a filter the matching tasks are *appended* to
    /// `output`; without one, `output` is replaced by a copy of `input`.
    /// No [`safety`](Self::safety) check is made here.
    pub fn subset_of(
        &mut self,
        ctx: &mut Context,
        input: &[Task],
        output: &mut Vec<Task>,
    ) -> Result<()> {
        let timer = Timer::new();
        self._start_count = input.len() as i32;

        let precompiled = prepare_and_precompile(ctx)?;

        if !precompiled.is_empty() {
            let mut eval = compile(ctx, &precompiled)?;

            evaluate_into(ctx, &eval, input, output)?;

            eval.debug(false);
        } else {
            *output = input.to_vec();
        }

        self._end_count = output.len() as i32;
        ctx.debug(&format(
            "Filtered {1} tasks --> {2} tasks [list subset]",
            &[&self._start_count, &self._end_count],
        ));
        ctx.time_filter_us += timer_us(&timer);
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Filter::subset (std::vector<Task>& output)`: take the set of all
    /// tasks and filter into a subset.
    ///
    /// With a filter, the pending tasks are evaluated first; the completed
    /// tasks are loaded and evaluated too unless [`pending_only`]
    /// (Self::pending_only) says the filter cannot match them. Without a
    /// filter, [`safety`](Self::safety) runs and `output` becomes all tasks.
    /// TDB2 load times are excluded from `time_filter_us`, as in C++.
    pub fn subset(&mut self, ctx: &mut Context, output: &mut Vec<Task>) -> Result<()> {
        let timer = Timer::new();
        let precompiled = prepare_and_precompile(ctx)?;

        // Shortcut indicates that only pending.data needs to be loaded.
        let mut shortcut = false;

        if !precompiled.is_empty() {
            let timer_pending = Timer::new();
            let pending: Vec<Task> = ctx.tdb2.pending_tasks()?.to_vec();
            ctx.time_filter_us -= timer_us(&timer_pending);
            self._start_count = pending.len() as i32;

            let mut eval = compile(ctx, &precompiled)?;

            output.clear();
            evaluate_into(ctx, &eval, &pending, output)?;

            shortcut = self.pending_only(ctx);
            if !shortcut {
                let timer_completed = Timer::new();
                let completed: Vec<Task> = ctx.tdb2.completed_tasks()?.to_vec();
                ctx.time_filter_us -= timer_us(&timer_completed);
                self._start_count += completed.len() as i32;

                evaluate_into(ctx, &eval, &completed, output)?;
            }

            eval.debug(false);
        } else {
            self.safety(ctx)?;

            let pending_completed = Timer::new();
            *output = ctx.tdb2.all_tasks()?;
            ctx.time_filter_us -= timer_us(&pending_completed);
        }

        self._end_count = output.len() as i32;
        let which = if shortcut {
            "pending only"
        } else {
            "all tasks"
        };
        ctx.debug(&format(
            "Filtered {1} tasks --> {2} tasks [{3}]",
            &[&self._start_count, &self._end_count, &which],
        ));
        ctx.time_filter_us += timer_us(&timer);
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Filter::hasFilter`: whether any argument is tagged `FILTER`.
    pub fn has_filter(&self, ctx: &Context) -> bool {
        ctx.cli2.args.iter().any(|a| a.has_tag("FILTER"))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Filter::pendingOnly`.
    ///
    /// If the filter contains no 'or', 'xor' or 'not' operators, and only
    /// includes status values 'pending', 'waiting' or 'recurring', then the
    /// filter is guaranteed to only need data from pending.data.
    pub fn pending_only(&self, ctx: &Context) -> bool {
        // When GC is off, there are no shortcuts.
        if !ctx.config.get_boolean("gc") {
            return false;
        }

        // To skip loading completed.data, there should be:
        // - 'status' in filter
        // - no 'completed'
        // - no 'deleted'
        // - no 'xor'
        // - no 'or'
        let mut count_status = 0;
        let mut count_pending = 0;
        let mut count_waiting = 0;
        let mut count_recurring = 0;
        let count_id = ctx.cli2.id_ranges.len();
        let count_uuid = ctx.cli2.uuid_list.len();
        let mut count_or = 0;
        let mut count_xor = 0;
        let mut count_not = 0;
        let mut pending_tag = false;
        let mut active_tag = false;

        for a in &ctx.cli2.args {
            if a.has_tag("FILTER") {
                let raw = a.attribute("raw");
                let canonical = a.attribute("canonical");

                if a.lextype == LexerType::op && raw == "or" {
                    count_or += 1;
                }
                if a.lextype == LexerType::op && raw == "xor" {
                    count_xor += 1;
                }
                if a.lextype == LexerType::op && raw == "not" {
                    count_not += 1;
                }
                if a.lextype == LexerType::dom && canonical == "status" {
                    count_status += 1;
                }
                if raw == "pending" {
                    count_pending += 1;
                }
                if raw == "waiting" {
                    count_waiting += 1;
                }
                if raw == "recurring" {
                    count_recurring += 1;
                }
            }
        }

        for word in &ctx.cli2.original_args {
            if word.attribute("raw") == "+PENDING" {
                pending_tag = true;
            }
            if word.attribute("raw") == "+ACTIVE" {
                active_tag = true;
            }
        }

        if count_uuid != 0 {
            return false;
        }

        if count_or != 0 || count_xor != 0 || count_not != 0 {
            return false;
        }

        if pending_tag || active_tag {
            return true;
        }

        if count_status != 0 {
            if count_pending == 0 && count_waiting == 0 && count_recurring == 0 {
                return false;
            }

            return true;
        }

        if count_id != 0 {
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Filter::safety`: disaster avoidance mechanism. If a !READONLY has
    /// no filter, then it can cause all tasks to be modified. This is usually
    /// not intended.
    ///
    /// Fails (C++ `throw std::string`) with the `allow.empty.filter` text, or
    /// with `Command prevented from running.` unless `confirmation` is on and
    /// the user confirms the prompt.
    pub fn safety(&self, ctx: &Context) -> Result<()> {
        if self._safety {
            let mut readonly = true;
            let mut filter = false;
            for a in &ctx.cli2.args {
                if a.has_tag("CMD") && !a.has_tag("READONLY") {
                    readonly = false;
                }

                if a.has_tag("FILTER") {
                    filter = true;
                }
            }

            if !readonly && !filter {
                if !ctx.config.get_boolean("allow.empty.filter") {
                    return Err(TwError::Message(
                        "You did not specify a filter, and with the 'allow.empty.filter' value, no action is taken."
                            .to_string(),
                    ));
                }

                // If user is willing to be asked, this can be avoided.
                if ctx.config.get_boolean("confirmation")
                    && confirm(
                        "This command has no filter, and will modify all (including completed and deleted) tasks.  Are you sure?",
                    )
                {
                    return Ok(());
                }

                // Sound the alarm.
                return Err(TwError::Message(
                    "Command prevented from running.".to_string(),
                ));
            }
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Filter::disableSafety`.
    pub fn disable_safety(&mut self) {
        self._safety = false;
    }
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;
