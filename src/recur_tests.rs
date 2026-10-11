//! Unit tests for `recur.rs`.

use std::sync::MutexGuard;

use tempfile::TempDir;

use super::*;
use crate::test_util::{env_lock, EnvGuard};

/// Runs with `TZ=UTC` under the env lock; restores `TZ` (and calls `tzset`)
/// before the lock is released (fields drop in declaration order).
struct Utc {
    env: Option<EnvGuard>,
    _lock: MutexGuard<'static, ()>,
}

impl Utc {
    fn new() -> Self {
        let lock = env_lock();
        let env = EnvGuard::save(&["TZ"]);
        std::env::set_var("TZ", "UTC");
        crate::shared::localtime::tzset();
        Utc {
            env: Some(env),
            _lock: lock,
        }
    }
}

impl Drop for Utc {
    fn drop(&mut self) {
        drop(self.env.take());
        crate::shared::localtime::tzset();
    }
}

fn dt(y: i32, m: i32, d: i32, ho: i32, mi: i32, se: i32) -> Datetime {
    Datetime::from_ymdhms(y, m, d, ho, mi, se)
}

/// `get_next_recurrence` collecting footnotes.
fn next(current: &Datetime, period: &str) -> (Result<Option<Datetime>>, Vec<String>) {
    let mut notes = Vec::new();
    let r = get_next_recurrence(current, period, &mut |m| notes.push(m.to_string()));
    (r, notes)
}

