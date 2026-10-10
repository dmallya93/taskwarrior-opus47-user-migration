//! Rust replacement for `test/make_tc_task.cpp`: a test fixture helper that
//! writes a task directly into the taskchampion replica, bypassing
//! Taskwarrior entirely, and prints the new task's UUID.
//!
//! Usage: `make_tc_task DATADIR KEY=VALUE ..` (called by
//! `test/basetest/task.py` `Task.make_tc_task`). The properties are stored
//! verbatim with `TaskData::update`, exactly as the C++ did through the cxx
//! bridge, so no Taskwarrior validation or defaulting happens.

use std::io::Write;
use std::process::ExitCode;

use taskchampion::{Operations, TaskData, Uuid};
use taskwarrior::tdb2::Tdb2;

/// C++ `usage ()`.
fn usage() -> ExitCode {
    eprintln!("USAGE: make_tc_task DATADIR KEY=VALUE ..");
    ExitCode::from(1)
}

fn main() -> ExitCode {
    // The bridge converted every C++ string with `to_string_lossy`, so
    // arguments that are not valid UTF-8 are accepted the same way here.
    let mut args = std::env::args_os().skip(1).map(|arg| arg.to_string_lossy().into_owned());
    let Some(datadir) = args.next() else {
        return usage();
    };

    let mut tdb2 = Tdb2::new();
    if let Err(err) = tdb2.open_replica_at(&datadir, /*create_if_missing=*/ true, /*read_write=*/ true) {
        eprintln!("{err}");
        return ExitCode::FAILURE;
    }
    let uuid = Uuid::new_v4();
    let mut operations = Operations::new();
    let mut task = TaskData::create(uuid, &mut operations);

    for arg in args {
        let Some(eq_idx) = arg.find('=') else {
            return usage();
        };
        let property = &arg[..eq_idx];
        let value = &arg[eq_idx + 1..];
        task.update(property, Some(value.to_string()), &mut operations);
    }
    if let Err(err) = tdb2.commit_operations(operations) {
        eprintln!("{err}");
        return ExitCode::FAILURE;
    }

    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "{}", uuid.as_hyphenated());
    ExitCode::SUCCESS
}
