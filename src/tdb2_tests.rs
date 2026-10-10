//! Assertion-count-checked ports of Taskwarrior `test/tdb2_test.cpp` and
//! `test/tc_cpp_test.cpp`, with the C++ messages as assertion messages.
//!
//! Counted assertions:
//! * `tdb2_test.cpp` (plan 12): **4 execute in C++**. Its task is built with
//!   `Task task(R"([description:"description" name:"value"])")`, a leftover
//!   of the removed FF4 format: `Task::parse` is JSON-only in 3.x, and
//!   `json::parse` throws `Error: missing ']' at position 1`. The test's
//!   `catch (const std::string&)` prints the diag and returns -1, but the
//!   `UnitTest` destructor runs first and calls `exit (_failed > 0)`, so the
//!   upstream test "passes" after the 4 empty-database assertions. The port
//!   asserts exactly that (the parse error and an executed count of 4), then,
//!   as a marked `Deviation:`, continues with the JSON equivalent of the
//!   intended task so the remaining 8 assertions (add / modify through
//!   `tdb2::add` / `tdb2::modify`) also run: 12 in total. Two of those
//!   expectations are stale in C++ (they predate taskchampion's automatic
//!   `modified` update, which the C++ bridge also triggers) and are
//!   recomputed, marked `Deviation:` at the spot. No assertion
//!   exercises a function deferred to a later milestone; nothing excluded.
//! * `tc_cpp_test.cpp` (no plan): 44 execute in C++ (the final
//!   "error message has full context" check runs inside the `catch`, which
//!   is always reached). The cxx-bridge calls are mapped onto the native
//!   replica as Taskwarrior now uses it: `tc::new_replica_on_disk` ->
//!   [`Tdb2::open_replica_at`], `tc::create_task` / `TaskData::update` /
//!   `delete_task` -> [`TaskData::create`] / [`TaskData::update`] /
//!   [`TaskData::delete`], `tc::add_undo_point` -> [`add_undo_point`],
//!   `ops[i].*` -> [`crate::operation::Operation`] getters (`get_value (str)`
//!   returning `bool` becomes `Option`), replica methods -> the [`Tdb2`]
//!   accessors (`commit_operations`, `get_task_data`, `get_undo_operations`,
//!   `commit_reversed_operations`, `sync_to_local`), `properties ()` /
//!   `items ()` -> [`TaskData::properties`] / [`TaskData::iter`], and
//!   `all_task_data` -> the replica's `HashMap` (each entry's key must be its
//!   task's uuid: the native form of "fully populated").
//!   A further 12 assertions ([`NATIVE_MAPPING_ASSERTIONS`], not from C++)
//!   check the Task <-> TaskData mapping and the working set through Tdb2,
//!   which in C++ lived in `Task::parseTC` / `TDB2` on top of the bridge.

use super::*;
use crate::operation::Operation;
use crate::test_util::{env_lock, EnvGuard};
use std::fmt::Debug;
use std::sync::MutexGuard;
use tempfile::TempDir;

/// The C++ `UnitTest` (test/test.cpp), recording instead of printing.
struct UnitTest {
    counter: usize,
    failures: Vec<String>,
}

impl UnitTest {
    fn new() -> Self {
        UnitTest { counter: 0, failures: Vec::new() }
    }