fn next_ok(current: &Datetime, period: &str) -> Datetime {
    let (r, notes) = next(current, period);
    assert!(notes.is_empty(), "{notes:?}");
    r.unwrap().unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// checked_add_datetime

#[test]
fn checked_add_datetime_overflow() {
    let base = Datetime::from_epoch(1_234_526_400);
    assert_eq!(
        checked_add_datetime(&base, 10).unwrap().to_epoch(),
        1_234_526_410
    );
    assert!(
        checked_add_datetime(&base, 0x1_0000_0000).is_none(),
        "delta > 32bit"
    );
    assert!(
        checked_add_datetime(&base, i64::from(i32::MAX)).is_some(),
        "delta == INT_MAX"
    );
    let max = Datetime::from_epoch(i64::MAX);
    assert!(checked_add_datetime(&max, 1).is_none(), "huge base time");
    assert_eq!(checked_add_datetime(&max, 0).unwrap().to_epoch(), i64::MAX);
    assert_eq!(
        checked_add_datetime(&base, -400).unwrap().to_epoch(),
        1_234_526_000
    );
    let negative = Datetime::from_epoch(-100);
    assert_eq!(checked_add_datetime(&negative, 50).unwrap().to_epoch(), -50);
}

////////////////////////////////////////////////////////////////////////////////
// get_next_recurrence

#[test]
fn monthly_clamps_to_end_of_month() {
    let _tz = Utc::new();
    let jan31 = dt(2023, 1, 31, 10, 20, 30);
    for period in ["monthly", "P1M", "1m"] {
        assert_eq!(
            next_ok(&jan31, period),
            dt(2023, 2, 28, 10, 20, 30),
            "{period}"
        );
    }
    // Leap year.
    assert_eq!(
        next_ok(&dt(2024, 1, 31, 0, 0, 0), "monthly"),
        dt(2024, 2, 29, 0, 0, 0)
    );
    // Year wrap.
    assert_eq!(
        next_ok(&dt(2023, 12, 15, 8, 0, 0), "monthly"),
        dt(2024, 1, 15, 8, 0, 0)
    );
    // Multi-month ordinals, wrapping more than one year.
    assert_eq!(
        next_ok(&dt(2023, 3, 31, 1, 2, 3), "2m"),
        dt(2023, 5, 31, 1, 2, 3)
    );
    assert_eq!(
        next_ok(&dt(2023, 11, 30, 1, 2, 3), "27m"),
        dt(2026, 2, 28, 1, 2, 3)
    );
    assert_eq!(
        next_ok(&dt(2023, 12, 31, 0, 0, 0), "bimonthly"),
        dt(2024, 2, 29, 0, 0, 0)
    );
    assert_eq!(
        next_ok(&dt(2023, 8, 31, 0, 0, 0), "P6M"),
        dt(2024, 2, 29, 0, 0, 0)
    );
    assert_eq!(
        next_ok(&dt(2023, 8, 31, 0, 0, 0), "semiannual"),
        dt(2024, 2, 29, 0, 0, 0)
    );
}

#[test]
fn designated_months_drop_the_time() {
    let _tz = Utc::new();
    assert_eq!(
        next_ok(&dt(2023, 1, 31, 10, 20, 30), "P13M"),
        Datetime::from_ymd(2024, 2, 29)
    );
    let (r, _) = next(&dt(2023, 1, 31, 0, 0, 0), "P0M");
    assert_eq!(
        r.unwrap_err().to_string(),
        "Recurrence period 'P0M' is equivalent to 0 and hence invalid."
    );
}

#[test]
fn quarters() {
    let _tz = Utc::new();
    let nov30 = dt(2023, 11, 30, 12, 0, 0);
    for period in ["quarterly", "1q"] {
        assert_eq!(
            next_ok(&nov30, period),
            dt(2024, 2, 29, 12, 0, 0),
            "{period}"
        );
    }
    // As in C++, "P3M" (and "P2M", "P6M") match the earlier `P<n>M` branch,
    // which drops the time of day.
    assert_eq!(next_ok(&nov30, "P3M"), Datetime::from_ymd(2024, 2, 29));
    assert_eq!(next_ok(&nov30, "P2M"), Datetime::from_ymd(2024, 1, 30));
    assert_eq!(
        next_ok(&dt(2023, 5, 31, 0, 0, 0), "3q"),
        dt(2024, 2, 29, 0, 0, 0)
    );
    assert_eq!(
        next_ok(&dt(2023, 1, 15, 0, 0, 0), "2q"),
        dt(2023, 7, 15, 0, 0, 0)
    );

    // A non-positive quarter count is a footnote, not an error.
    let (r, notes) = next(&nov30, "0q");
    assert!(r.unwrap().is_none());
    assert_eq!(
        notes,
        vec!["Recurrence period '0q' is equivalent to 0 and hence invalid.".to_string()]
    );
}

#[test]
fn zero_months_is_an_error() {
    let _tz = Utc::new();
    let (r, notes) = next(&dt(2023, 1, 1, 0, 0, 0), "0m");
    assert_eq!(
        r.unwrap_err().to_string(),
        "Recurrence period '0m' is equivalent to 0 and hence invalid."
    );
    assert!(notes.is_empty());
}

#[test]
fn years_and_leap_days() {
    let _tz = Utc::new();
    let leap = dt(2024, 2, 29, 9, 0, 0);
    for period in ["annual", "yearly", "P1Y"] {
        assert_eq!(next_ok(&leap, period), dt(2025, 2, 28, 9, 0, 0), "{period}");
    }
    assert_eq!(
        next_ok(&dt(2023, 6, 1, 0, 0, 0), "yearly"),
        dt(2024, 6, 1, 0, 0, 0)
    );
    for period in ["biannual", "biyearly", "P2Y"] {
        assert_eq!(
            next_ok(&dt(2023, 6, 1, 0, 0, 0), period),
            dt(2025, 6, 1, 0, 0, 0),
            "{period}"
        );
    }
    // As in C++, biannual does not clamp: mktime normalizes 2026-02-29 to
    // March 1st.
    assert_eq!(next_ok(&leap, "biannual"), dt(2026, 3, 1, 9, 0, 0));
}

#[test]
fn weekdays_skip_the_weekend() {
    let _tz = Utc::new();
    // 2023-06-02 is a Friday.
    let friday = dt(2023, 6, 2, 9, 0, 0);
    assert_eq!(friday.day_of_week(), 5);
    assert_eq!(next_ok(&friday, "weekdays"), dt(2023, 6, 5, 9, 0, 0));
    assert_eq!(
        next_ok(&dt(2023, 6, 3, 9, 0, 0), "weekdays"),
        dt(2023, 6, 5, 9, 0, 0)
    );
    assert_eq!(
        next_ok(&dt(2023, 6, 4, 9, 0, 0), "weekdays"),
        dt(2023, 6, 5, 9, 0, 0)
    );
    assert_eq!(
        next_ok(&dt(2023, 6, 7, 9, 0, 0), "weekdays"),
        dt(2023, 6, 8, 9, 0, 0)
    );
}

#[test]
fn generic_durations() {
    let _tz = Utc::new();
    let base = dt(2023, 6, 1, 0, 0, 0);
    assert_eq!(next_ok(&base, "daily"), dt(2023, 6, 2, 0, 0, 0));
    assert_eq!(next_ok(&base, "2d"), dt(2023, 6, 3, 0, 0, 0));
    assert_eq!(next_ok(&base, "weekly"), dt(2023, 6, 8, 0, 0, 0));
    // `1y` is not a month/quarter ordinal: it is a Duration.
    let year = Duration::from_string("1y").to_time_t();
    assert_eq!(next_ok(&base, "1y").to_epoch(), base.to_epoch() + year);

    let (r, notes) = next(&base, "bogus");
    assert!(r.unwrap().is_none());
    assert_eq!(
        notes,
        vec!["Warning: The recurrence value 'bogus' is not valid.".to_string()]
    );

    let (r, notes) = next(&base, "");
    assert!(r.unwrap().is_none());
    assert_eq!(
        notes,
        vec!["Warning: The recurrence value '' is not valid.".to_string()]
    );
}

#[test]
fn huge_duration_overflows_to_none() {
    let _tz = Utc::new();
    let (r, notes) = next(&dt(2023, 6, 1, 0, 0, 0), "1000y");
    assert!(notes.is_empty());
    assert!(r.unwrap().is_none());
}

////////////////////////////////////////////////////////////////////////////////
// generate_due_dates

fn recurring(due: i64, recur: &str) -> Task {
    let mut t = Task::new();
    t.set_status(Status::Recurring);
    t.set("due", &due.to_string());
    t.set("recur", recur);
    t
}

#[test]
fn generate_due_dates_without_due_is_depleted() {
    let mut t = Task::new();
    t.set("recur", "daily");
    let mut due = Vec::new();
    assert!(!generate_due_dates(&t, &mut due, 1, &mut |_| {}).unwrap());
    assert!(due.is_empty());
}

#[test]
fn generate_due_dates_fills_past_and_limit() {
    let now = Datetime::new().to_epoch();
    let t = recurring(now - 3 * 86400 + 60, "daily");
    let mut due = Vec::new();
    assert!(generate_due_dates(&t, &mut due, 1, &mut |_| {}).unwrap());
    // 3 past instances, then one future instance (limit 1).
    assert_eq!(due.len(), 4);

    let mut due = Vec::new();
    assert!(generate_due_dates(&t, &mut due, 3, &mut |_| {}).unwrap());
    assert_eq!(due.len(), 6);
}

#[test]
fn generate_due_dates_until() {
    let now = Datetime::new().to_epoch();
    let start = now - 10 * 86400;
    let mut t = recurring(start, "daily");
    t.set("until", &(start + 86400 + 1).to_string());
    let mut due = Vec::new();
    // start, start+1d, start+2d (> until).
    assert!(generate_due_dates(&t, &mut due, 1, &mut |_| {}).unwrap());
    assert_eq!(due.len(), 3);

    // All instances done: depleted.
    t.set("mask", "++X");
    let mut due = Vec::new();
    assert!(!generate_due_dates(&t, &mut due, 1, &mut |_| {}).unwrap());

    // One still pending: not depleted.
    t.set("mask", "+-X");
    let mut due = Vec::new();
    assert!(generate_due_dates(&t, &mut due, 1, &mut |_| {}).unwrap());
}

#[test]
fn generate_due_dates_stops_on_invalid_period() {
    let t = recurring(Datetime::new().to_epoch() - 86400, "bogus");
    let mut notes = Vec::new();
    let mut due = Vec::new();
    assert!(generate_due_dates(&t, &mut due, 1, &mut |m| notes.push(m.to_string())).unwrap());
    assert_eq!(due.len(), 1);
    assert_eq!(
        notes,
        vec!["Warning: The recurrence value 'bogus' is not valid.".to_string()]
    );
}

////////////////////////////////////////////////////////////////////////////////
// Context-based functions.

/// Fields drop in declaration order: the TempDir is removed before the lock
/// is released.
struct Fixture {
    ctx: Context,
    _dir: TempDir,
    _lock: MutexGuard<'static, ()>,
}

fn fixture() -> Fixture {
    // Context::new reads $HOME; serialize with env-mutating tests.
    let lock = env_lock();
    let dir = TempDir::new().unwrap();
    let mut ctx = Context::new();
    ctx.tdb2
        .open_replica_at(dir.path().to_str().unwrap(), true, true)
        .unwrap();
    ctx.config.set("recurrence", "on");
    ctx.config.set("recurrence.limit", "1");
    ctx.config.set("verbose", "affected,recur");
    Fixture {
        ctx,
        _dir: dir,
        _lock: lock,
    }
}

fn all(ctx: &mut Context) -> Vec<Task> {
    ctx.tdb2.all_tasks().unwrap()
}

fn add_parent(ctx: &mut Context, due: i64) -> Task {
    let mut t = recurring(due, "daily");
    t.set("description", "rec");
    tdb2::add(ctx, &mut t).unwrap();
    t
}

#[test]
fn handle_recurrence_creates_children_and_mask() {
    let mut f = fixture();
    let now = Datetime::new().to_epoch();
    let due = now - 2 * 86400 + 60;
    let parent = add_parent(&mut f.ctx, due);

    handle_recurrence(&mut f.ctx).unwrap();

    let tasks = all(&mut f.ctx);
    let stored = tasks
        .iter()
        .find(|t| t.get("uuid") == parent.get("uuid"))
        .unwrap();
    assert_eq!(stored.get("mask"), "---");
    let mut children: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.get("parent") == parent.get("uuid"))
        .collect();
    children.sort_by_key(|t| t.get_int("imask"));
    assert_eq!(children.len(), 3);
    for (i, child) in children.iter().enumerate() {
        assert_eq!(child.get("imask"), i.to_string());
        assert_eq!(child.get("status"), "pending");
        assert_eq!(child.get_date("due"), due + i as i64 * 86400);
        assert!(!child.has("mask"));
        assert_ne!(child.get("uuid"), parent.get("uuid"));
    }
    assert_eq!(
        f.ctx.footnotes,
        vec!["Creating recurring task instance 'rec'".to_string()]
    );

    // A second pass generates nothing new.
    f.ctx.footnotes.clear();
    handle_recurrence(&mut f.ctx).unwrap();
    assert_eq!(all(&mut f.ctx).len(), 4);
    assert!(f.ctx.footnotes.is_empty());
}

