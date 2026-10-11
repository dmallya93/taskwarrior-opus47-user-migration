//! Unit tests for [`Task::modify`] and [`Task::substitute`]. The `modify`
//! tests build a Context through `Context::initialize` (as `main` does) for
//! a real `task <filter> <command> <modifications>` command line, select the
//! task through `Filter::subset` (which runs `CLI2::prepareFilter`, as
//! `CmdModify` does) and apply the parse tree to it, so the CLI2 tags and
//! attributes are exactly the binary's.

use super::*;
use crate::cli2;
use crate::columns::column::{column_base, Column, ColumnBase};
use crate::context::set_stdout_tty_override;
use crate::filter::Filter;
use crate::test_util::{env_lock, EnvGuard};
use std::sync::MutexGuard;
use tempfile::TempDir;

const UUID_TWO: &str = "aaaaaaaa-0000-4000-8000-000000000002";

/// ID 1 (tagged, with a project and a dependency on ID 2), ID 2, and a
/// completed task addressed by UUID.
const SEED: &[&str] = &[
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z","project":"home","tags":["alpha","beta"],"depends":"aaaaaaaa-0000-4000-8000-000000000002"}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000002","description":"two","status":"pending","entry":"20260101T000000Z","annotations":[{"entry":"20260101T000000Z","description":"note"}]}"#,
    r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000003","description":"three","status":"completed","entry":"20260101T000000Z","end":"20260102T000000Z"}"#,
];

/// Restores the process-wide CLI2 abbreviation length that
/// `Context::initialize` sets.
struct MinimumMatchLength(i32);

impl Drop for MinimumMatchLength {
    fn drop(&mut self) {
        cli2::set_minimum_match_length(self.0);
    }
}

/// Restores the `Task::regex` / `Task::searchCaseSensitive` statics.
struct SearchStatics;

impl Drop for SearchStatics {
    fn drop(&mut self) {
        Task::set_regex(false);
        Task::set_search_case_sensitive(true);
    }
}

/// Fields drop in declaration order: the Context and data directory first,
/// then the statics and environment are restored, the lock goes last.
struct Fixture {
    ctx: Context,
    _dir: TempDir,
    _statics: SearchStatics,
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
    /// Seeds [`SEED`] into a fresh data directory (with `rc` lines in the
    /// taskrc), then initializes a Context for `task <list...>` on it.
    fn with_rc(rc_lines: &str, list: &[&str]) -> Self {
        let lock = env_lock();
        let env = EnvGuard::save(&["HOME", "TASKRC", "TASKDATA", "XDG_CONFIG_HOME"]);
        let mml = MinimumMatchLength(cli2::minimum_match_length());
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let home = format!("{base}/home");
        std::fs::create_dir(&home).unwrap();
        let rc = format!("{base}/taskrc");
        std::fs::write(&rc, rc_lines).unwrap();
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
            _statics: SearchStatics,
            _mml: mml,
            _env: env,
            _lock: lock,
        }
    }

    fn new(list: &[&str]) -> Self {
        Fixture::with_rc("", list)
    }

    /// The single task the command's filter selects (as `CmdModify` gets it).
    fn target(&mut self) -> Task {
        let mut output = Vec::new();
        Filter::new().subset(&mut self.ctx, &mut output).unwrap();
        assert_eq!(output.len(), 1, "filter must select one task");
        output.remove(0)
    }

    /// `target ().modify (mod_type, text_required)`.
    fn modify(&mut self, mod_type: ModType, text_required: bool) -> (Task, Result<()>) {
        let mut task = self.target();
        self.ctx.debug_messages.clear();
        let result = task.modify(&mut self.ctx, mod_type, text_required);
        assert!(
            self.ctx.current_task.is_none(),
            "current task must be restored"
        );
        (task, result)
    }

    fn modified(&mut self, mod_type: ModType) -> Task {
        let (task, result) = self.modify(mod_type, false);
        result.unwrap();
        task
    }

    fn labelled(&self) -> Vec<String> {
        self.ctx
            .debug_messages
            .iter()
            .filter_map(|m| m.strip_prefix(MODIFICATION_LABEL))
            .map(str::to_string)
            .collect()
    }
}

