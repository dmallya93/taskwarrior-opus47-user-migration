//! Assertion-count-checked ports of Taskwarrior `test/t_test.cpp` and
//! `test/tw_2689_test.cpp`, with the C++ messages as assertion messages.
//!
//! Counted assertions (what actually executes in C++; the `UnitTest t(N)`
//! plans are not used):
//! * `t_test.cpp`: 25 executed (its plan says 48, which is stale). It calls
//!   neither `Task::modify`/`substitute` (tested in `task_modify_tests.rs`)
//!   nor the milestone-7 `Task::diff`, so nothing is excluded.
//! * `tw_2689_test.cpp`: 12 executed (plan 12). Nothing excluded.
//!
//! Both C++ tests build a `Context` (never initialized: no rc file, empty
//! `Task::attributes` until the test fills it) and unset `TASKDATA`/`TASKRC`.
//! Here a `Context::new ()` gets a replica in a temporary data directory (the
//! `depends` handling reaches `context.tdb2`); [`Upstream`] holds
//! [`crate::test_util::env_lock`], unsets the two variables, and restores
//! them and the `Task::attributes` static before the lock is released.

use super::*;
use crate::test_util::{env_lock, EnvGuard};
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
        UnitTest { counter: 0, failures: Vec::new(), diags: Vec::new() }
    }

    fn ok(&mut self, expression: bool, name: &str) {
        self.counter += 1;
        if !expression {
            self.failures.push(format!("not ok {} - {}", self.counter, name));
        }
    }

    fn notok(&mut self, expression: bool, name: &str) {
        self.ok(!expression, name);
    }

    fn is<A: PartialEq<E> + Debug, E: Debug>(&mut self, actual: A, expected: E, name: &str) {
        self.counter += 1;
        if actual != expected {
            self.failures.push(format!(
                "not ok {} - {}\n#   expected: {:?}\n#        got: {:?}",
                self.counter, name, expected, actual
            ));
        }
    }

    /// C++ `diag`: not an assertion; shown with the failures.
    fn diag(&mut self, text: &str) {
        self.diags.push(format!("# {text}"));
    }

    /// Fails on any failed assertion, then checks the executed count.
    fn finish(&self, expected: usize) {
        assert!(self.failures.is_empty(), "{}\n{}", self.failures.join("\n"), self.diags.join("\n"));
        assert_eq!(self.counter, expected, "number of assertions executed");
    }
}

/// The C++ test `main` environment. Fields drop in declaration order: the
/// Context and data directory go first, then the statics and environment
/// are restored, and the lock is released last.
struct Upstream {
    ctx: Context,
    _dir: TempDir,
    attributes: BTreeMap<String, String>,
    env: Option<EnvGuard>,
    _lock: MutexGuard<'static, ()>,
}

impl Upstream {
    fn setup() -> Self {
        let lock = env_lock();
        let env = EnvGuard::save(&["TASKDATA", "TASKRC"]);

        // Ensure environment has no influence.
        std::env::remove_var("TASKDATA");
        std::env::remove_var("TASKRC");

        let attributes = Task::attributes();
        // A fresh C++ process starts with no attribute types.
        Task::set_attributes(BTreeMap::new());

        let dir = TempDir::new().unwrap();
        let mut ctx = Context::new();
        ctx.tdb2.open_replica_at(dir.path().to_str().unwrap(), true, true).unwrap();
        Upstream { ctx, _dir: dir, attributes, env: Some(env), _lock: lock }
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        Task::set_attributes(std::mem::take(&mut self.attributes));
        drop(self.env.take());
        // `_lock` is released after this body returns.
    }
}

/// Assertions executed by `t_test.cpp`: 8 status conversions, 2
/// `operator==`, 9 set/has/get/remove/all, 6 JSON constructor/tag checks.
const T_TEST_ASSERTIONS: usize = 25;

/// Assertions executed by `tw_2689_test.cpp`: 4 for each of the 3 samples.
const TW_2689_TEST_ASSERTIONS: usize = 12;

/// C++ `(int) Task::textToStatus (input)`; -1 stands in for a C++ value
/// that no status has (the Rust port reports invalid text as an error).
fn text_to_status(input: &str) -> i32 {
    Task::text_to_status(input).map_or(-1, |s| s as i32)
}