#[test]
fn handle_recurrence_shifts_wait_and_scheduled() {
    let mut f = fixture();
    let now = Datetime::new().to_epoch();
    let due = now + 10 * 86400;
    let mut t = recurring(due, "weekly");
    t.set("description", "w");
    t.set("wait", &(due - 86400).to_string());
    t.set("scheduled", &(due - 2 * 86400).to_string());
    tdb2::add(&mut f.ctx, &mut t).unwrap();

    handle_recurrence(&mut f.ctx).unwrap();

    let tasks = all(&mut f.ctx);
    let stored = tasks
        .iter()
        .find(|x| x.get("uuid") == t.get("uuid"))
        .unwrap();
    assert_eq!(stored.get("mask"), "W");
    let child = tasks
        .iter()
        .find(|x| x.get("parent") == t.get("uuid"))
        .unwrap();
    assert_eq!(child.get_date("wait"), due - 86400);
    assert_eq!(child.get_date("scheduled"), due - 2 * 86400);
    assert_eq!(child.get("imask"), "0");
}

#[test]
fn handle_recurrence_disabled() {
    let mut f = fixture();
    add_parent(&mut f.ctx, Datetime::new().to_epoch() - 86400);
    f.ctx.config.set("recurrence", "off");
    handle_recurrence(&mut f.ctx).unwrap();
    assert_eq!(all(&mut f.ctx).len(), 1);
}

