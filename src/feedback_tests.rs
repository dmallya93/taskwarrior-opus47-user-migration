//! Unit tests for [`super`]: every `feedback.cpp` function, with the
//! verbosity gates, the exact C++ message text, the project completion
//! arithmetic over real TDB2 data, the unblocked report and the backlog
//! footnote. Each test builds a Context through `Context::initialize` (as
//! `main` does) on a temporary `TASKDATA`.

use super::*;
use crate::cli2;
use crate::context::set_stdout_tty_override;
use crate::shared::localtime;
use crate::test_util::{env_lock, EnvGuard};
use std::sync::MutexGuard;
use tempfile::TempDir;

const UUID_BLOCKER: &str = "bbbbbbbb-0000-4000-8000-000000000001";
const UUID_BLOCKED: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const UUID_BLOCKER2: &str = "bbbbbbbb-0000-4000-8000-000000000003";

/// Project `home`: 2 pending (one waiting), 2 completed, 1 deleted.
/// Project `solo`: 1 pending. Project `done`: 1 completed.
/// `blocked` depends on the completed `blocker`; `blocked2` depends on the
/// pending `blocker2`.
const SEED: &[&str] = &[
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"h1","status":"pending","entry":"20260101T000000Z","project":"home"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"h2","status":"pending","entry":"20260101T000000Z","project":"home","wait":"20990101T000000Z"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000003","description":"h3","status":"completed","entry":"20260101T000000Z","end":"20260102T000000Z","project":"home"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000004","description":"h4","status":"completed","entry":"20260101T000000Z","end":"20260102T000000Z","project":"home"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000005","description":"h5","status":"deleted","entry":"20260101T000000Z","end":"20260102T000000Z","project":"home"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000006","description":"s1","status":"pending","entry":"20260101T000000Z","project":"solo"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000007","description":"d1","status":"completed","entry":"20260101T000000Z","end":"20260102T000000Z","project":"done"}"#,
    r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000001","description":"blocker","status":"completed","entry":"20260101T000000Z","end":"20260102T000000Z"}"#,
    r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000002","description":"blocked","status":"pending","entry":"20260101T000000Z","depends":["bbbbbbbb-0000-4000-8000-000000000001"]}"#,
    r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000003","description":"blocker2","status":"pending","entry":"20260101T000000Z"}"#,
    r#"{"uuid":"bbbbbbbb-0000-4000-8000-000000000004","description":"blocked2","status":"pending","entry":"20260101T000000Z","depends":["bbbbbbbb-0000-4000-8000-000000000003"]}"#,
];

/// Restores the process-wide CLI2 abbreviation length that
/// `Context::initialize` sets.
struct MinimumMatchLength(i32);

impl Drop for MinimumMatchLength {
    fn drop(&mut self) {
        cli2::set_minimum_match_length(self.0);
    }
}

/// Restores `TZ` handling after the environment guard put the variable back.
struct TzReset;

impl Drop for TzReset {
    fn drop(&mut self) {
        localtime::tzset();
    }
}

/// Fields drop in declaration order: the Context and data directory first,
/// then the statics and environment are restored, the lock goes last.
struct Fixture {
    ctx: Context,
    _dir: TempDir,
    _mml: MinimumMatchLength,
    _tz: TzReset,
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
    /// Seeds [`SEED`] into a fresh data directory, then initializes a
    /// Context for `task <list...> count` on it (TZ=UTC).
    fn new(list: &[&str]) -> Self {
        let lock = env_lock();
        let env = EnvGuard::save(&["HOME", "TASKRC", "TASKDATA", "XDG_CONFIG_HOME", "TZ"]);
        let tz = TzReset;
        let mml = MinimumMatchLength(cli2::minimum_match_length());
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let home = format!("{base}/home");
        std::fs::create_dir(&home).unwrap();
        let rc = format!("{base}/taskrc");
        std::fs::write(&rc, "").unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("TASKRC", &rc);
        std::env::set_var("TASKDATA", format!("{base}/data"));
        std::env::set_var("TZ", "UTC");
        std::env::remove_var("XDG_CONFIG_HOME");
        localtime::tzset();
        set_stdout_tty_override(Some(false));

        {
            let mut seed = initialized(&["task", "count"]);
            for json in SEED {
                let mut task = Task::from_input(&mut seed, json).unwrap();
                crate::tdb2::add(&mut seed, &mut task).unwrap();
            }
        }

        let mut full = vec!["task"];
        full.extend_from_slice(list);
        full.push("count");
        let ctx = initialized(&full);
        Fixture {
            ctx,
            _dir: dir,
            _mml: mml,
            _tz: tz,
            _env: env,
            _lock: lock,
        }
    }

