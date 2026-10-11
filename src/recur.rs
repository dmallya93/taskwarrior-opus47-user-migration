//! Rust translation of `src/recur.cpp` / `src/recur.h`.
//!
//! API mapping:
//! * `std::optional<Datetime> checked_add_datetime (Datetime&, time_t)` ->
//!   [`checked_add_datetime`] returning `Option<Datetime>`.
//! * `void handleRecurrence ()` / `void handleUntil ()` ->
//!   [`handle_recurrence`] / [`handle_until`], taking the [`Context`]
//!   explicitly. Tasks are stored through [`tdb2::add`] / [`tdb2::modify`]
//!   so validation and hooks run as in `TDB2::add` / `TDB2::modify`.
//! * `bool generateDueDates (Task&, std::vector<Datetime>&)` ->
//!   [`generate_due_dates`]. Instead of the whole Context it takes the
//!   `rc.recurrence.limit` value and a footnote sink (narrow inputs).
//! * `std::optional<Datetime> getNextRecurrence (Datetime&, std::string&)` ->
//!   [`get_next_recurrence`], taking the footnote sink for its two
//!   `Context::footnote` warnings. The `throw format (...)` for a
//!   non-positive `<n>m` / `P<n>M` period becomes `Err(TwError::Message)`.
//! * `void updateRecurrenceMask (Task&)` -> [`update_recurrence_mask`].

use crate::context::Context;
use crate::error::{Result, TwError};
use crate::feedback::on_expiration;
use crate::lexer::Lexer;
use crate::shared::datetime::Datetime;
use crate::shared::duration::Duration;
use crate::shared::format::format;
use crate::shared::unicode::unicode_latin_digit;
use crate::task::{c_strtol, Status, Task};
use crate::tdb2;
use crate::util::uuid;