#[test]
fn handle_recurrence_reaps_depleted_parent() {
    let mut f = fixture();
    let now = Datetime::new().to_epoch();
    let mut t = recurring(now - 10 * 86400, "daily");
    t.set("description", "old");
    t.set("until", &(now - 9 * 86400 - 60).to_string());
    t.set("mask", "++");
    tdb2::add(&mut f.ctx, &mut t).unwrap();

    handle_recurrence(&mut f.ctx).unwrap();

    let stored = f.ctx.tdb2.get_by_uuid(t.get_ref("uuid")).unwrap().unwrap();
    assert_eq!(stored.get("status"), "deleted");
    assert_eq!(f.ctx.footnotes.len(), 1);
    assert!(
        f.ctx.footnotes[0].ends_with(" 'old' expired and was deleted."),
        "{:?}",
        f.ctx.footnotes
    );
}

#[test]
fn handle_until_deletes_expired_tasks() {
    let mut f = fixture();
    let now = Datetime::new().to_epoch();
    let mut expired = Task::new();
    expired.set("description", "gone");
    expired.set("until", &(now - 60).to_string());
    tdb2::add(&mut f.ctx, &mut expired).unwrap();
    let mut alive = Task::new();
    alive.set("description", "kept");
    alive.set("until", &(now + 3600).to_string());
    tdb2::add(&mut f.ctx, &mut alive).unwrap();

    handle_until(&mut f.ctx).unwrap();

    let gone = f
        .ctx
        .tdb2
        .get_by_uuid(expired.get_ref("uuid"))
        .unwrap()
        .unwrap();
    assert_eq!(gone.get("status"), "deleted");
    let kept = f
        .ctx
        .tdb2
        .get_by_uuid(alive.get_ref("uuid"))
        .unwrap()
        .unwrap();
    assert_eq!(kept.get("status"), "pending");
    assert_eq!(f.ctx.footnotes.len(), 1);
    assert!(
        f.ctx.footnotes[0].ends_with(" 'gone' expired and was deleted."),
        "{:?}",
        f.ctx.footnotes
    );
    assert_eq!(f.ctx.debug_messages.len(), 1);
    assert!(f.ctx.debug_messages[0].starts_with("handleUntil: recurrence expired until "));
}