    fn task(&mut self, uuid: &str) -> Task {
        self.ctx.tdb2.get_by_uuid(uuid).unwrap().unwrap()
    }
}

fn project_task(project: &str) -> Task {
    let mut task = Task::new();
    task.set("description", "x");
    if !project.is_empty() {
        task.set("project", project);
    }
    task
}

fn text(out: Vec<u8>) -> String {
    String::from_utf8(out).unwrap()
}

////////////////////////////////////////////////////////////////////////////////
// renderAttribute

#[test]
fn render_attribute_formats_date_columns() {
    let f = Fixture::new(&["rc.dateformat:Y-M-D"]);
    let (cols, cfg) = (&f.ctx.columns, &f.ctx.config);
    assert_eq!(
        render_attribute(cols, cfg, "due", "1700000000", ""),
        "2023-11-14"
    );
    assert_eq!(
        render_attribute(cols, cfg, "due", "1700000000", "H:N:S"),
        "22:13:20"
    );
    assert_eq!(
        render_attribute(cols, cfg, "entry", "1700000000x", "D.M.Y"),
        "14.11.2023"
    );
}

#[test]
fn render_attribute_passes_other_values_through() {
    let f = Fixture::new(&[]);
    let (cols, cfg) = (&f.ctx.columns, &f.ctx.config);
    // Un-parseable dates (strtoll == 0) and empty values are unchanged.
    assert_eq!(
        render_attribute(cols, cfg, "due", "tomorrow", ""),
        "tomorrow"
    );
    assert_eq!(render_attribute(cols, cfg, "due", "0", ""), "0");
    assert_eq!(render_attribute(cols, cfg, "due", "", "Y"), "");
    // Non-date and unknown columns.
    assert_eq!(
        render_attribute(cols, cfg, "description", "1700000000", ""),
        "1700000000"
    );
    assert_eq!(
        render_attribute(cols, cfg, "nosuchcolumn", "1700000000", ""),
        "1700000000"
    );
}

////////////////////////////////////////////////////////////////////////////////
// feedback_affected (all three overloads)

#[test]
fn feedback_affected_honours_verbosity() {
    let mut f = Fixture::new(&["rc.verbose:affected"]);
    let mut task = project_task("");
    task.id = 12;
    task.set("description", "This is a test");

    let mut out = Vec::new();
    feedback_affected_to(&mut f.ctx, "Plain effect", &mut out);
    feedback_affected_count_to(&mut f.ctx, "Deleted {1} tasks", 3, &mut out);
    feedback_affected_task_to(&mut f.ctx, "Deleting task {1} '{2}'", &task, &mut out);
    task.id = 0;
    task.set("uuid", "0123456789-abcdef");
    feedback_affected_task_to(&mut f.ctx, "Deleting task {1} '{2}'", &task, &mut out);
    assert_eq!(
        text(out),
        "Plain effect\nDeleted 3 tasks\nDeleting task 12 'This is a test'\nDeleting task 01234567 'This is a test'\n"
    );
}

#[test]
fn feedback_affected_silent_without_token() {
    let mut f = Fixture::new(&["rc.verbose:nothing"]);
    let task = project_task("");
    let mut out = Vec::new();
    feedback_affected_to(&mut f.ctx, "Plain effect", &mut out);
    feedback_affected_count_to(&mut f.ctx, "Deleted {1} tasks", 3, &mut out);
    feedback_affected_task_to(&mut f.ctx, "Deleting task {1} '{2}'", &task, &mut out);
    assert_eq!(text(out), "");
}

