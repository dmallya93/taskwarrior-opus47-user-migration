//! Rust translation of `src/main.cpp`.
//!
//! Placeholder entry point: a later foundation task rewrites this to
//! reproduce `main.cpp` exactly (Context initialisation, dispatch, stderr
//! output and exit codes).

use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{}", taskwarrior::version::VERSION);
        return ExitCode::SUCCESS;
    }
    eprintln!(
        "{} {}: not yet implemented.",
        taskwarrior::version::PACKAGE,
        taskwarrior::version::VERSION
    );
    ExitCode::FAILURE
}
