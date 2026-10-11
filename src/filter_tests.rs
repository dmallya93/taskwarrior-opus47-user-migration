//! Unit tests for [`super`]: `Filter::subset` (both overloads) over real
//! TDB2 data with filters on attributes, tags (including virtual tags), IDs,
//! UUIDs and `or`; `pendingOnly`; `safety`/`disableSafety`; the debug
//! messages; and error propagation. Each test builds a Context through
//! `Context::initialize` (as `main` does) on a temporary `TASKDATA`.

use super::*;
use crate::context::set_stdout_tty_override;
use crate::test_util::{env_lock, EnvGuard};
use std::sync::MutexGuard;
use tempfile::TempDir;

const UUID_DONE: &str = "aaaaaaaa-0000-4000-8000-000000000003";
const UUID_ONE: &str = "aaaaaaaa-0000-4000-8000-000000000001";

/// The seeded tasks: three in the working set (IDs 1-3), one completed and
/// one deleted.
const SEED: &[&str] = &[
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z","project":"home","tags":["alpha"]}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"two","status":"pending","entry":"20260101T000000Z","project":"work","priority":"H"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000005","description":"five","status":"pending","entry":"20260101T000000Z","wait":"20990101T000000Z"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000003","description":"three","status":"completed","entry":"20260101T000000Z","end":"20260102T000000Z","project":"home"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000004","description":"four","status":"deleted","entry":"20260101T000000Z","end":"20260102T000000Z","tags":["alpha"]}"#,
];

/// Restores the process-wide CLI2 abbreviation length that
/// `Context::initialize` sets.
struct MinimumMatchLength(i32);

impl Drop for MinimumMatchLength {
    fn drop(&mut self) {
        cli2::set_minimum_match_length(self.0);
    }
}

/// Fields drop in declaration order: the Context and data directory first,
/// then the statics and environment are restored, the lock goes last.
struct Fixture {
    ctx: Context,
    _dir: TempDir,
    _mml: MinimumMatchLength,
    _env: EnvGuard,
    _lock: MutexGuard<'static, ()>,
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn initialized(list: &[&str]) -> Context {
    let mut ctx = Context::new();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let rc = ctx
        .initialize_with(&args(list), &mut out, &mut err)
        .unwrap();
    assert_eq!(rc, 0, "{}", String::from_utf8_lossy(&err));
    ctx
}

impl Fixture {
    /// Seeds [`SEED`] into a fresh data directory, then initializes a
    /// Context for `task <list...>` on it.
    fn new(list: &[&str]) -> Self {
        let lock = env_lock();
        let env = EnvGuard::save(&["HOME", "TASKRC", "TASKDATA", "XDG_CONFIG_HOME"]);
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
        std::env::remove_var("XDG_CONFIG_HOME");
        set_stdout_tty_override(Some(false));

        {
            let mut seed = initialized(&["task", "add", "seed"]);
            for json in SEED {
                let mut task = Task::from_input(&mut seed, json).unwrap();
                crate::tdb2::add(&mut seed, &mut task).unwrap();
            }
        }

        let mut full = vec!["task"];
        full.extend_from_slice(list);
        let ctx = initialized(&full);
        Fixture {
            ctx,
            _dir: dir,
            _mml: mml,
            _env: env,
            _lock: lock,
        }
    }

    /// `Filter::subset (output)`, returning the sorted descriptions.
    fn subset(&mut self) -> Vec<String> {
        let mut output = Vec::new();
        Filter::new().subset(&mut self.ctx, &mut output).unwrap();
        descriptions(&output)
    }