    fn ok(&mut self, expression: bool, name: &str) {
        self.counter += 1;
        if !expression {
            self.failures.push(format!("not ok {} - {}", self.counter, name));
        }
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

    fn check(&self) {
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
    }
}

/// Assertions of `tdb2_test.cpp` that execute in C++ (before the throw).
const TDB2_TEST_EXECUTED_IN_CPP: usize = 4;

/// All assertions of `tdb2_test.cpp` (4 empty + 4 after add + 4 after set),
/// run here thanks to the documented deviation.
const TDB2_TEST_ASSERTIONS: usize = 12;

/// Assertions executed by `tc_cpp_test.cpp`.
const TC_CPP_TEST_ASSERTIONS: usize = 44;

/// Additional native-mapping assertions run after the `tc_cpp_test.cpp` ones.
const NATIVE_MAPPING_ASSERTIONS: usize = 12;

/// The C++ `Context` test environment. Fields drop in declaration order: the
/// Context and data directory go first, then the statics and environment are
/// restored, and the lock is released last.
struct Upstream {
    ctx: Context,
    dir: TempDir,
    attributes: BTreeMap<String, String>,
    env: Option<EnvGuard>,
    _lock: MutexGuard<'static, ()>,
}

impl Upstream {
    fn setup() -> Self {
        // Context::new reads $HOME; serialize with env-mutating tests.
        let lock = env_lock();
        let env = EnvGuard::save(&["TASKDATA", "TASKRC"]);

        // Ensure environment has no influence.
        std::env::remove_var("TASKDATA");
        std::env::remove_var("TASKRC");

        let attributes = Task::attributes();
        // A fresh C++ process starts with no attribute types.
        Task::set_attributes(BTreeMap::new());

        let dir = TempDir::new().unwrap();
        let ctx = Context::new();
        Upstream { ctx, dir, attributes, env: Some(env), _lock: lock }
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        Task::set_attributes(std::mem::take(&mut self.attributes));
        drop(self.env.take());
        // `_lock` is released after this body returns.
    }
}

#[test]
fn tdb2_test_cpp() {
    let mut f = Upstream::setup();
    let location = f.dir.path().to_str().unwrap().to_string();
    let context = &mut f.ctx;
    let mut t = UnitTest::new();

    // cleardb (): the TempDir starts empty.

    // Set the context to allow GC.
    context.config.set("gc", 1);
    context.config.set("debug", 1);

    context.tdb2.open_replica_at(&location, /*create_if_missing=*/ true, /*read_write=*/ true).unwrap();

    // Try reading an empty database.
    let mut pending = context.tdb2.pending_tasks().unwrap().len();
    let mut completed = context.tdb2.completed_tasks().unwrap().len();
    let mut num_reverts_possible = context.tdb2.num_reverts_possible().unwrap();
    let mut num_local_changes = context.tdb2.num_local_changes().unwrap();

    t.is(pending as i32, 0, "TDB2 Read empty pending");
    t.is(completed as i32, 0, "TDB2 Read empty completed");
    t.is(num_reverts_possible, 0, "TDB2 Read empty undo");
    t.is(num_local_changes, 0, "TDB2 Read empty backlog");

    // Add a task.
    let cpp_input = r#"[description:"description" name:"value"]"#;
    let parsed = Task::from_input(context, cpp_input);
    // C++: `json::parse` throws here; the test's catch prints the diag and
    // returns, and `~UnitTest` exits 0 with these 4 assertions run.
    assert_eq!(
        parsed.map(|_| ()).unwrap_err().to_string(),
        "Error: missing ']' at position 1",
        "the C++ Task constructor throws on the FF4-style input"
    );
    t.check();
    assert_eq!(t.counter, TDB2_TEST_EXECUTED_IN_CPP, "number of assertions executed in C++");

    // Deviation: C++ stops at the throw above. Continue with the JSON form of
    // the intended task so the add/modify assertions are exercised too.
    let mut task = Task::from_input(context, r#"{"description":"description","name":"value"}"#).unwrap();
    crate::tdb2::add(context, &mut task).unwrap();

    pending = context.tdb2.pending_tasks().unwrap().len();
    completed = context.tdb2.completed_tasks().unwrap().len();
    num_reverts_possible = context.tdb2.num_reverts_possible().unwrap();
    num_local_changes = context.tdb2.num_local_changes().unwrap();

    t.is(pending as i32, 1, "TDB2 after add, 1 pending task");
    t.is(completed as i32, 0, "TDB2 after add, 0 completed tasks");
    t.is(num_reverts_possible, 1, "TDB2 after add, 1 revert possible");
    // Deviation: the C++ expectation (6) is stale and never ran upstream (see
    // the throw above). `TDB2::add` sets each attribute with taskchampion's
    // `Task::set_value` (the bridge's `tc::Task::set_value` in C++), which
    // records an automatic `modified` update before the first other property
    // ("description"). Undo points are not counted, so the local changes are
    // create + auto-modified + description, entry, modified, name, status = 7.
    t.is(num_local_changes, 7, "TDB2 after add, 6 local changes");

    task.set("description", "This is a test");
    crate::tdb2::modify(context, &mut task).unwrap();

    pending = context.tdb2.pending_tasks().unwrap().len();
    completed = context.tdb2.completed_tasks().unwrap().len();
    num_reverts_possible = context.tdb2.num_reverts_possible().unwrap();
    num_local_changes = context.tdb2.num_local_changes().unwrap();

    t.is(pending as i32, 1, "TDB2 after set, 1 pending task");
    t.is(completed as i32, 0, "TDB2 after set, 0 completed tasks");
    t.is(num_reverts_possible, 1, "TDB2 after set, 1 revert possible");

    // At this point, there may be 7 or 8 local changes, depending on whether
    // the `modified` property changed between the `add` and `modify`
    // invocation. That only happens if the clock ticks over to the next second
    // between those invocations.
    //
    // Deviation: as after the add, the stale C++ numbers are one automatic
    // `modified` update short. `TDB2::modify` sets the changed "description"
    // with `Task::set_value` (auto-modified + description: +2), and also
    // "modified" when the clock ticked over (+1), so 7 + 2 = 9 or 10.
    t.ok(num_local_changes == 9 || num_local_changes == 10, "TDB2 after set, 7 or 8 local changes");

    // Reset for reuse: C++ unlinks the database and reopens it in place; a
    // fresh directory is the equivalent without removing an open SQLite file.
    let reset = f.dir.path().join("reset");
    context.tdb2.open_replica_at(reset.to_str().unwrap(), /*create_if_missing=*/ true, /*read_write=*/ true).unwrap();

    t.check();
    assert_eq!(t.counter, TDB2_TEST_ASSERTIONS, "number of assertions executed");
}

/// C++ `uuid2str`.
fn uuid2str(uuid: Uuid) -> String {
    uuid.as_hyphenated().to_string()
}

/// C++ `TaskData::get (property, str)`: true and `str` updated when the
/// property exists.
fn get(task: &TaskData, property: &str, out: &mut String) -> bool {
    match task.get(property) {
        Some(value) => {
            *out = value.to_string();
            true
        }
        None => false,
    }
}

/// C++ `Operation::get_value (str)` / `get_old_value (str)` on the bridge.
fn take(value: Option<String>, out: &mut String) -> bool {
    match value {
        Some(value) => {
            *out = value;
            true
        }
        None => false,
    }
}

// Tests for the basic replica functionality. This focuses on the methods with
// complex mappings, rather than those with complex taskchampion
// implementations but simple APIs, like sync.
#[test]
fn tc_cpp_test_cpp() {
    let tmp = TempDir::new().unwrap();
    let mut t = UnitTest::new();
    let mut str: String;

    let mut replica = Tdb2::new();
    replica.open_replica_at(tmp.path().to_str().unwrap(), true, true).unwrap();
    let uuid = Uuid::new_v4();
    let uuid2 = Uuid::new_v4();
    t.is(uuid2str(uuid).len(), 36usize, "uuid string is the right length");

    let mut ops = Operations::new();
    let mut task = TaskData::create(uuid, &mut ops);
    t.is(uuid2str(task.get_uuid()), uuid2str(uuid), "new task has correct uuid");
    task.update("status", Some("pending".into()), &mut ops);
    task.update("description", Some("a task".into()), &mut ops);
    task.update("description", Some("a cool task".into()), &mut ops);
    add_undo_point(&mut ops);
    task.delete(&mut ops);

    let op = Operation::operations(&ops);

    t.is(op[0].is_create(), true, "ops[0] is create");
    t.is(op[0].get_uuid(), uuid2str(uuid), "ops[0] has correct uuid");

    t.is(op[1].is_update(), true, "ops[1] is update");
    t.is(op[1].get_uuid(), uuid2str(uuid), "ops[1] has correct uuid");
    str = op[1].get_property();
    t.is(str.as_str(), "status", "ops[1] property is 'status'");
    t.ok(take(op[1].get_value(), &mut str), "get_value succeeds");
    t.is(str.as_str(), "pending", "ops[1] value is 'pending'");
    t.ok(!take(op[1].get_old_value(), &mut str), "get_old_value has no old value");

    t.is(op[2].is_update(), true, "ops[2] is update");
    t.is(op[2].get_uuid(), uuid2str(uuid), "ops[2] has correct uuid");
    str = op[2].get_property();
    t.is(str.as_str(), "description", "ops[2] property is 'description'");
    t.ok(take(op[2].get_value(), &mut str), "get_value succeeds");
    t.is(str.as_str(), "a task", "ops[2] value is 'a task'");
    t.ok(!take(op[2].get_old_value(), &mut str), "get_old_value has no old value");

    t.is(op[3].is_update(), true, "ops[3] is update");
    t.is(op[3].get_uuid(), uuid2str(uuid), "ops[3] has correct uuid");
    str = op[3].get_property();
    t.is(str.as_str(), "description", "ops[3] property is 'description'");
    t.ok(take(op[3].get_value(), &mut str), "get_value succeeds");
    t.is(str.as_str(), "a cool task", "ops[3] value is 'a cool task'");
    t.ok(take(op[3].get_old_value(), &mut str), "get_old_value succeeds");
    t.is(str.as_str(), "a task", "ops[3] old value is 'a task'");

    t.is(op[4].is_undo_point(), true, "ops[4] is undo_point");

    t.is(op[5].is_delete(), true, "ops[5] is delete");
    t.is(op[5].get_uuid(), uuid2str(uuid), "ops[5] has correct uuid");
    let old_task = op[5].get_old_task();
    // old_task is in arbitrary order, so just check that status is in there.
    let mut found = false;
    for (p, v) in &old_task {
        if p == "status" {
            t.is(v.as_str(), "pending", "old_task has status:pending");
            found = true;
        }
    }
    t.ok(found, "found the status property in ops[5].old_task");

    replica.commit_operations(ops).unwrap();
    let maybe_task2 = replica.get_task_data(Uuid::new_v4()).unwrap();
    t.ok(maybe_task2.is_none(), "looking up a random uuid gets nothing");

    // The last operation deleted the task, but we want to see the task, so undo it..
    let undo_ops = replica.get_undo_operations().unwrap();
    t.ok(replica.commit_reversed_operations(undo_ops).unwrap(), "undo committed successfully");

    let maybe_task3 = replica.get_task_data(uuid).unwrap();
    t.ok(maybe_task3.is_some(), "looking up the original uuid get TaskData");
    let task3 = maybe_task3.unwrap_or_else(|| TaskData::create(uuid, &mut Operations::new()));
    t.is(uuid2str(task3.get_uuid()), uuid2str(uuid), "reloaded task has correct uuid");
    t.ok(get(&task3, "description", &mut str), "reloaded task has a description");
    t.is(str.as_str(), "a cool task", "reloaded task has correct description");
    t.ok(get(&task3, "status", &mut str), "reloaded task has a status");
    t.is(str.as_str(), "pending", "reloaded task has correct status");

    t.is(task3.properties().count(), 2usize, "task has 2 properties");
    t.is(task3.iter().count(), 2usize, "task has 2 items");

    let mut ops2 = Operations::new();
    let mut task4 = TaskData::create(uuid2, &mut ops2);
    task4.update("description", Some("another".into()), &mut ops2);
    replica.commit_operations(ops2).unwrap();

    let all_tasks = rt().block_on(replica.replica().unwrap().all_task_data()).unwrap();
    t.is(all_tasks.len(), 2usize, "now there are 2 tasks");
    for (key, task) in &all_tasks {
        t.ok(*key == task.get_uuid(), "all_tasks is fully populated");
        if task.get_uuid() == uuid {
            t.ok(get(task, "description", &mut str), "get_value succeeds");
            t.is(str.as_str(), "a cool task", "description is 'a cool task'");
        }
    }

    // Check exception formatting.
    if let Err(err) = replica.sync_to_local("/does/not/exist", false) {
        let msg = err.to_string();
        t.ok(
            msg.contains("/does/not/exist/taskchampion-local-sync-server.sqlite3") && msg.contains("Error code 14"),
            "error message has full context",
        );
    }

    t.check();
    assert_eq!(t.counter, TC_CPP_TEST_ASSERTIONS, "number of assertions executed");

    // Native mapping: TaskData -> Task (C++ Task::parseTC) ...
    let reloaded = Task::from_task_data(replica.get_task_data(uuid).unwrap().unwrap());
    t.is(reloaded.get("uuid"), uuid2str(uuid), "Task from TaskData has the uuid");
    t.is(reloaded.get("description"), "a cool task", "Task from TaskData has the description");
    t.is(reloaded.get("status"), "pending", "Task from TaskData has the status");
    t.is(reloaded.all().len(), 3usize, "Task from TaskData has 2 properties plus uuid");

    // ... the working set: only the pending task gets an ID ...
    replica.rebuild_working_set(true).unwrap();
    replica.invalidate_cached_info();
    t.is(replica.id(&uuid2str(uuid)).unwrap(), 1, "pending task has working-set ID 1");
    t.is(replica.id(&uuid2str(uuid2)).unwrap(), 0, "task without status has no working-set ID");

    // ... Tdb2 lookups map TaskData to Task with the ID ...
    let mut by_uuid = replica.get_by_uuid(&uuid2str(uuid)).unwrap().unwrap_or_default();
    t.is(by_uuid.id, 1, "get_by_uuid sets the working-set ID");
    t.is(replica.pending_tasks().unwrap().len(), 1usize, "one pending task");

    // ... and Task -> TaskData on modify.
    by_uuid.set("description", "changed");
    replica.modify_task(&mut by_uuid).unwrap();
    let data = replica.get_task_data(uuid).unwrap().unwrap();
    t.is(data.get("description"), Some("changed"), "modify_task updates the TaskData");
    t.ok(data.has("modified"), "modify_task stamps modified");
    t.is(data.get("uuid"), None, "uuid is not stored as a property");
    t.is(replica.all_tasks().unwrap().len(), 2usize, "all_tasks maps both TaskData");

    t.check();
    assert_eq!(
        t.counter,
        TC_CPP_TEST_ASSERTIONS + NATIVE_MAPPING_ASSERTIONS,
        "number of assertions executed"
    );
}