fn message(result: Result<()>) -> String {
    match result {
        Err(TwError::Message(m)) => m,
        other => panic!("expected a message error, got {other:?}"),
    }
}

fn runtime(result: Result<()>) -> String {
    match result {
        Err(TwError::Runtime(m)) => m,
        other => panic!("expected a runtime error, got {other:?}"),
    }
}

////////////////////////////////////////////////////////////////////////////////
// modify: name:value pairs.

#[test]
fn pairs_delegate_to_the_column() {
    let mut f = Fixture::new(&["1", "modify", "pro:work", "priority:H", "due:2030-01-02"]);
    let t = f.modified(ModType::Replace);
    // The abbreviation is canonicalized by CLI2.
    assert_eq!(t.get("project"), "work");
    assert_eq!(t.get("priority"), "H");
    assert_eq!(
        t.get_date("due"),
        Datetime::from_string("2030-01-02", "Y-M-D")
            .unwrap()
            .to_epoch()
    );
    assert_eq!(t.get("description"), "one");
    let labelled = f.labelled();
    assert!(
        labelled.iter().any(|m| m.starts_with("project <-- 'work'")),
        "{labelled:?}"
    );
    assert!(
        labelled.iter().any(|m| m.starts_with("priority <-- 'H'")),
        "{labelled:?}"
    );
}

#[test]
fn pair_values_are_dequoted() {
    let mut f = Fixture::new(&["1", "modify", "project:'a b'"]);
    let t = f.modified(ModType::Replace);
    assert_eq!(t.get("project"), "a b");
}

#[test]
fn pair_dom_references_see_earlier_modifications() {
    // C++ `withCurrentTask (this)` is a live pointer: `scheduled:due` reads
    // the due date set by the preceding argument.
    let mut f = Fixture::new(&["1", "modify", "due:2030-01-02", "scheduled:due"]);
    let t = f.modified(ModType::Replace);
    assert!(t.get_date("due") != 0);
    assert_eq!(t.get_date("scheduled"), t.get_date("due"));
}

#[test]
fn tags_and_depends_pairs() {
    let mut f = Fixture::new(&[
        "2",
        "modify",
        "tags:x,y",
        "depends:aaaaaaaa-0000-4000-8000-000000000003",
    ]);
    let t = f.modified(ModType::Replace);
    assert_eq!(t.get_tags(), ["x", "y"]);
    assert_eq!(
        t.get_dependency_uuids(),
        ["aaaaaaaa-0000-4000-8000-000000000003"]
    );
}

#[test]
fn empty_value_removes_the_attribute() {
    for value in ["project:", "project:''", "project:\"\""] {
        let mut f = Fixture::new(&["1", "modify", value]);
        let (t, result) = f.modify(ModType::Replace, true);
        // The removal counts as a modification, so no text is required.
        result.unwrap();
        assert!(t.has("project"), "{value}");
        assert_eq!(t.get("project"), "", "{value}");
        assert_eq!(f.labelled(), ["project <-- ''"], "{value}");
    }
}

#[test]
fn empty_value_of_absent_attribute_is_not_a_modification() {
    let mut f = Fixture::new(&["1", "modify", "due:"]);
    let (t, result) = f.modify(ModType::Replace, true);
    assert_eq!(message(result), "Additional text must be provided.");
    assert!(!t.has("due"));
    assert_eq!(f.labelled(), ["due <-- ''"]);
}

#[test]
fn empty_value_with_configured_default_is_kept_blank() {
    for (setting, name) in [
        ("default.due", "due"),
        ("default.scheduled", "scheduled"),
        ("default.project", "project"),
    ] {
        let value = if name == "project" {
            "dflt"
        } else {
            "2030-01-01"
        };
        let rc = format!("{setting}={value}\n");
        let arg = format!("{name}:");
        let mut f = Fixture::with_rc(&rc, &["2", "modify", &arg]);
        let (t, result) = f.modify(ModType::Replace, true);
        result.unwrap();
        // Present but blank, so ::validate does not apply the default.
        assert!(t.has(name), "{name}");
        assert_eq!(t.get(name), "", "{name}");
    }
}

