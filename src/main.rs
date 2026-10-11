//! Rust translation of `src/main.cpp`.
//!
//! The C++ `main` returns an `int` status, which the OS truncates to its low
//! byte (-1 -> 255, -2 -> 254). The `catch` clauses map to [`TwError`]:
//!
//! | C++ `catch`               | Rust                                         | stderr                           | status |
//! |---------------------------|----------------------------------------------|----------------------------------|--------|
//! | `const std::string&`      | `Message`, `Io`, `NotImplemented`            | message + `\n`                   | -1     |
//! | `rust::Error&`            | `TaskChampion`                               | message + `\n`                   | -1     |
//! | `...` (e.g. `throw int`)  | `Status`, `Runtime`, or a panic              | `Unknown error. Please report.\n`| -2     |
//!
//! `std::bad_alloc` (-3) has no Rust equivalent (allocation failure aborts),
//! and `std::regex_error` is never thrown: libshared `RX` rethrows it as
//! `std::runtime_error`, i.e. [`TwError::Runtime`], which only `catch (...)`
//! catches (as in `Context::initialize` / `Context::run`, which normally
//! catch it first).
//!
//! Deviation: on a panic the Rust panic message is printed to stderr (by
//! the default panic hook) before the C++ catch-all text.

use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::ExitCode;

use taskwarrior::context::Context;
use taskwarrior::error::{Result, TwError};
use taskwarrior::version::VERSION;

/// Writes `text` to stderr after flushing stdout (`std::cerr` is tied to
/// `std::cout`). Write failures are ignored, as with iostreams.
fn to_stderr(text: &str) {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().write_all(text.as_bytes());
}

/// The body of the C++ `try` block.
fn initialize_and_run(context: &mut Context, args: &[String]) -> Result<i32> {
    let mut status = context.initialize(args)?;
    if status == 0 {
        status = context.run();
    }
    Ok(status)
}

/// The C++ `catch` clauses: the status and the stderr text for the outcome
/// of the `try` block (`Err` of the outer result is a panic).
fn status_for(result: std::thread::Result<Result<i32>>) -> (i32, Option<String>) {
    match result {
        Ok(Ok(status)) => (status, None),

        // catch (const std::string& error) / catch (rust::Error& err)
        Ok(Err(
            err @ (TwError::Message(_)
            | TwError::Io(_)
            | TwError::NotImplemented(_)
            | TwError::TaskChampion(_)),
        )) => (-1, Some(format!("{err}\n"))),

        // catch (...)
        Ok(Err(TwError::Status(_) | TwError::Runtime(_))) | Err(_) => {
            (-2, Some("Unknown error. Please report.\n".to_string()))
        }
    }
}

/// The process exit code for a C++ `main` return value (its low byte).
fn exit_code(status: i32) -> u8 {
    (status & 0xff) as u8
}

fn main() -> ExitCode {
    // Ignore SIGPIPE from writes to network sockets after the remote end has hung
    // up. Rust code expects this, and the Rust runtime ignores this signal at startup.
    // SAFETY: installing SIG_IGN for SIGPIPE has no preconditions.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    let mut context = Context::new();

    let mut args: Vec<String> = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    // Reuse the existing help command instead of maintaining separate output.
    if args.len() == 2 && args[1] == "--help" {
        args[1] = "help".to_string();
    }

    // Lightweight version checking that doesn't require initialization or any I/O.
    let status = if args.len() == 2 && args[1] == "--version" {
        let _ = writeln!(std::io::stdout(), "{VERSION}");
        0
    } else {
        let result = catch_unwind(AssertUnwindSafe(|| initialize_and_run(&mut context, &args)));
        let (status, message) = status_for(result);
        if let Some(message) = message {
            to_stderr(&message);
        }
        status
    };

    let _ = std::io::stdout().flush();
    ExitCode::from(exit_code(status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_clauses() {
        assert_eq!(status_for(Ok(Ok(0))), (0, None));
        assert_eq!(status_for(Ok(Ok(2))), (2, None));
        assert_eq!(
            status_for(Ok(Err(TwError::Message("boom".into())))),
            (-1, Some("boom\n".to_string()))
        );
        assert_eq!(
            status_for(Ok(Err(TwError::NotImplemented("add".into())))),
            (
                -1,
                Some("Command 'add' is not yet implemented.\n".to_string())
            )
        );
        assert_eq!(
            status_for(Ok(Err(TwError::Io(std::io::Error::other("disk"))))),
            (-1, Some("disk\n".to_string()))
        );
        let unknown = Some("Unknown error. Please report.\n".to_string());
        assert_eq!(
            status_for(Ok(Err(TwError::Status(4)))),
            (-2, unknown.clone())
        );
        assert_eq!(
            status_for(Ok(Err(TwError::Runtime(
                "Parenthesis is not closed.".into()
            )))),
            (-2, unknown.clone())
        );
        assert_eq!(status_for(Err(Box::new("panic"))), (-2, unknown));
    }

    #[test]
    fn exit_codes_are_the_low_byte() {
        assert_eq!(exit_code(0), 0);
        assert_eq!(exit_code(2), 2);
        assert_eq!(exit_code(-1), 255);
        assert_eq!(exit_code(-2), 254);
        assert_eq!(exit_code(-3), 253);
        assert_eq!(exit_code(256), 0);
    }
}