    fn debug_with(&self, prefix: &str) -> Vec<String> {
        self.ctx
            .debug_messages
            .iter()
            .filter(|m| m.starts_with(prefix))
            .cloned()
            .collect()
    }
}

fn descriptions(tasks: &[Task]) -> Vec<String> {
    let mut d: Vec<String> = tasks.iter().map(|t| t.get("description")).collect();
    d.sort();
    d
}

fn sorted(list: &[&str]) -> Vec<String> {
    let mut d = args(list);
    d.sort();
    d
}

#[test]
fn default_is_safe_and_empty() {
    let f = Filter::new();
    assert_eq!((f._start_count, f._end_count, f._safety), (0, 0, true));
    let mut f = f;
    f.disable_safety();
    assert!(!f._safety);
}

#[test]
fn no_filter_returns_all_tasks() {
    let mut f = Fixture::new(&["count"]);
    let mut filter = Filter::new();
    let mut output = vec![];
    filter.subset(&mut f.ctx, &mut output).unwrap();
    assert_eq!(
        descriptions(&output),
        sorted(&["one", "two", "five", "three", "four"])
    );
    // C++ never sets _startCount on the no-filter path.
    assert_eq!((filter._start_count, filter._end_count), (0, 5));
    assert!(!filter.has_filter(&f.ctx));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 0 tasks --> 5 tasks [all tasks]"]
    );
    assert!(f.ctx.time_filter_us >= 0);
}

#[test]
fn attribute_filter_scans_completed_tasks() {
    let mut f = Fixture::new(&["project:home", "count"]);
    assert_eq!(f.subset(), sorted(&["one", "three"]));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 5 tasks --> 2 tasks [all tasks]"]
    );
}

#[test]
fn tag_filters() {
    let mut f = Fixture::new(&["+alpha", "count"]);
    assert_eq!(f.subset(), sorted(&["one", "four"]));

    drop(f);
    let mut f = Fixture::new(&["-alpha", "project:home", "count"]);
    assert_eq!(f.subset(), ["three"]);
}

#[test]
fn virtual_tags_see_the_context() {
    let mut f = Fixture::new(&["+COMPLETED", "count"]);
    assert_eq!(f.subset(), ["three"]);
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 5 tasks --> 1 tasks [all tasks]"]
    );

    drop(f);
    let mut f = Fixture::new(&["+DELETED", "count"]);
    assert_eq!(f.subset(), ["four"]);

    // +PENDING on the original command line: pending only.
    drop(f);
    let mut f = Fixture::new(&["+PENDING", "count"]);
    assert_eq!(f.subset(), sorted(&["one", "two"]));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 3 tasks --> 2 tasks [pending only]"]
    );

    drop(f);
    let mut f = Fixture::new(&["+WAITING", "count"]);
    assert_eq!(f.subset(), ["five"]);

    // UDA/priority-based virtual tags read the Context's column registry.
    drop(f);
    let mut f = Fixture::new(&["+PRIORITY", "count"]);
    assert_eq!(f.subset(), ["two"]);
}

#[test]
fn id_filters_are_pending_only() {
    let mut f = Fixture::new(&["1", "count"]);
    assert_eq!(f.subset(), ["one"]);
    assert!(Filter::new().pending_only(&f.ctx));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 3 tasks --> 1 tasks [pending only]"]
    );

    drop(f);
    let mut f = Fixture::new(&["1-2", "count"]);
    assert_eq!(f.subset(), sorted(&["one", "two"]));
}

#[test]
fn uuid_filters_scan_all_tasks() {
    let mut f = Fixture::new(&[UUID_DONE, "count"]);
    assert_eq!(f.subset(), ["three"]);
    assert!(!Filter::new().pending_only(&f.ctx));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 5 tasks --> 1 tasks [all tasks]"]
    );

    drop(f);
    let mut f = Fixture::new(&[UUID_ONE, "count"]);
    assert_eq!(f.subset(), ["one"]);
}

