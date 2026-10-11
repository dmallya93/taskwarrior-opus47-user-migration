//! Unit tests for [`super`]: the complete port of Taskwarrior
//! `test/dom_test.cpp` (12 assertions, assertion-count checked through
//! [`UnitTest`]; the `t.diag (dom.dump ())` output is checked separately),
//! plus direct tests of both `getDOM` overloads (`rc.*`, `tw.*`,
//! `context.*`, `system.*`, and task-relative references including
//! `<id>.`/`<uuid>.` loading through TDB2), the `DOM` tree edge cases, and
//! [`ContextResolver`] as Eval's `domSource`.
//!
//! Nothing in `dom_test.cpp` is excluded.

use super::*;
use crate::eval::Eval;
use crate::test_util::{env_lock, EnvGuard};
use crate::variant::Type as VariantType;
use std::fmt::Debug;
use std::sync::MutexGuard;
use tempfile::TempDir;

/// The C++ `UnitTest` (test/test.cpp), recording instead of printing.
struct UnitTest {
    counter: usize,
    failures: Vec<String>,
    diags: Vec<String>,
}

impl UnitTest {
    fn new() -> Self {
        UnitTest {
            counter: 0,
            failures: Vec::new(),
            diags: Vec::new(),
        }
    }

    /// C++ `UnitTest::ok`.
    fn ok(&mut self, expression: bool, name: &str) {
        self.counter += 1;
        if !expression {
            self.failures
                .push(format!("not ok {} - {}", self.counter, name));
        }
    }

    /// C++ `UnitTest::notok`.
    fn notok(&mut self, expression: bool, name: &str) {
        self.ok(!expression, name);
    }

    /// C++ `UnitTest::is`.
    fn is<A: PartialEq<E> + Debug, E: Debug>(&mut self, actual: A, expected: E, name: &str) {
        self.counter += 1;
        if actual != expected {
            self.failures.push(format!(
                "not ok {} - {}\n#   expected: {:?}\n#        got: {:?}",
                self.counter, name, expected, actual
            ));
        }
    }

    /// C++ `UnitTest::diag` (recorded, not counted).
    fn diag(&mut self, text: &str) {
        self.diags.push(text.to_string());
    }