#[test]
fn empty_tags_and_depends_remove_all() {
    let mut f = Fixture::new(&["1", "modify", "tags:", "depends:"]);
    let (t, result) = f.modify(ModType::Replace, true);
    assert!(t.get_tags().is_empty());
    assert!(t.get_dependency_uuids().is_empty());
    assert!(!t.has_dependency(UUID_TWO));
    // Neither legacy attribute was present, so no modification is counted.
    assert_eq!(message(result), "Additional text must be provided.");
    assert_eq!(f.labelled(), ["tags <-- ''", "depends <-- ''"]);
}

#[test]
fn non_modifiable_column_is_rejected() {
    let mut f = Fixture::new(&["1", "modify", "urgency:5"]);
    let (_, result) = f.modify(ModType::Replace, false);
    assert_eq!(
        message(result),
        "The 'urgency' attribute does not allow a value of '5'."
    );
}

#[test]
fn unknown_column_is_rejected() {
    let mut f = Fixture::new(&["1", "modify", "project:x"]);
    // A canonical name with no column (C++ `columns[name]` is null).
    f.ctx.columns.remove("project");
    let (t, result) = f.modify(ModType::Replace, false);
    assert_eq!(
        message(result),
        "The 'project' attribute does not allow a value of 'x'."
    );
    assert_eq!(t.get("project"), "home");
}

/// A modifiable column whose type `Task::modify` does not delegate.
struct BogusColumn {
    base: ColumnBase,
}

impl Column for BogusColumn {
    column_base!(base);
}

#[test]
fn unrecognized_column_type_is_rejected() {
    let mut f = Fixture::new(&["1", "modify", "project:x"]);
    let mut base = ColumnBase::new();
    base.name = "project".into();
    base.type_ = "bogus".into();
    base.modifiable = true;
    f.ctx
        .columns
        .insert("project".into(), Rc::new(BogusColumn { base }));
    let (_, result) = f.modify(ModType::Replace, false);
    assert_eq!(
        message(result),
        "Unrecognized column type 'bogus' for column 'project'"
    );
}

#[test]
fn column_errors_propagate_and_restore_current_task() {
    let mut f = Fixture::new(&["1", "modify", "due:nonsense"]);
    let (_, result) = f.modify(ModType::Replace, false);
    assert!(result.is_err());
}

////////////////////////////////////////////////////////////////////////////////
// modify: substitutions, tags, words.

#[test]
fn substitution_argument() {
    let mut f = Fixture::new(&["1", "modify", "/on/ON/"]);
    let t = f.modified(ModType::Replace);
    assert_eq!(t.get("description"), "ONe");
    assert_eq!(f.labelled(), ["substitute /on/ON/"]);
}

#[test]
fn substitution_counts_as_modification_without_a_match() {
    let mut f = Fixture::new(&["1", "modify", "/zzz/y/"]);
    let (t, result) = f.modify(ModType::Replace, true);
    result.unwrap();
    assert_eq!(t.get("description"), "one");
}

#[test]
fn tag_arguments() {
    let mut f = Fixture::new(&["1", "modify", "+gamma", "-alpha"]);
    let (t, result) = f.modify(ModType::Replace, true);
    result.unwrap();
    assert_eq!(t.get_tags(), ["beta", "gamma"]);
    assert_eq!(
        f.labelled(),
        ["tags <-- add 'gamma'", "tags <-- remove 'alpha'"]
    );
}

#[test]
fn special_tag_is_added() {
    let mut f = Fixture::new(&["1", "modify", "+nocolor"]);
    let t = f.modified(ModType::Replace);
    assert!(t.get_tags().iter().any(|t| t == "nocolor"));
}