#[test]
fn t_test_cpp() {
    let mut f = Upstream::setup();
    let context = &mut f.ctx;
    let mut test = UnitTest::new();

    test.is(text_to_status("pending"), Status::Pending as i32, "textToStatus pending");
    test.is(text_to_status("completed"), Status::Completed as i32, "textToStatus completed");
    test.is(text_to_status("deleted"), Status::Deleted as i32, "textToStatus deleted");
    test.is(text_to_status("recurring"), Status::Recurring as i32, "textToStatus recurring");

    test.is(Task::status_to_text(Status::Pending), "pending", "statusToText pending");
    test.is(Task::status_to_text(Status::Completed), "completed", "statusToText completed");
    test.is(Task::status_to_text(Status::Deleted), "deleted", "statusToText deleted");
    test.is(Task::status_to_text(Status::Recurring), "recurring", "statusToText recurring");

    // Task::operator==
    let mut left = Task::from_input(context, "{\"one\":\"1\", \"two\":\"2\", \"three\":\"3\"}").unwrap();
    let right = left.clone();
    test.ok(left == right, "left == right -> true");
    left.set("one", "1.0");
    test.notok(left == right, "left == right -> false");

    ////////////////////////////////////////////////////////////////////////////
    // Task::set
    let mut task = Task::new();
    task.set("name", "value");
    test.is(task.compose_json(context, false).unwrap(), "{\"name\":\"value\"}", "Task::set");

    // Task::has
    test.ok(task.has("name"), "Task::has");
    test.notok(task.has("woof"), "Task::has not");

    // Task::get_int
    task.set_int("one", 1);
    test.is(task.compose_json(context, false).unwrap(), r#"{"name":"value","one":"1"}"#, "Task::set");
    test.is(task.get_int("one"), 1, "Task::get_int");

    // Task::get_ulong
    task.set("two", "4294967295");
    test.is(
        task.compose_json(context, false).unwrap(),
        r#"{"name":"value","one":"1","two":"4294967295"}"#,
        "Task::set",
    );
    test.is(task.get_ulong("two") as usize, 4294967295usize, "Task::get_ulong");

    // Task::remove
    task.remove("one");
    task.remove("two");
    test.is(task.compose_json(context, false).unwrap(), "{\"name\":\"value\"}", "Task::remove");

    // Task::all
    test.is(task.all().len(), 1usize, "Task::all size");

    ////////////////////////////////////////////////////////////////////////////
    let mut attributes = Task::attributes();
    attributes.insert("description".into(), "string".into());
    attributes.insert("entry".into(), "date".into());
    attributes.insert("tags".into(), "string".into());
    attributes.insert("uuid".into(), "string".into());
    Task::set_attributes(attributes);

    let mut good = true;
    if let Err(e) = Task::from_input(context, "{}") {
        test.diag(&e.to_string());
        good = false;
    }
    test.ok(good, "Task::Task ('{}')");

    good = true;
    if let Err(e) = Task::from_input(
        context,
        r#"{"uuid":"00000000-0000-0000-000000000001","description":"foo","entry":"1234567890"}"#,
    ) {
        test.diag(&e.to_string());
        good = false;
    }
    test.ok(good, "Task::Task ('{<minimal>}')");

    // Verify tag handling is correct
    let mut t6 = Task::new();
    t6.set("entry", "20130602T224000Z");
    t6.set("description", "DESC");
    t6.add_tag("tag1");
    test.is(
        t6.compose_json(context, false).unwrap(),
        r#"{"description":"DESC","entry":"20130602T224000Z","tags":["tag1"]}"#,
        "JSON good",
    );

    t6.add_tag("tag2");
    test.is(
        t6.compose_json(context, false).unwrap(),
        r#"{"description":"DESC","entry":"20130602T224000Z","tags":["tag1","tag2"]}"#,
        "JSON good",
    );

    good = true;
    let mut t7 = Task::new();
    match Task::from_input(context, r#"{"description":"DESC","entry":"20130602T224000Z","tags":["tag1","tag2"]}"#) {
        Ok(parsed) => t7 = parsed,
        Err(e) => {
            test.diag(&e.to_string());
            good = false;
        }
    }
    test.ok(good, "Task::Task ('{two tags}')");
    test.is(
        t7.compose_json(context, false).unwrap(),
        r#"{"description":"DESC","entry":"20130602T224000Z","tags":["tag1","tag2"]}"#,
        "JSON good",
    );

    test.finish(T_TEST_ASSERTIONS);
}

#[test]
fn tw_2689_test_cpp() {
    let mut f = Upstream::setup();
    let context = &mut f.ctx;
    let mut test = UnitTest::new();

    // Inform Task about the attributes in the JSON below
    let mut attributes = Task::attributes();
    attributes.insert("depends".into(), "string".into());
    attributes.insert("uuid".into(), "string".into());
    Task::set_attributes(attributes);

    // depends in [..] string from a taskserver (issue#2689)
    let mut sample = concat!(
        "{",
        "\"depends\":\"[\\\"92a40a34-37f3-4785-8ca1-ff89cfbfd105\\\",\\\"e08e35fa-e42b-4de0-acc4-",
        "518fca8f6365\\\"]\",",
        "\"uuid\":\"00000000-0000-0000-0000-000000000000\"",
        "}"
    );
    let mut json = Task::from_input(context, sample).unwrap();
    let mut value = json.get("uuid");
    test.is(value.as_str(), "00000000-0000-0000-0000-000000000000", "json [..] uuid");
    value = json.get("depends");
    test.is(
        value.as_str(),
        "92a40a34-37f3-4785-8ca1-ff89cfbfd105,e08e35fa-e42b-4de0-acc4-518fca8f6365",
        "json [..] depends",
    );
    test.ok(json.has("dep_92a40a34-37f3-4785-8ca1-ff89cfbfd105"), "json [..] dep attr");
    test.ok(json.has("dep_e08e35fa-e42b-4de0-acc4-518fca8f6365"), "json [..] dep attr");

    // depends in comma-delimited string from a taskserver (deprecated format)
    sample = concat!(
        "{",
        "\"depends\":\"92a40a34-37f3-4785-8ca1-ff89cfbfd105,e08e35fa-e42b-4de0-acc4-518fca8f6365\",",
        "\"uuid\":\"00000000-0000-0000-0000-000000000000\"",
        "}"
    );
    json = Task::from_input(context, sample).unwrap();
    value = json.get("uuid");
    test.is(value.as_str(), "00000000-0000-0000-0000-000000000000", "json comma-separated uuid");
    value = json.get("depends");
    test.is(
        value.as_str(),
        "92a40a34-37f3-4785-8ca1-ff89cfbfd105,e08e35fa-e42b-4de0-acc4-518fca8f6365",
        "json comma-separated depends",
    );
    test.ok(json.has("dep_92a40a34-37f3-4785-8ca1-ff89cfbfd105"), "json comma-separated dep attr");
    test.ok(json.has("dep_e08e35fa-e42b-4de0-acc4-518fca8f6365"), "json comma-separated dep attr");

    // depends in a JSON array from a taskserver
    sample = concat!(
        "{",
        "\"depends\":[\"92a40a34-37f3-4785-8ca1-ff89cfbfd105\",\"e08e35fa-e42b-4de0-acc4-",
        "518fca8f6365\"],",
        "\"uuid\":\"00000000-0000-0000-0000-000000000000\"",
        "}"
    );
    json = Task::from_input(context, sample).unwrap();
    value = json.get("uuid");
    test.is(value.as_str(), "00000000-0000-0000-0000-000000000000", "json array uuid");
    value = json.get("depends");
    test.is(
        value.as_str(),
        "92a40a34-37f3-4785-8ca1-ff89cfbfd105,e08e35fa-e42b-4de0-acc4-518fca8f6365",
        "json array depends",
    );
    test.ok(json.has("dep_92a40a34-37f3-4785-8ca1-ff89cfbfd105"), "json array dep attr");
    test.ok(json.has("dep_e08e35fa-e42b-4de0-acc4-518fca8f6365"), "json array dep attr");

    test.finish(TW_2689_TEST_ASSERTIONS);
}