////////////////////////////////////////////////////////////////////////////////
// feedback_reserved_tags

#[test]
fn reserved_tags_are_rejected() {
    for tag in [
        "ACTIVE",
        "ANNOTATED",
        "BLOCKED",
        "BLOCKING",
        "CHILD",
        "COMPLETED",
        "DELETED",
        "DUE",
        "DUETODAY",
        "INSTANCE",
        "LATEST",
        "MONTH",
        "ORPHAN",
        "OVERDUE",
        "PARENT",
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
    ] {
        match feedback_reserved_tags(tag) {
            Err(TwError::Message(m)) => {
                assert_eq!(m, format!("Virtual tags (including '{tag}') are reserved and may not be added or removed."))
            }
            other => panic!("{tag}: {other:?}"),
        }
    }
    for tag in ["active", "home", "Due", "NEXT", "", "OVERDUE2"] {
        assert!(feedback_reserved_tags(tag).is_ok(), "{tag}");
    }
}

////////////////////////////////////////////////////////////////////////////////
// feedback_special_tags

#[test]
fn special_tags_explained() {
    let mut f = Fixture::new(&["rc.verbose:special"]);
    let mut task = project_task("");
    task.id = 4;
    let mut out = Vec::new();
    for tag in ["nocolor", "nonag", "nocal", "next", "other"] {
        feedback_special_tags_to(&mut f.ctx, &task, tag, &mut out);
    }
    assert_eq!(
        text(out),
        "The 'nocolor' special tag will disable color rules for this task.\n\
         The 'nonag' special tag will prevent nagging when this task is modified.\n\
         The 'nocal' special tag will keep this task off the 'calendar' report.\n\
         The 'next' special tag will boost the urgency of this task so it appears on the 'next' report.\n"
    );

    drop(f);
    let mut f = Fixture::new(&["rc.verbose:affected"]);
    let mut out = Vec::new();
    feedback_special_tags_to(&mut f.ctx, &task, "next", &mut out);
    assert_eq!(text(out), "");
}

////////////////////////////////////////////////////////////////////////////////
// feedback_unblocked

#[test]
fn unblocked_reports_tasks_with_no_pending_dependency_left() {
    let mut f = Fixture::new(&["rc.verbose:affected"]);
    let blocker = f.task(UUID_BLOCKER);
    let blocked = f.task(UUID_BLOCKED);
    let mut out = Vec::new();
    feedback_unblocked_to(&mut f.ctx, &blocker, &mut out).unwrap();
    assert_eq!(text(out), format!("Unblocked {} 'blocked'.\n", blocked.id));

    // blocker2 is still pending, so blocked2 stays blocked.
    let blocker2 = f.task(UUID_BLOCKER2);
    let mut out = Vec::new();
    feedback_unblocked_to(&mut f.ctx, &blocker2, &mut out).unwrap();
    assert_eq!(text(out), "");

    // A task blocking nothing.
    let mut out = Vec::new();
    feedback_unblocked_to(&mut f.ctx, &blocked, &mut out).unwrap();
    assert_eq!(text(out), "");
}

#[test]
fn unblocked_silent_without_token() {
    let mut f = Fixture::new(&["rc.verbose:nothing"]);
    let blocker = f.task(UUID_BLOCKER);
    let mut out = Vec::new();
    feedback_unblocked_to(&mut f.ctx, &blocker, &mut out).unwrap();
    assert_eq!(text(out), "");
}

////////////////////////////////////////////////////////////////////////////////
// feedback_backlog