#[test]
fn reserved_tags_are_rejected() {
    for arg in ["+PENDING", "-OVERDUE"] {
        let mut f = Fixture::new(&["1", "modify", arg]);
        let (_, result) = f.modify(ModType::Replace, false);
        let tag = &arg[1..];
        assert_eq!(
            message(result),
            format(
                "Virtual tags (including '{1}') are reserved and may not be added or removed.",
                &[&tag]
            )
        );
    }
}

#[test]
fn words_replace_the_description() {
    let mut f = Fixture::new(&["1", "modify", "new", "text"]);
    let t = f.modified(ModType::Replace);
    assert_eq!(t.get("description"), "new text");
    assert_eq!(f.labelled(), ["description <-- 'new text'"]);
}

#[test]
fn quoted_words_are_dequoted() {
    let mut f = Fixture::new(&["1", "modify", "'new text'"]);
    let t = f.modified(ModType::Replace);
    assert_eq!(t.get("description"), "new text");
}

#[test]
fn words_prepend() {
    let mut f = Fixture::new(&["1", "prepend", "pre"]);
    let t = f.modified(ModType::Prepend);
    assert_eq!(t.get("description"), "pre one");
    assert_eq!(f.labelled(), ["description <-- 'pre' + description"]);
}

#[test]
fn words_append() {
    let mut f = Fixture::new(&["1", "append", "post", "fix"]);
    let t = f.modified(ModType::Append);
    assert_eq!(t.get("description"), "one post fix");
    assert_eq!(f.labelled(), ["description <-- description + 'post fix'"]);
}

#[test]
fn words_annotate() {
    let mut f = Fixture::new(&["1", "annotate", "a", "note"]);
    let t = f.modified(ModType::Annotate);
    assert_eq!(t.get("description"), "one");
    let annotations = t.get_annotations();
    assert_eq!(annotations.values().collect::<Vec<_>>(), ["a note"]);
    assert_eq!(f.labelled(), ["new annotation <-- 'a note'"]);
}

#[test]
fn mixed_arguments_in_order() {
    let mut f = Fixture::new(&["1", "modify", "+x", "word", "project:p", "more"]);
    let t = f.modified(ModType::Replace);
    assert_eq!(t.get("description"), "word more");
    assert_eq!(t.get("project"), "p");
    let labelled = f.labelled();
    assert_eq!(
        labelled.first().map(String::as_str),
        Some("tags <-- add 'x'")
    );
    assert_eq!(
        labelled.last().map(String::as_str),
        Some("description <-- 'word more'")
    );
}

#[test]
fn text_required() {
    let mut f = Fixture::new(&["1", "modify"]);
    let (t, result) = f.modify(ModType::Replace, true);
    assert_eq!(message(result), "Additional text must be provided.");
    assert_eq!(t.get("description"), "one");

    // Not required: nothing happens.
    drop(f);
    let mut f = Fixture::new(&["1", "modify"]);
    let (t, result) = f.modify(ModType::Replace, false);
    result.unwrap();
    assert_eq!(t.get("description"), "one");
    assert!(f.labelled().is_empty());
}

////////////////////////////////////////////////////////////////////////////////
// modify: the completed/deleted footnote.

const NOTE: &str = "Note: Modified task aaaaaaaa is completed. You may wish to make this task pending with: task aaaaaaaa modify status:pending";

#[test]
fn modifying_a_completed_task_footnotes() {
    let mut f = Fixture::new(&["aaaaaaaa-0000-4000-8000-000000000003", "modify", "new"]);
    let t = f.modified(ModType::Replace);
    assert_eq!(t.get("description"), "new");
    assert!(
        f.ctx.footnotes.iter().any(|n| n == NOTE),
        "{:?}",
        f.ctx.footnotes
    );
}