#[test]
fn or_filters_scan_all_tasks() {
    let mut f = Fixture::new(&["project:home", "or", "project:work", "count"]);
    assert_eq!(f.subset(), sorted(&["one", "two", "three"]));
    assert!(!Filter::new().pending_only(&f.ctx));

    // An ID with 'or' is not pending only either.
    drop(f);
    let mut f = Fixture::new(&["1", "or", "project:home", "count"]);
    assert_eq!(f.subset(), sorted(&["one", "three"]));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 5 tasks --> 2 tasks [all tasks]"]
    );

    drop(f);
    let mut f = Fixture::new(&["status:pending", "xor", "project:home", "count"]);
    assert_eq!(f.subset(), sorted(&["two", "three"]));
    assert!(!Filter::new().pending_only(&f.ctx));
}

#[test]
fn status_filters() {
    let mut f = Fixture::new(&["status:pending", "count"]);
    // 'five' has a future wait date, so its status is 'waiting'.
    assert_eq!(f.subset(), sorted(&["one", "two"]));
    assert!(Filter::new().pending_only(&f.ctx));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 3 tasks --> 2 tasks [pending only]"]
    );

    drop(f);
    let mut f = Fixture::new(&["status:completed", "count"]);
    assert_eq!(f.subset(), ["three"]);
    assert!(!Filter::new().pending_only(&f.ctx));

    drop(f);
    let mut f = Fixture::new(&["status:waiting", "count"]);
    assert!(
        !Filter::new().pending_only(&f.ctx),
        "still one undesugared pair before prepareFilter"
    );
    assert_eq!(f.subset(), ["five"]);
    assert!(Filter::new().pending_only(&f.ctx));

    drop(f);
    let mut f = Fixture::new(&["status:recurring", "count"]);
    let _ = f.subset();
    assert!(Filter::new().pending_only(&f.ctx));
}

#[test]
fn pending_only_rules() {
    // Before prepareFilter there are no ID ranges and no FILTER args.
    let f = Fixture::new(&["1", "count"]);
    assert!(!Filter::new().pending_only(&f.ctx));

    // An attribute filter without status, ID or +PENDING/+ACTIVE.
    drop(f);
    let mut f = Fixture::new(&["project:home", "count"]);
    let _ = f.subset();
    assert!(!Filter::new().pending_only(&f.ctx));

    // +ACTIVE comes from the original arguments.
    drop(f);
    let mut f = Fixture::new(&["+ACTIVE", "count"]);
    assert!(
        Filter::new().pending_only(&f.ctx),
        "read from _original_args"
    );
    assert_eq!(f.subset(), Vec::<String>::new());
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 3 tasks --> 0 tasks [pending only]"]
    );

    // When GC is off, there are no shortcuts.
    drop(f);
    let mut f = Fixture::new(&["rc.gc=0", "1", "count"]);
    assert_eq!(f.subset(), ["one"]);
    assert!(!Filter::new().pending_only(&f.ctx));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 5 tasks --> 1 tasks [all tasks]"]
    );
}

#[test]
fn subset_of_input_list() {
    let mut f = Fixture::new(&["project:home", "count"]);
    let input = f.ctx.tdb2.all_tasks().unwrap();
    let mut filter = Filter::new();

    // Matches are appended to whatever output already holds.
    let mut output = vec![input[0].clone()];
    let first = input[0].get("description");
    filter.subset_of(&mut f.ctx, &input, &mut output).unwrap();
    let mut expected = vec![first, "one".to_string(), "three".to_string()];
    expected.sort();
    assert_eq!(descriptions(&output), expected);
    assert_eq!((filter._start_count, filter._end_count), (5, 3));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 5 tasks --> 3 tasks [list subset]"]
    );
}

#[test]
fn subset_of_without_filter_copies_input() {
    let mut f = Fixture::new(&["modify"]);
    let input = f.ctx.tdb2.all_tasks().unwrap();
    let mut filter = Filter::new();
    let mut output = vec![input[0].clone(), input[1].clone()];
    // No safety check on the list overload, even for a write command.
    filter
        .subset_of(&mut f.ctx, &input[1..3], &mut output)
        .unwrap();
    assert_eq!(descriptions(&output), descriptions(&input[1..3]));
    assert_eq!(
        f.debug_with("Filtered "),
        ["Filtered 2 tasks --> 2 tasks [list subset]"]
    );
}