fn add_child(ctx: &mut Context, parent: &Task, imask: i64) -> Task {
    let mut c = Task::new();
    c.set("description", "child");
    c.set("parent", parent.get_ref("uuid"));
    c.set_int("imask", imask);
    tdb2::add(ctx, &mut c).unwrap();
    c
}

#[test]
fn update_recurrence_mask_sets_status_char() {
    let mut f = fixture();
    let mut parent = add_parent(&mut f.ctx, Datetime::new().to_epoch() + 86400);
    parent.set("mask", "----");
    tdb2::modify(&mut f.ctx, &mut parent).unwrap();

    let mut child = add_child(&mut f.ctx, &parent, 1);
    let expected = [
        (Status::Completed, "-+--"),
        (Status::Deleted, "-X--"),
        (Status::Pending, "----"),
    ];
    for (status, mask) in expected {
        child.set_status(status);
        update_recurrence_mask(&mut f.ctx, &child).unwrap();
        let stored = f
            .ctx
            .tdb2
            .get_by_uuid(parent.get_ref("uuid"))
            .unwrap()
            .unwrap();
        assert_eq!(stored.get("mask"), mask);
    }

    // Waiting (a pending task with a future wait).
    child.set_status(Status::Pending);
    child.set("wait", &(Datetime::new().to_epoch() + 3600).to_string());
    update_recurrence_mask(&mut f.ctx, &child).unwrap();
    let stored = f
        .ctx
        .tdb2
        .get_by_uuid(parent.get_ref("uuid"))
        .unwrap()
        .unwrap();
    assert_eq!(stored.get("mask"), "-W--");
}

#[test]
fn update_recurrence_mask_beyond_mask_keeps_it() {
    let mut f = fixture();
    let mut parent = add_parent(&mut f.ctx, Datetime::new().to_epoch() + 86400);
    parent.set("mask", "--");
    tdb2::modify(&mut f.ctx, &mut parent).unwrap();

    let mut child = add_child(&mut f.ctx, &parent, 5);
    child.set_status(Status::Completed);
    update_recurrence_mask(&mut f.ctx, &child).unwrap();
    // C++ quirk: the padded mask is built in a shadowing local and dropped.
    let stored = f
        .ctx
        .tdb2
        .get_by_uuid(parent.get_ref("uuid"))
        .unwrap()
        .unwrap();
    assert_eq!(stored.get("mask"), "--");
}

#[test]
fn update_recurrence_mask_without_parent_is_noop() {
    let mut f = fixture();
    let mut orphan = Task::new();
    orphan.set("description", "orphan");
    tdb2::add(&mut f.ctx, &mut orphan).unwrap();
    update_recurrence_mask(&mut f.ctx, &orphan).unwrap();

    orphan.set("parent", "00000000-0000-0000-0000-000000000000");
    update_recurrence_mask(&mut f.ctx, &orphan).unwrap();
    assert_eq!(all(&mut f.ctx).len(), 1);
}