////////////////////////////////////////////////////////////////////////////////
/// C++ `checked_add_datetime`: add a `time_t` delta to a Datetime, returning
/// `None` on integer overflow.
pub fn checked_add_datetime(base: &Datetime, delta: i64) -> Option<Datetime> {
    // Datetime::operator+ takes an integer delta, so check that range
    if i64::from(i32::MAX) < delta {
        return None;
    }

    // Check for time_t overflow in the Datetime. (For a negative base the C++
    // subtraction overflows; saturating keeps the intended "no overflow".)
    if i64::MAX.saturating_sub(base.to_epoch()) < delta {
        return None;
    }
    Some(*base + delta)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `handleRecurrence ()`: scans all tasks, and for any recurring tasks,
/// determines whether any new child tasks need to be generated to fill gaps.
///
/// # Errors
/// Storage, validation and hook errors from [`tdb2::add`] /
/// [`tdb2::modify`], and the invalid-period error of
/// [`get_next_recurrence`].
pub fn handle_recurrence(ctx: &mut Context) -> Result<()> {
    // Recurrence can be disabled.
    // Note: This is currently a workaround for TD-44, TW-1520.
    if !ctx.config.get_boolean("recurrence") {
        return Ok(());
    }

    let tasks: Vec<Task> = ctx.tdb2.pending_tasks()?.to_vec();

    // Look at all tasks and find any recurring ones.
    for mut t in tasks {
        if t.get_status()? != Status::Recurring {
            continue;
        }

        // Generate a list of due dates for this recurring task, regardless of
        // the mask.
        let mut due: Vec<Datetime> = Vec::new();
        let recurrence_limit = ctx.config.get_integer("recurrence.limit");
        if !generate_due_dates(&t, &mut due, recurrence_limit, &mut |msg| ctx.footnote(msg))? {
            // Determine the end date.
            t.set_status(Status::Deleted);
            tdb2::modify(ctx, &mut t)?;
            let msg = on_expiration(ctx, &t);
            ctx.footnote(&msg);
            continue;
        }

        // Get the mask from the parent task.
        let mut mask = t.get("mask");

        // Iterate over the due dates, and check each against the mask.
        let mut changed = false;
        for (i, d) in due.iter().enumerate() {
            if mask.len() <= i {
                changed = true;

                let mut rec = t.clone(); // Clone the parent.
                rec.set_status(Status::Pending); // Change the status.
                rec.set("uuid", &uuid()); // New UUID.
                rec.set("parent", t.get_ref("uuid")); // Remember mom.
                rec.set_as_now("entry"); // New entry date.
                rec.set("due", &d.to_epoch().to_string());

                if t.has("wait") {
                    let old_wait = Datetime::from_epoch(t.get_date("wait"));
                    let old_due = Datetime::from_epoch(t.get_date("due"));
                    match checked_add_datetime(d, old_wait - old_due) {
                        Some(wait) => rec.set("wait", &wait.to_epoch().to_string()),
                        None => rec.remove("wait"),
                    }
                    rec.set_status(Status::Waiting);
                    mask.push('W');
                } else {
                    mask.push('-');
                    rec.set_status(Status::Pending);
                }

                if t.has("scheduled") {
                    let old_scheduled = Datetime::from_epoch(t.get_date("scheduled"));
                    let old_due = Datetime::from_epoch(t.get_date("due"));
                    match checked_add_datetime(d, old_scheduled - old_due) {
                        Some(scheduled) => rec.set("scheduled", &scheduled.to_epoch().to_string()),
                        None => rec.remove("scheduled"),
                    }
                }

                rec.set_int("imask", i as i64);
                rec.remove("mask"); // Remove the mask of the parent.

                // Add the new task to the DB.
                tdb2::add(ctx, &mut rec)?;
            }
        }

        // Only modify the parent if necessary.
        if changed {
            t.set("mask", &mask);
            tdb2::modify(ctx, &mut t)?;

            if ctx.verbose("recur") {
                ctx.footnote(&format(
                    "Creating recurring task instance '{1}'",
                    &[&t.get("description")],
                ));
            }
        }
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `generateDueDates`: determine a start date (due), an optional end date
/// (until), and an increment period (recur). Then generate a set of
/// corresponding dates into `all_due`.
///
/// `recurrence_limit` is `rc.recurrence.limit`; `footnote` receives the
/// warnings of [`get_next_recurrence`].
///
/// Returns `false` if the parent recurring task is depleted.
///
/// # Errors
/// An unparsable `until` (the C++ `Datetime (std::string)` throw) and the
/// invalid-period error of [`get_next_recurrence`].
pub fn generate_due_dates(
    parent: &Task,
    all_due: &mut Vec<Datetime>,
    recurrence_limit: i32,
    footnote: &mut dyn FnMut(&str),
) -> Result<bool> {
    // Determine due date, recur period and until date.
    let due = Datetime::from_epoch(parent.get_date("due"));
    if due.to_epoch() == 0 {
        return Ok(false);
    }

    let recur = parent.get("recur");

    let mut specific_end = false;
    let mut until = Datetime::new();
    if !parent.get_ref("until").is_empty() {
        until = Datetime::from_string(parent.get_ref("until"), "")?;
        specific_end = true;
    }

    let mut recurrence_counter = 0;
    let now = Datetime::new();
    let mut i = due;
    loop {
        all_due.push(i);

        if specific_end && i > until {
            // If i > until, it means there are no more tasks to generate, and if the
            // parent mask contains all + or X, then there never will be another task
            // to generate, and this parent task may be safely reaped.
            let mask = parent.get_ref("mask");
            if mask.len() == all_due.len() && !mask.contains('-') {
                return Ok(false);
            }

            return Ok(true);
        }

        if i > now {
            recurrence_counter += 1;
        }

        if recurrence_counter >= recurrence_limit {
            return Ok(true);
        }
        match get_next_recurrence(&i, &recur, footnote)? {
            Some(next) => i = next,
            None => return Ok(true),
        }
    }
}

/// C++ `std::string::operator[]` on a period: `'\0'` past the end (the C++
/// reads `period[period.length () - 1]` of an empty string, which is
/// undefined; `'\0'` never matches a suffix, as for any non-letter byte).
fn byte_at(period: &str, index: usize) -> u8 {
    period.as_bytes().get(index).copied().unwrap_or(0)
}

/// The last byte of `period`, `'\0'` when empty.
fn last_byte(period: &str) -> u8 {
    period
        .len()
        .checked_sub(1)
        .map_or(0, |i| byte_at(period, i))
}

/// C++ `int increment = strtol (...)`: `long` truncated to `int`.
fn strtol_int(text: &str) -> i32 {
    c_strtol(text) as i32
}

/// The C++ invalid-period message.
fn invalid_period(period: &str, increment: i32) -> String {
    format(
        "Recurrence period '{1}' is equivalent to {2} and hence invalid.",
        &[&period, &increment],
    )
}

/// C++ `while (!Datetime::valid (y, m, d)) --d;`: end-of-month clamping.
fn clamp_day(y: i32, m: i32, mut d: i32) -> i32 {
    while !Datetime::valid_ymd(y, m, d) {
        d -= 1;
    }
    d
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `getNextRecurrence`: determine the next recurrence of the given period.
///
/// If no such date can be calculated, such as with a very large period,
/// returns `None`. The two C++ `Context::footnote` warnings go to `footnote`.
///
/// # Errors
/// `Recurrence period '<p>' is equivalent to <n> and hence invalid.` for a
/// non-positive `<n>m` or `P<n>M` period (the C++ `throw`).
pub fn get_next_recurrence(
    current: &Datetime,
    period: &str,
    footnote: &mut dyn FnMut(&str),
) -> Result<Option<Datetime>> {
    let mut m = current.month();
    let mut d = current.day();
    let mut y = current.year();
    let ho = current.hour();
    let mi = current.minute();
    let se = current.second();

    let first = byte_at(period, 0);
    let last = last_byte(period);

    // Some periods are difficult, because they can be vague.
    if period == "monthly" || period == "P1M" {
        m += 1;
        if m > 12 {
            m -= 12;
            y += 1;
        }

        d = clamp_day(y, m, d);

        return Ok(Some(Datetime::from_ymdhms(y, m, d, ho, mi, se)));
    } else if period == "weekdays" {
        let dow = current.day_of_week();
        let days = if dow == 5 {
            3
        } else if dow == 6 {
            2
        } else {
            1
        };

        return Ok(checked_add_datetime(current, days * 86400));
    } else if unicode_latin_digit(u32::from(first)) && last == b'm' {
        let increment = strtol_int(&period[..period.len() - 1]);

        if increment <= 0 {
            return Err(TwError::Message(invalid_period(period, increment)));
        }

        m += increment;
        while m > 12 {
            m -= 12;
            y += 1;
        }

        d = clamp_day(y, m, d);

        return Ok(Some(Datetime::from_ymdhms(y, m, d, ho, mi, se)));
    } else if first == b'P'
        && last == b'M'
        && period.len() >= 2
        && Lexer::is_all_digits(&period[1..period.len() - 1])
    {
        let increment = strtol_int(&period[1..period.len() - 1]);

        if increment <= 0 {
            return Err(TwError::Message(invalid_period(period, increment)));
        }

        m += increment;
        while m > 12 {
            m -= 12;
            y += 1;
        }

        d = clamp_day(y, m, d);

        // As in C++, this branch drops the time of day.
        return Ok(Some(Datetime::from_ymd(y, m, d)));
    } else if period == "quarterly" || period == "P3M" {
        m += 3;
        if m > 12 {
            m -= 12;
            y += 1;
        }

        d = clamp_day(y, m, d);

        return Ok(Some(Datetime::from_ymdhms(y, m, d, ho, mi, se)));
    } else if unicode_latin_digit(u32::from(first)) && last == b'q' {
        let increment = strtol_int(&period[..period.len() - 1]);

        if increment <= 0 {
            footnote(&invalid_period(period, increment));
            return Ok(None);
        }

        m += 3 * increment;
        while m > 12 {
            m -= 12;
            y += 1;
        }

        d = clamp_day(y, m, d);

        return Ok(Some(Datetime::from_ymdhms(y, m, d, ho, mi, se)));
    } else if period == "semiannual" || period == "P6M" {
        m += 6;
        if m > 12 {
            m -= 12;
            y += 1;
        }

        d = clamp_day(y, m, d);

        return Ok(Some(Datetime::from_ymdhms(y, m, d, ho, mi, se)));
    } else if period == "bimonthly" || period == "P2M" {
        m += 2;
        if m > 12 {
            m -= 12;
            y += 1;
        }

        d = clamp_day(y, m, d);

        return Ok(Some(Datetime::from_ymdhms(y, m, d, ho, mi, se)));
    } else if period == "biannual" || period == "biyearly" || period == "P2Y" {
        y += 2;

        return Ok(Some(Datetime::from_ymdhms(y, m, d, ho, mi, se)));
    } else if period == "annual" || period == "yearly" || period == "P1Y" {
        y += 1;

        // If the due data just happens to be 2/29 in a leap year, then simply
        // incrementing y is going to create an invalid date.
        if m == 2 && d == 29 {
            d = 28;
        }

        return Ok(Some(Datetime::from_ymdhms(y, m, d, ho, mi, se)));
    }

    // Add the period to current, and we're done.
    let mut idx = 0usize;
    let mut p = Duration::new();
    if !p.parse(period, &mut idx) {
        footnote(&format(
            "Warning: The recurrence value '{1}' is not valid.",
            &[&period],
        ));
        return Ok(None);
    }

    Ok(checked_add_datetime(current, p.to_time_t()))
}

/// The mask character for a child's status.
fn mask_char(status: Status) -> char {
    match status {
        Status::Pending => '-',
        Status::Completed => '+',
        Status::Deleted => 'X',
        Status::Waiting => 'W',
        Status::Recurring => '?',
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `updateRecurrenceMask`: when the status of a recurring child task
/// changes, the parent task must update its mask.
///
/// C++ quirk kept: when `imask` lies beyond the parent's mask, the C++ builds
/// the padded mask in a *shadowing* local, so the parent is rewritten with
/// its unchanged mask (and still modified, i.e. hooks run and `modified` is
/// stamped).
///
/// # Errors
/// Storage, validation and hook errors, and an invalid stored status.
pub fn update_recurrence_mask(ctx: &mut Context, task: &Task) -> Result<()> {
    let uuid = task.get("parent");

    if uuid.is_empty() {
        return Ok(());
    }
    let Some(mut parent) = ctx.tdb2.get_by_uuid(&uuid)? else {
        return Ok(());
    };

    // `unsigned int index = strtol (...)`.
    let index = c_strtol(task.get_ref("imask")) as u32 as usize;
    let mut mask = parent.get("mask");
    if mask.len() > index {
        let c = mask_char(task.get_status()?);
        // The mask is ASCII ('-', '+', 'X', 'W', '?'); replace one byte.
        let mut bytes = mask.into_bytes();
        bytes[index] = c as u8;
        mask = String::from_utf8_lossy(&bytes).into_owned();
    } else {
        // The C++ builds "?" * index + status into a shadowing `mask` that is
        // discarded; only the status evaluation (which may throw) remains.
        task.get_status()?;
    }

    parent.set("mask", &mask);
    tdb2::modify(ctx, &mut parent)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `handleUntil ()`: delete expired tasks.
///
/// # Errors
/// Storage, validation and hook errors from [`tdb2::modify`].
pub fn handle_until(ctx: &mut Context) -> Result<()> {
    let now = Datetime::new();
    let tasks: Vec<Task> = ctx.tdb2.pending_tasks()?.to_vec();
    for mut t in tasks {
        // C++ comment: what about expiring template tasks?
        if t.get_status()? == Status::Pending && t.has("until") {
            let until = Datetime::from_epoch(t.get_date("until"));
            if until < now {
                ctx.debug(&format(
                    "handleUntil: recurrence expired until {1} < now {2}",
                    &[&until.to_iso_local_extended(), &now.to_iso_local_extended()],
                ));
                t.set_status(Status::Deleted);
                tdb2::modify(ctx, &mut t)?;
                let msg = on_expiration(ctx, &t);
                ctx.footnote(&msg);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "recur_tests.rs"]
mod tests;