#[test]
fn status_change_does_not_footnote() {
    let mut f = Fixture::new(&[
        "aaaaaaaa-0000-4000-8000-000000000003",
        "modify",
        "status:pending",
    ]);
    let t = f.modified(ModType::Replace);
    assert_eq!(t.get("status"), "pending");
    assert!(!f
        .ctx
        .footnotes
        .iter()
        .any(|n| n.starts_with("Note: Modified task")));

    // Fixture holds the env lock: release it before taking it again.
    drop(f);
    let mut f = Fixture::new(&["2", "modify", "status:completed"]);
    let t = f.modified(ModType::Replace);
    assert_eq!(t.get("status"), "completed");
    assert!(!f
        .ctx
        .footnotes
        .iter()
        .any(|n| n.starts_with("Note: Modified task")));
}

#[test]
fn bad_status_propagates() {
    let mut f = Fixture::new(&["1", "modify", "x"]);
    let mut t = f.target();
    t.set("status", "bogus");
    assert!(t.modify(&mut f.ctx, ModType::Replace, false).is_err());
    assert!(f.ctx.current_task.is_none());
}

////////////////////////////////////////////////////////////////////////////////
// substitute.

/// A task with `description` and the annotations `annotations` (in order).
fn task(description: &str, annotations: &[&str]) -> Task {
    let mut t = Task::new();
    t.set("description", description);
    for (i, a) in annotations.iter().enumerate() {
        t.set(&format!("annotation_{}", 1000 + i), a);
    }
    t
}

fn annotations(t: &Task) -> Vec<String> {
    t.get_annotations().into_values().collect()
}

/// Runs `body` with `Task::regex` / `Task::searchCaseSensitive` set,
/// restoring the defaults afterwards.
fn with_statics(regex: bool, sensitive: bool, body: impl FnOnce()) {
    let _restore = SearchStatics;
    Task::set_regex(regex);
    Task::set_search_case_sensitive(sensitive);
    body();
}

#[test]
fn literal_first_and_global() {
    with_statics(false, true, || {
        let mut t = task("foo foo foo", &[]);
        t.substitute("foo", "FOO", "").unwrap();
        assert_eq!(t.get("description"), "FOO foo foo");
        t.substitute("foo", "FOO", "g").unwrap();
        assert_eq!(t.get("description"), "FOO FOO FOO");
        t.substitute("FOO", "aaa", "").unwrap();
        t.substitute("FOO", "bbb", "").unwrap();
        t.substitute("bbb", "", "").unwrap();
        assert_eq!(t.get("description"), "aaa  FOO");
    });
}

#[test]
fn literal_case_sensitivity() {
    with_statics(false, true, || {
        let mut t = task("Foo", &[]);
        t.recalc_urgency = false;
        t.substitute("foo", "x", "").unwrap();
        assert_eq!(t.get("description"), "Foo");
        // No change: nothing is written back.
        assert!(!t.recalc_urgency);
    });
    with_statics(false, false, || {
        let mut t = task("Foo", &[]);
        t.substitute("foo", "x", "").unwrap();
        assert_eq!(t.get("description"), "x");
    });
}

#[test]
fn literal_annotations() {
    with_statics(false, true, || {
        // A description match ends a non-global substitution.
        let mut t = task("bar", &["bar bar", "bar"]);
        t.substitute("bar", "X", "").unwrap();
        assert_eq!(
            (t.get("description"), annotations(&t)),
            ("X".into(), vec!["bar bar".into(), "bar".into()])
        );

        // Otherwise the first annotation match.
        let mut t = task("foo", &["bar bar", "bar"]);
        t.recalc_urgency = false;
        t.substitute("bar", "BAR", "").unwrap();
        assert_eq!(annotations(&t), ["BAR bar", "bar"]);
        assert_eq!(t.get("description"), "foo");
        assert!(t.recalc_urgency);

        // Global: the description and every annotation.
        let mut t = task("bar", &["bar bar", "bar"]);
        t.substitute("bar", "BAR", "g").unwrap();
        assert_eq!(
            (t.get("description"), annotations(&t)),
            ("BAR".into(), vec!["BAR BAR".into(), "BAR".into()])
        );
    });
}