#[test]
fn backlog_needs_sync_setup_and_token() {
    let mut f = Fixture::new(&["rc.verbose:sync"]);
    feedback_backlog(&mut f.ctx).unwrap();
    assert!(f.ctx.footnotes.is_empty());

    drop(f);
    let mut f = Fixture::new(&["rc.verbose:affected", "rc.sync.encryption_secret:s"]);
    feedback_backlog(&mut f.ctx).unwrap();
    assert!(f.ctx.footnotes.is_empty());

    drop(f);
    let mut f = Fixture::new(&["rc.verbose:sync", "rc.sync.encryption_secret:s"]);
    let count = f.ctx.tdb2.num_local_changes().unwrap();
    assert!(count > 1);
    feedback_backlog(&mut f.ctx).unwrap();
    assert_eq!(
        f.ctx.footnotes,
        [format!("There are {count} local changes.  Sync required.")]
    );
}

////////////////////////////////////////////////////////////////////////////////
// onProjectChange / onExpiration

#[test]
fn project_change_summaries() {
    let mut f = Fixture::new(&[]);
    // home: pending h1 + waiting h2, completed h3 + h4, deleted h5 ignored.
    assert_eq!(
        on_project_change(&mut f.ctx, &project_task("home"), true).unwrap(),
        "The project 'home' has changed.  Project 'home' is 50% complete (2 of 4 tasks remaining)."
    );
    assert_eq!(
        on_project_change(&mut f.ctx, &project_task("home"), false).unwrap(),
        "Project 'home' is 50% complete (2 of 4 tasks remaining)."
    );
    // One pending, none done: the singular form.
    assert_eq!(
        on_project_change(&mut f.ctx, &project_task("solo"), false).unwrap(),
        "Project 'solo' is 0% complete (1 task remaining)."
    );
    // Only done tasks: 100%.
    assert_eq!(
        on_project_change(&mut f.ctx, &project_task("done"), false).unwrap(),
        "Project 'done' is 100% complete (0 of 1 tasks remaining)."
    );
    // A project with no stored tasks: 0%, "0 of 0".
    assert_eq!(
        on_project_change(&mut f.ctx, &project_task("new"), false).unwrap(),
        "Project 'new' is 0% complete (0 of 0 tasks remaining)."
    );
    // No project: empty.
    assert_eq!(
        on_project_change(&mut f.ctx, &project_task(""), true).unwrap(),
        ""
    );
}

#[test]
fn project_change_pair() {
    let mut f = Fixture::new(&[]);
    // Same project: unscoped, once.
    assert_eq!(
        on_project_change_pair(&mut f.ctx, &project_task("solo"), &project_task("solo")).unwrap(),
        "Project 'solo' is 0% complete (1 task remaining)."
    );
    // Different projects: both scoped, newline-separated.
    assert_eq!(
        on_project_change_pair(&mut f.ctx, &project_task("solo"), &project_task("done")).unwrap(),
        "The project 'solo' has changed.  Project 'solo' is 0% complete (1 task remaining).\n\
         The project 'done' has changed.  Project 'done' is 100% complete (0 of 1 tasks remaining)."
    );
    // One side without a project: no separator.
    assert_eq!(
        on_project_change_pair(&mut f.ctx, &project_task(""), &project_task("solo")).unwrap(),
        "The project 'solo' has changed.  Project 'solo' is 0% complete (1 task remaining)."
    );
    assert_eq!(
        on_project_change_pair(&mut f.ctx, &project_task(""), &project_task("")).unwrap(),
        ""
    );
}

#[test]
fn expiration_message() {
    let mut f = Fixture::new(&["rc.verbose:affected"]);
    let mut task = project_task("");
    task.set("description", "gone");
    task.id = 7;
    assert_eq!(
        on_expiration(&mut f.ctx, &task),
        "Task 7 'gone' expired and was deleted."
    );
    task.id = 0;
    task.set("uuid", "abcdef0123-4567");
    assert_eq!(
        on_expiration(&mut f.ctx, &task),
        "Task abcdef01 'gone' expired and was deleted."
    );

    drop(f);
    let mut f = Fixture::new(&["rc.verbose:nothing"]);
    assert_eq!(on_expiration(&mut f.ctx, &task), "");
}