#[test]
fn safety_refuses_unfiltered_write_commands() {
    // rc.allow.empty.filter off.
    let mut f = Fixture::new(&["rc.allow.empty.filter=0", "modify"]);
    let mut output = Vec::new();
    let err = Filter::new().subset(&mut f.ctx, &mut output).unwrap_err();
    assert_eq!(
        err.to_string(),
        "You did not specify a filter, and with the 'allow.empty.filter' value, no action is taken."
    );
    assert!(f.debug_with("Filtered ").is_empty());

    // Allowed, but no confirmation: prevented.
    drop(f);
    let mut f = Fixture::new(&["rc.confirmation=0", "modify"]);
    let err = Filter::new().safety(&f.ctx).unwrap_err();
    assert_eq!(err.to_string(), "Command prevented from running.");
    let err = Filter::new().subset(&mut f.ctx, &mut output).unwrap_err();
    assert_eq!(err.to_string(), "Command prevented from running.");

    // disableSafety skips the check.
    let mut filter = Filter::new();
    filter.disable_safety();
    filter.safety(&f.ctx).unwrap();
    filter.subset(&mut f.ctx, &mut output).unwrap();
    assert_eq!(output.len(), 5);
}

#[test]
fn safety_allows_read_only_or_filtered_commands() {
    let f = Fixture::new(&["rc.allow.empty.filter=0", "count"]);
    Filter::new().safety(&f.ctx).unwrap();

    drop(f);
    let mut f = Fixture::new(&["rc.allow.empty.filter=0", "2", "modify"]);
    assert_eq!(f.subset(), ["two"]);
    Filter::new().safety(&f.ctx).unwrap();
    assert!(Filter::new().has_filter(&f.ctx));
}

#[test]
fn eval_debug_output_with_debug_parser_3() {
    let mut f = Fixture::new(&["rc.debug.parser=3", "project:home", "count"]);
    let _ = f.subset();
    let prefix = "\x1b[1;37;42mFILTER\x1b[0m ";
    assert_eq!(f.debug_with(&format!("{prefix}Infix        ")).len(), 1);
    assert_eq!(f.debug_with(&format!("{prefix}Infix parsed")).len(), 1);
    assert_eq!(f.debug_with(&format!("{prefix}Postfix")).len(), 1);
    // As in C++, eval.debug (false) only comes after the loops, so every
    // per-task evaluation (5 tasks) logs as well.
    assert!(
        f.debug_with("Eval identifier source 'project'").len() >= 5,
        "{:?}",
        f.ctx.debug_messages
    );

    drop(f);
    let mut f = Fixture::new(&["rc.debug.parser=2", "project:home", "count"]);
    let _ = f.subset();
    assert!(f.debug_with(prefix).is_empty());
}

#[test]
fn errors_propagate() {
    // An Eval error (here, a DOM reference through TDB2 to a missing task
    // is fine, but a malformed expression is not).
    let mut f = Fixture::new(&["(", "project:home", "count"]);
    let mut output = Vec::new();
    assert!(Filter::new().subset(&mut f.ctx, &mut output).is_err());

    drop(f);
    let mut f = Fixture::new(&["(", "project:home", "count"]);
    let input = f.ctx.tdb2.all_tasks().unwrap();
    assert!(Filter::new()
        .subset_of(&mut f.ctx, &input, &mut output)
        .is_err());
}

#[test]
fn dom_references_see_the_current_task() {
    let mut f = Fixture::new(&["description.startswith:t", "count"]);
    assert_eq!(f.subset(), sorted(&["two", "three"]));

    drop(f);
    let mut f = Fixture::new(&["id", "==", "2", "count"]);
    assert_eq!(f.subset(), ["two"]);

    // A reference to another task by ID loads it through TDB2.
    drop(f);
    let mut f = Fixture::new(&["project", "==", "1.project", "count"]);
    assert_eq!(f.subset(), sorted(&["one", "three"]));
}
