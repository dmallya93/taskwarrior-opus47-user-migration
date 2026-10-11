//! End-to-end tests of the `make_tc_task` fixture binary (C++
//! `test/make_tc_task.cpp`): it writes a task straight into the replica and
//! prints its UUID, as `test/basetest/task.py` `Task.make_tc_task` expects.

use std::process::Command;

use taskwarrior::tdb2::Tdb2;

fn make_tc_task(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_make_tc_task"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn no_arguments_prints_usage() {
    let output = make_tc_task(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "USAGE: make_tc_task DATADIR KEY=VALUE ..\n"
    );
}

#[test]
fn argument_without_equals_prints_usage() {
    let dir = tempfile::tempdir().unwrap();
    let output = make_tc_task(&[dir.path().to_str().unwrap(), "description"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "USAGE: make_tc_task DATADIR KEY=VALUE ..\n"
    );
}

#[test]
fn writes_task_properties_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let datadir = dir.path().to_str().unwrap();
    let output = make_tc_task(&[
        datadir,
        "status=pending",
        "description=hello world",
        "entry=1700000000",
        "unusual=a=b",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let uuid = stdout.strip_suffix('\n').unwrap().to_string();
    assert_eq!(uuid.len(), 36);
    assert_eq!(uuid, uuid.to_lowercase());

    let mut tdb2 = Tdb2::new();
    tdb2.open_replica_at(datadir, false, false).unwrap();
    let task = tdb2
        .get_by_uuid(&uuid)
        .unwrap()
        .expect("task written to the replica");
    assert_eq!(task.get("description"), "hello world");
    assert_eq!(task.get("status"), "pending");
    assert_eq!(task.get("entry"), "1700000000");
    // Only the first '=' separates property and value.
    assert_eq!(task.get("unusual"), "a=b");
    // No Taskwarrior defaulting happens: no "modified" stamp was added.
    assert!(!task.has("modified"));
    assert_eq!(task.id, 1);
}

#[test]
fn second_task_gets_next_working_set_id() {
    let dir = tempfile::tempdir().unwrap();
    let datadir = dir.path().to_str().unwrap();
    let first = make_tc_task(&[datadir, "status=pending", "description=one"]);
    let second = make_tc_task(&[datadir, "status=pending", "description=two"]);
    let first = String::from_utf8(first.stdout).unwrap().trim().to_string();
    let second = String::from_utf8(second.stdout).unwrap().trim().to_string();
    assert_ne!(first, second);

    let mut tdb2 = Tdb2::new();
    tdb2.open_replica_at(datadir, false, false).unwrap();
    assert_eq!(tdb2.id(&first).unwrap(), 1);
    assert_eq!(tdb2.id(&second).unwrap(), 2);
}