    /// Asserts that every assertion passed and that `planned` ran.
    fn check(&self, planned: usize) {
        assert!(
            self.failures.is_empty(),
            "{} of {} assertions failed:\n{}",
            self.failures.len(),
            self.counter,
            self.failures.join("\n")
        );
        assert_eq!(self.counter, planned, "number of assertions executed");
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ anonymous-namespace `providerString`.
fn provider_string(path: &str, var: &mut Variant) -> bool {
    if path == "name" {
        *var = Variant::from_string("value");
        return true;
    } else if path == "name.next" {
        *var = Variant::from_string("value.next");
        return true;
    } else if path == "foo" {
        *var = Variant::from_string("bar");
        return true;
    } else if path == "name.size" {
        *var = Variant::from_int(6);
        return true;
    }

    false
}

/// Port of `test/dom_test.cpp`.
#[test]
fn dom_test_cpp() {
    let mut t = UnitTest::new();

    let mut dom = Dom::new();
    t.is(dom.count(), 0, "DOM empty count is zero");

    dom.add_source("name", provider_string);
    dom.add_source("name.next", provider_string);
    dom.add_source("name.size", provider_string);
    dom.add_source("foo", provider_string);
    let dump = dom.dump();
    t.diag(&dump);
    t.is(dom.count(), 4, "DOM now contains 4 nodes");

    t.ok(dom.valid("name"), "DOM 'name' valid");
    t.ok(dom.valid("name.next"), "DOM 'name.next' valid");
    t.ok(dom.valid("name.size"), "DOM 'name.size' valid");
    t.ok(dom.valid("foo"), "DOM 'foo' valid");
    t.notok(dom.valid("missing"), "DOM 'missing' not valid");

    let v = dom.get("name");
    t.is(v.get_string(), "value", "DOM get 'name' --> 'value'");

    let v = dom.get("name.next");
    t.is(
        v.get_string(),
        "value.next",
        "DOM get 'name.next' --> 'value.next'",
    );

    let v = dom.get("name.size");
    t.is(v.get_integer(), 6, "DOM get 'name.size' --> 6");

    let v = dom.get("foo");
    t.is(v.get_string(), "bar", "DOM get 'name.size' --> 6");

    let v = dom.get("missing");
    t.is(v.get_string(), "", "DOM get 'missing' --> ''");

    t.check(12);
    assert_eq!(t.diags.len(), 1);
}

/// The `t.diag (dom.dump ())` text: the C++ format with the provider
/// addresses as `0x<hex>`.
#[test]
fn dump_format() {
    assert_eq!(Dom::new().dump(), "");

    let mut dom = Dom::new();
    dom.add_source("name", provider_string);
    dom.add_source("name.next", provider_string);
    dom.add_source("name.size", provider_string);
    dom.add_source("foo", provider_string);

    let address = format!("0x{:x}", provider_string as Provider as usize);
    let expected = format!(
        "DOM::Node (4 nodes)\n  \x1b[31mUnknown\x1b[0m\n    \x1b[31mname\x1b[0m {address}\n      \
         \x1b[31mnext\x1b[0m {address}\n      \x1b[31msize\x1b[0m {address}\n    \x1b[31mfoo\x1b[0m {address}\n"
    );
    assert_eq!(dom.dump(), expected);
}

/// `DOM::Node::find` stops at the deepest matching prefix, so an unknown
/// sub-reference of a known node is valid and calls that node's provider
/// with the full reference; intermediate nodes without a provider do not
/// count.
#[test]
fn tree_edge_cases() {
    let empty = Dom::new();
    assert!(!empty.valid("name"));
    assert_eq!(empty.get("name").get_string(), "");
    assert_eq!(empty.get("name").type_(), VariantType::String);
    assert_eq!(Dom::decompose_reference("a.b..c"), vec!["a", "b", "", "c"]);

    let mut dom = Dom::new();
    dom.add_source("a.b.c", provider_string);
    assert_eq!(dom.count(), 1, "only the leaf has a provider");
    assert!(dom.valid("a"), "a prefix node is found");
    assert!(dom.valid("a.x"), "partial match finds 'a'");
    assert!(!dom.valid(""), "an empty reference is never valid");
    assert!(!dom.valid("x.a"));
    // 'a' has no provider: the default "" is returned.
    assert_eq!(dom.get("a").get_string(), "");

    dom.add_source("name", provider_string);
    // 'name.missing' finds 'name', whose provider rejects the full reference.
    assert!(dom.valid("name.missing"));
    assert_eq!(dom.get("name.missing").get_string(), "");

    // A provider returning false keeps its assignment (C++ passes `v` by
    // reference and returns it).
    fn assigns_then_fails(_: &str, v: &mut Variant) -> bool {
        *v = Variant::from_int(42);
        false
    }
    dom.add_source("odd", assigns_then_fails);
    assert_eq!(dom.get("odd").get_integer(), 42);

    // Re-adding replaces the provider without adding a node.
    dom.add_source("a.b.c", assigns_then_fails);
    assert_eq!(dom.count(), 3);
    assert_eq!(dom.get("a.b.c").get_integer(), 42);

    let node = Node::default();
    assert_eq!(node.name, "Unknown");
    assert!(!node.valid("x"));
}

////////////////////////////////////////////////////////////////////////////////
/// A Context like `Context::initialize` builds it for DOM lookups: the
/// column registry (with three UDAs) registered as "attribute" entities,
/// and `args` as the CLI2 original arguments. Fields drop in declaration
/// order: Context and data directory first, then the environment is
/// restored, and the lock is released last.
struct Fixture {
    ctx: Context,
    dir: TempDir,
    _statics: Statics,
    _env: EnvGuard,
    _lock: MutexGuard<'static, ()>,
}

/// The thread-local statics `Context::staticInitialization` sets for DOM
/// lookups (attribute types for Task/Lexer, and no standalone date, time or
/// seconds lexing), restored when dropped.
struct Statics {
    task_attributes: BTreeMap<String, String>,
    lexer_attributes: BTreeMap<String, String>,
    standalone: (bool, bool, bool),
}

impl Statics {
    fn set(ctx: &Context) -> Self {
        let saved = Statics {
            task_attributes: Task::attributes(),
            lexer_attributes: Lexer::attributes(),
            standalone: (
                Datetime::standalone_date_enabled(),
                Datetime::standalone_time_enabled(),
                Duration::standalone_seconds_enabled(),
            ),
        };
        Datetime::set_standalone_date_enabled(false);
        Datetime::set_standalone_time_enabled(false);
        Duration::set_standalone_seconds_enabled(false);
        for (name, col) in &ctx.columns {
            Task::insert_attribute(name, col.type_());
            Lexer::insert_attribute(name, col.type_());
        }
        saved
    }
}

impl Drop for Statics {
    fn drop(&mut self) {
        Task::set_attributes(std::mem::take(&mut self.task_attributes));
        Lexer::set_attributes(std::mem::take(&mut self.lexer_attributes));
        Datetime::set_standalone_date_enabled(self.standalone.0);
        Datetime::set_standalone_time_enabled(self.standalone.1);
        Duration::set_standalone_seconds_enabled(self.standalone.2);
    }
}

impl Fixture {
    fn new(args: &[&str]) -> Self {
        // Context::new reads $HOME, and the dates below read TZ.
        let lock = env_lock();
        let env = EnvGuard::save(&["TASKDATA", "TASKRC"]);
        std::env::remove_var("TASKDATA");
        std::env::remove_var("TASKRC");

        let mut ctx = Context::new();
        ctx.config.set("foo", "bar");
        ctx.config.set("uda.size.type", "numeric");
        ctx.config.set("uda.note.type", "string");
        ctx.config.set("uda.span.type", "duration");
        ctx.config.set("uda.when.type", "date");
        crate::columns::column::factory_all(&mut ctx.columns, &ctx.config).unwrap();
        let names: Vec<String> = ctx.columns.keys().cloned().collect();
        for name in names {
            ctx.cli2.entity("attribute", &name);
        }
        let statics = Statics::set(&ctx);
        for arg in args {
            ctx.cli2.add(arg);
        }
        // `CLI2::analyze` copies the original args into `_args` (getBinary).
        ctx.cli2.entity("cmd", "calc");
        if !args.is_empty() {
            crate::cli2::with_cli2(&mut ctx, |cli2, ctx| cli2.analyze(ctx)).unwrap();
        }

        Fixture {
            ctx,
            dir: TempDir::new().unwrap(),
            _statics: statics,
            _env: env,
            _lock: lock,
        }
    }

    fn open(&mut self) {
        let location = self.dir.path().to_str().unwrap().to_string();
        self.ctx
            .tdb2
            .open_replica_at(&location, true, true)
            .unwrap();
    }

    fn add(&mut self, json: &str) -> Task {
        let mut task = Task::from_input(&mut self.ctx, json).unwrap();
        crate::tdb2::add(&mut self.ctx, &mut task).unwrap();
        task
    }

    fn get(&mut self, name: &str) -> Option<Variant> {
        get_dom(&mut self.ctx, name).unwrap()
    }

    fn get_task(&mut self, name: &str, task: Option<&Task>) -> Option<Variant> {
        get_dom_task(&mut self.ctx, name, task).unwrap()
    }
}

fn string(v: Option<Variant>) -> Option<(VariantType, String)> {
    v.map(|v| (v.type_(), v.to_string()))
}

fn s(text: &str) -> Option<(VariantType, String)> {
    Some((VariantType::String, text.to_string()))
}

fn int(value: i64) -> Option<(VariantType, i64)> {
    Some((VariantType::Integer, value))
}

fn integer(v: Option<Variant>) -> Option<(VariantType, i64)> {
    v.map(|v| (v.type_(), v.get_integer()))
}

#[test]
fn context_free_rc_references() {
    let mut f = Fixture::new(&[]);
    assert_eq!(string(f.get("rc.foo")), s("bar"));
    assert_eq!(string(f.get("rc.uda.size.type")), s("numeric"));
    assert!(f.get("rc.missing").is_none());
    // Exactly "rc." is not a reference (len > 3).
    assert!(f.get("rc.").is_none());
    assert!(f.get("").is_none());
    assert!(f.get("foo").is_none());
    assert!(f.get("rc").is_none());
}

#[test]
fn context_free_tw_context_system_references() {
    let mut f = Fixture::new(&["/usr/bin/task", "calc", "1", "+", "rc.foo"]);

    assert_eq!(string(f.get("tw.version")), s(VERSION));
    assert_eq!(string(f.get("tw.version")), s("3.5.0"));
    assert_eq!(string(f.get("system.version")), s("3.5.0"));
    assert_eq!(string(f.get("system.os")), s(&os_name()));
    if cfg!(target_os = "linux") {
        assert_eq!(string(f.get("system.os")), s("Linux"));
    }

    assert_eq!(string(f.get("tw.program")), s("/usr/bin/task"));
    assert_eq!(string(f.get("context.program")), s("/usr/bin/task"));
    assert_eq!(string(f.get("tw.args")), s("/usr/bin/task calc 1 + rc.foo"));
    assert_eq!(
        string(f.get("context.args")),
        s("/usr/bin/task calc 1 + rc.foo")
    );

    // terminal_width/height win when set.
    f.ctx.terminal_width = 123;
    f.ctx.terminal_height = 45;
    assert_eq!(integer(f.get("tw.width")), int(123));
    assert_eq!(integer(f.get("context.width")), int(123));
    assert_eq!(integer(f.get("tw.height")), int(45));
    assert_eq!(integer(f.get("context.height")), int(45));

    // Otherwise getWidth/getHeight: defaultwidth/defaultheight without
    // detection, and 0 meaning 2^16.
    f.ctx.terminal_width = 0;
    f.ctx.terminal_height = 0;
    f.ctx.config.set("detection", "off");
    f.ctx.config.set("defaultwidth", "80");
    f.ctx.config.set("defaultheight", "24");
    assert_eq!(integer(f.get("tw.width")), int(80));
    assert_eq!(integer(f.get("tw.height")), int(24));
    f.ctx.config.set("defaultwidth", "0");
    f.ctx.config.set("defaultheight", "0");
    assert_eq!(integer(f.get("context.width")), int(65536));
    assert_eq!(integer(f.get("context.height")), int(65536));

    // Unknown names in a known namespace are not found.
    assert!(f.get("tw.nope").is_none());
    assert!(f.get("context.nope").is_none());
    assert!(f.get("system.nope").is_none());
    // The prefixes alone are too short.
    assert!(f.get("tw.").is_none());
    assert!(f.get("context.").is_none());
    assert!(f.get("system.").is_none());
}

#[test]
fn tw_syncneeded_reads_tdb2() {
    let mut f = Fixture::new(&[]);
    // C++ throws from `num_local_changes` without a replica.
    assert!(get_dom(&mut f.ctx, "tw.syncneeded").is_err());

    f.open();
    assert_eq!(integer(f.get("tw.syncneeded")), int(0));
    f.add(r#"{"description":"one"}"#);
    assert_eq!(integer(f.get("tw.syncneeded")), int(1));
}

/// A task with every attribute type the C++ branches distinguish.
const RICH: &str = r#"{"uuid":"a8a2f0a1-7c4e-4a5b-9d6e-1234567890ab","description":"rich task","status":"pending","entry":"20260101T120000Z","due":"20260315T093045Z","recur":"weekly","project":"home","tags":["alpha","beta"],"size":"3.5","span":"P1D","annotations":[{"entry":"20260102T080910Z","description":"first note"},{"entry":"20260103T000000Z","description":"second note"}]}"#;

#[test]
fn task_attribute_references() {
    let mut f = Fixture::new(&[]);
    let mut task = Task::from_input(&mut f.ctx, RICH).unwrap();
    task.id = 7;
    let task = &task;

    // id / urgency special cases.
    assert_eq!(integer(f.get_task("id", Some(task))), int(7));
    let urgency = f.get_task("urgency", Some(task)).unwrap();
    assert_eq!(urgency.type_(), VariantType::Real);
    assert_eq!(
        urgency.get_real(),
        f64::from(task.urgency_c(&mut f.ctx).unwrap())
    );

    // Plain string attributes, and abbreviations through canonicalize.
    assert_eq!(
        string(f.get_task("description", Some(task))),
        s("rich task")
    );
    assert_eq!(string(f.get_task("desc", Some(task))), s("rich task"));
    assert_eq!(string(f.get_task("project", Some(task))), s("home"));
    assert_eq!(string(f.get_task("status", Some(task))), s("pending"));
    assert_eq!(
        string(f.get_task("uuid", Some(task))),
        s("a8a2f0a1-7c4e-4a5b-9d6e-1234567890ab")
    );

    // tags: the deprecated whole-list form and single tags (virtual too).
    assert_eq!(string(f.get_task("tags", Some(task))), s("alpha,beta"));
    assert_eq!(string(f.get_task("tags.alpha", Some(task))), s("alpha"));
    assert_eq!(string(f.get_task("tags.gamma", Some(task))), s(""));
    assert_eq!(string(f.get_task("tags.PENDING", Some(task))), s("PENDING"));
    assert_eq!(string(f.get_task("tags.UDA", Some(task))), s("UDA"));

    // Dates: a date Variant, or "" when unset.
    let due = f.get_task("due", Some(task)).unwrap();
    assert_eq!(due.type_(), VariantType::Date);
    assert_eq!(due.get_date(), task.get_date("due"));
    assert_eq!(string(f.get_task("end", Some(task))), s(""));

    // Date elements.
    let date = Datetime::from_epoch(task.get_date("due"));
    for (element, expected) in [
        ("year", date.year()),
        ("month", date.month()),
        ("day", date.day()),
        ("week", date.week()),
        ("weekday", date.day_of_week()),
        ("julian", date.day_of_year()),
        ("hour", date.hour()),
        ("minute", date.minute()),
        ("second", date.second()),
    ] {
        assert_eq!(
            integer(f.get_task(&format!("due.{element}"), Some(task))),
            int(i64::from(expected)),
            "{element}"
        );
    }
    // An unset date is the epoch.
    let epoch = Datetime::from_epoch(0);
    assert_eq!(
        integer(f.get_task("end.year", Some(task))),
        int(i64::from(epoch.year()))
    );
    // An unknown element falls through to the context-free lookup.
    assert!(f.get_task("due.nope", Some(task)).is_none());
    // Elements only apply to date columns.
    assert!(f.get_task("project.year", Some(task)).is_none());

    // Durations: recur (whatever its column type) and duration UDAs.
    let recur = f.get_task("recur", Some(task)).unwrap();
    assert_eq!(
        (recur.type_(), recur.get_duration()),
        (VariantType::Duration, 604800)
    );
    let span = f.get_task("span", Some(task)).unwrap();
    assert_eq!(
        (span.type_(), span.get_duration()),
        (VariantType::Duration, 86400)
    );

    // Numeric UDA, and unset UDAs are "" whatever their type.
    let size = f.get_task("size", Some(task)).unwrap();
    assert_eq!((size.type_(), size.get_real()), (VariantType::Real, 3.5));
    assert_eq!(string(f.get_task("note", Some(task))), s(""));
    assert_eq!(string(f.get_task("when", Some(task))), s(""));

    // Annotations.
    assert_eq!(integer(f.get_task("annotations.count", Some(task))), int(2));
    assert_eq!(
        string(f.get_task("annotations.1.description", Some(task))),
        s("first note")
    );
    assert_eq!(
        string(f.get_task("annotations.2.description", Some(task))),
        s("second note")
    );
    let entry = f.get_task("annotations.1.entry", Some(task)).unwrap();
    assert_eq!(entry.type_(), VariantType::Date);
    let first_key = task.get_annotations().keys().next().unwrap().clone();
    assert_eq!(entry.get_date(), first_key[11..].parse::<i64>().unwrap());
    let entry_date = Datetime::from_epoch(entry.get_date());
    assert_eq!(
        integer(f.get_task("annotations.1.entry.year", Some(task))),
        int(i64::from(entry_date.year()))
    );
    assert_eq!(
        integer(f.get_task("annotations.1.entry.second", Some(task))),
        int(i64::from(entry_date.second()))
    );
    assert!(f.get_task("annotations.3.entry", Some(task)).is_none());
    assert!(f
        .get_task("annotations.0.description", Some(task))
        .is_none());
    assert!(f.get_task("annotations.1.nope", Some(task)).is_none());
    assert!(f.get_task("annotations.1.entry.nope", Some(task)).is_none());

    // Context-free references still resolve with a task.
    assert_eq!(string(f.get_task("rc.foo", Some(task))), s("bar"));
    // Not attributes.
    assert!(f.get_task("nope", Some(task)).is_none());
    assert!(f.get_task("", Some(task)).is_none());
}

#[test]
fn no_task_delegates_to_the_context_free_lookup() {
    let mut f = Fixture::new(&[]);
    assert!(f.get_task("description", None).is_none());
    assert!(f.get_task("id", None).is_none());
    assert_eq!(string(f.get_task("rc.foo", None)), s("bar"));
    assert_eq!(string(f.get_task("system.version", None)), s("3.5.0"));
}

#[test]
fn id_and_uuid_references_load_from_tdb2() {
    let mut f = Fixture::new(&[]);

    // C++ throws from `TDB2::get` without a replica.
    assert!(get_dom_task(&mut f.ctx, "1.description", None).is_err());

    f.open();
    let one = f.add(r#"{"description":"first"}"#);
    let two = f.add(r#"{"description":"second","due":"20260401T000000Z"}"#);
    let one = f.ctx.tdb2.get_by_uuid(&one.get("uuid")).unwrap().unwrap();
    let two = f.ctx.tdb2.get_by_uuid(&two.get("uuid")).unwrap().unwrap();
    assert_eq!((one.id, two.id), (1, 2));

    assert_eq!(string(f.get_task("1.description", None)), s("first"));
    assert_eq!(string(f.get_task("2.description", None)), s("second"));
    assert_eq!(integer(f.get_task("2.id", None)), int(2));
    assert_eq!(
        integer(f.get_task("2.due.year", None)),
        int(i64::from(Datetime::from_epoch(two.get_date("due")).year()))
    );
    assert_eq!(
        string(f.get_task(&format!("{}.description", one.get("uuid")), None)),
        s("first")
    );
    // A partial UUID (the Lexer accepts 8+ hex digits).
    assert_eq!(
        string(f.get_task(&format!("{}.description", &two.get("uuid")[..8]), None)),
        s("second")
    );
    assert_eq!(
        integer(f.get_task(&format!("{}.annotations.count", one.get("uuid")), None)),
        int(0)
    );

    // Relative to a task: another ID loads it, the task's own ID uses it.
    let mut edited = one.clone();
    edited.set("description", "first (edited)");
    assert_eq!(
        string(f.get_task("1.description", Some(&edited))),
        s("first (edited)")
    );
    assert_eq!(
        string(f.get_task("2.description", Some(&edited))),
        s("second")
    );
    assert_eq!(
        string(f.get_task(&format!("{}.description", one.get("uuid")), Some(&edited))),
        s("first (edited)")
    );

    // An unknown ID leaves the contextual task (if any) as the reference,
    // with the ID element eaten.
    assert!(f.get_task("99.description", None).is_none());
    assert_eq!(
        string(f.get_task("99.description", Some(&edited))),
        s("first (edited)")
    );
    // ID 0 is never loaded.
    assert_eq!(string(f.get_task("0.description", Some(&two))), s("second"));
    // A decimal is not an ID: no element is eaten, and 'tw.1'-like names are
    // left alone.
    assert!(f.get_task("1.5.description", None).is_none());
}

#[test]
fn context_resolver_is_dom_source() {
    let mut f = Fixture::new(&["task", "calc"]);
    let mut task = Task::from_input(&mut f.ctx, RICH).unwrap();
    task.id = 3;

    // Without a task (C++ null currentTask).
    {
        let mut r = ContextResolver::with_task(&mut f.ctx, None);
        let v = r.resolve("rc.foo").unwrap().unwrap();
        assert_eq!(v.get_string(), "bar");
        assert_eq!(v.source(), "rc.foo", "domSource sets the source");
        assert!(r.resolve("description").unwrap().is_none());
        assert!(r.current_task().is_none());
    }

    // With a task: attributes resolve and virtual tags see the Context.
    {
        let mut r = ContextResolver::with_task(&mut f.ctx, Some(&task));
        assert_eq!(
            r.resolve("description").unwrap().unwrap().get_string(),
            "rich task"
        );
        let current = r.current_task().expect("a current task");
        assert!(current.has_tag("alpha").unwrap());
        assert!(
            current.has_tag("UDA").unwrap(),
            "UDA columns come from the Context"
        );
        assert_eq!(current.get_annotations().len(), 2);

        let e = Eval::new();
        assert!(e
            .evaluate_infix_expression("project == 'home'", &mut r)
            .unwrap()
            .get_bool());
        assert!(e
            .evaluate_infix_expression("tags _hastag_ UDA", &mut r)
            .unwrap()
            .get_bool());
        assert_eq!(
            e.evaluate_infix_expression("id * 2", &mut r)
                .unwrap()
                .get_integer(),
            6
        );
        assert_eq!(
            e.evaluate_infix_expression("due.year", &mut r)
                .unwrap()
                .get_integer(),
            2026
        );
    }

    // ContextResolver::new reads ctx.current_task.
    f.ctx.current_task = Some(std::rc::Rc::new(task.clone()));
    {
        let mut r = ContextResolver::new(&mut f.ctx);
        assert_eq!(r.resolve("id").unwrap().unwrap().get_integer(), 3);
        assert!(r.current_task().is_some());
    }
    f.ctx.current_task = None;
    {
        let mut r = ContextResolver::new(&mut f.ctx);
        assert!(r.resolve("id").unwrap().is_none());
        assert!(r.current_task().is_none());
        assert_eq!(r.context().config.get("foo"), "bar");
    }

    // Debug output goes to Context::debug.
    f.ctx.debug_messages.clear();
    {
        let mut e = Eval::new();
        e.debug(true);
        let mut r = ContextResolver::new(&mut f.ctx);
        assert_eq!(
            e.evaluate_infix_expression("rc.foo", &mut r)
                .unwrap()
                .get_string(),
            "bar"
        );
    }
    assert!(
        f.ctx
            .debug_messages
            .iter()
            .any(|m| m == "Eval identifier source 'rc.foo' → ↑'bar'"),
        "{:?}",
        f.ctx.debug_messages
    );

    // A getDOM throw aborts the evaluation (here: no replica for '1.').
    let mut r = ContextResolver::new(&mut f.ctx);
    assert!(Eval::new()
        .evaluate_infix_expression("1.description", &mut r)
        .is_err());
    let mut compiled = Eval::new();
    let tokens = vec![("1.description".to_string(), LexerType::dom)];
    assert!(
        compiled.compile_expression(&tokens, &mut r).is_err(),
        "the parsePrimitive lookup throws too"
    );
}