#[test]
fn literal_offset_quirks() {
    with_statics(false, true, || {
        // C++ searches the modified text but replaces at pos + skew.
        let mut t = task("aaa", &[]);
        t.substitute("a", "bb", "g").unwrap();
        assert_eq!(t.get("description"), "bbabb");

        // A shrinking global replacement wraps pos + skew below zero:
        // std::string::replace throws std::out_of_range.
        let mut t = task("abcabc", &[]);
        assert_eq!(
            runtime(t.substitute("abc", "", "g")),
            "basic_string::replace: __pos (which is 18446744073709551613) > this->size() (which is 3)"
        );
        assert_eq!(t.get("description"), "abcabc");

        // The same in an annotation.
        let mut t = task("x", &["abcabc"]);
        assert!(matches!(
            t.substitute("abc", "", "g"),
            Err(TwError::Runtime(_))
        ));
    });
}

#[test]
fn literal_infinite_loop_protection() {
    let expected = "Terminated substitution because more than 1000 changes were made - infinite loop protection.";
    with_statics(false, true, || {
        let mut t = task("abc", &[]);
        assert_eq!(message(t.substitute("", "", "g")), expected);
        let mut t = task("", &["abc"]);
        // The description (empty) matches the empty pattern too; the
        // shared counter trips in the description loop already.
        assert_eq!(message(t.substitute("", "", "g")), expected);
    });
}

#[test]
fn regex_first_and_global() {
    with_statics(true, true, || {
        let mut t = task("aaa bbb", &[]);
        t.substitute("b{3}", "BbB", "").unwrap();
        assert_eq!(t.get("description"), "aaa BbB");

        let mut t = task("aaaaBaaaa", &[]);
        t.substitute("aaaa", "c", "g").unwrap();
        assert_eq!(t.get("description"), "cBc");

        let mut t = task("a1b22c333", &[]);
        t.substitute("[0-9]+", "#", "").unwrap();
        assert_eq!(t.get("description"), "a#b22c333");
        t.substitute("[0-9]+", "<>", "g").unwrap();
        assert_eq!(t.get("description"), "a#b<>c<>");
    });
}

#[test]
fn regex_case_sensitivity() {
    with_statics(true, true, || {
        let mut t = task("FOO", &[]);
        t.substitute("foo", "x", "").unwrap();
        assert_eq!(t.get("description"), "FOO");
    });
    with_statics(true, false, || {
        let mut t = task("FOO", &[]);
        t.substitute("foo", "x", "").unwrap();
        assert_eq!(t.get("description"), "x");
    });
}

#[test]
fn regex_annotations() {
    with_statics(true, true, || {
        let mut t = task("foo", &["bar bar bar", "bar"]);
        t.substitute("bar", "BAR", "").unwrap();
        assert_eq!(annotations(&t), ["BAR bar bar", "bar"]);
        // Same-length global replacement: skew stays 0.
        t.substitute("bar", "BAZ", "g").unwrap();
        assert_eq!(annotations(&t), ["BAR BAZ BAZ", "BAZ"]);
        assert_eq!(t.get("description"), "foo");
    });
}

#[test]
fn regex_annotation_index_quirk() {
    with_statics(true, true, || {
        // C++ indexes start[i + skew]: with skew 1 after the first
        // replacement the second uses start[2] (the third match), the third
        // reads past the vector (undefined behaviour in C++, an error here).
        let mut t = task("x", &["a b a c a"]);
        assert!(matches!(
            t.substitute("a", "XY", "g"),
            Err(TwError::Runtime(_))
        ));
        // Nothing was written back.
        assert_eq!(annotations(&t), ["a b a c a"]);
    });
}

#[test]
fn regex_invalid_pattern() {
    with_statics(true, true, || {
        let mut t = task("abc", &[]);
        assert!(matches!(
            t.substitute("(", "x", ""),
            Err(TwError::Runtime(_))
        ));
    });
}

#[test]
fn regex_split_utf8_is_stored_lossily() {
    with_statics(true, true, || {
        // `.` matches single bytes, as with std::regex over std::string.
        let mut t = task("\u{e9}", &[]);
        t.substitute(".", "x", "").unwrap();
        assert_eq!(t.get("description"), "x\u{fffd}");
    });
}
