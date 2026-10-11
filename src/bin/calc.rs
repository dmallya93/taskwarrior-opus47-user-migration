//! Rust translation of `src/calc.cpp`: the standalone `calc` expression
//! evaluator that `test/calc.test.py` runs from `BIN_PREFIX`.
//!
//! The C++ `main` returns an `int` status, which the OS truncates to its low
//! byte (-1 -> 255, -2 -> 254); `exit (1)` after `--help` / `--version`
//! exits with 1. The `catch` clauses map to [`TwError`] as in `main.rs`:
//!
//! | C++ `catch`              | Rust                                          | stderr                             | status |
//! |--------------------------|-----------------------------------------------|------------------------------------|--------|
//! | `const std::string&`     | `Message`, `Io`, `NotImplemented`, `TaskChampion` | message + `\n`                 | -1     |
//! | `...`                    | `Status`, `Runtime`, or a panic               | `Unknown error occured.  Oops.\n`  | -2     |

use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::ExitCode;

use taskwarrior::error::{Result, TwError};
use taskwarrior::eval::{Eval, FnResolver};
use taskwarrior::shared::datetime::Datetime;
use taskwarrior::shared::duration::Duration;
use taskwarrior::shared::format::format;
use taskwarrior::shared::shared::os_name;
use taskwarrior::variant::Variant;
use taskwarrior::version::VERSION;

////////////////////////////////////////////////////////////////////////////////
/// Constants (C++ `bool get (const std::string&, Variant&)`, the added
/// source).
fn get(_name: &str) -> Option<Variant> {
    /*
      // An example, although a bad one because this is supported by default.
      if (name == "pi") {value = Variant (3.14159165); return true;}
    */

    None
}

/// What the C++ `try` block ends with: `exit (1)` or falling through to
/// `return status` with status 0.
enum Outcome {
    Exit(i32),
    Done,
}

/// The body of the C++ `try` block, writing to `out` (`std::cout`).
///
/// Deviation: C++ constructs a `Context` and installs it with
/// `Context::setContext`, only so that `Eval` can reach
/// `Context::getContext ().debug` (and a null `currentTask`). The Rust
/// `Eval` takes those from its resolver, so the resolver's
/// `debug_messages` stands in for `Context::debugMessages` and no Context
/// is built.
fn try_block(args: &[String], out: &mut dyn Write) -> Result<Outcome> {
    // Same operating parameters as Context::staticInitialization.
    Datetime::set_standalone_date_enabled(false);
    Datetime::set_standalone_time_enabled(false);
    Duration::set_standalone_seconds_enabled(false);

    let mut infix = true;

    // Add a source for constants.
    let mut e = Eval::new();
    let mut source = FnResolver::new(get);

    let argv0 = args.first().map(String::as_str).unwrap_or("");

    // Combine all the arguments into one expression string.
    let mut expression = String::new();
    for arg in args.iter().skip(1) {
        let arg = arg.as_str();
        if arg == "-h" || arg == "--help" {
            let _ = write!(
                out,
                "\nUsage: {argv0} [options] '<expression>'\n\
                 \n\
                 Options:\n  \
                 -h|--help         Display this usage\n  \
                 -d|--debug        Debug mode\n  \
                 -i|--infix        Infix expression (default)\n  \
                 -p|--postfix      Postfix expression\n\
                 \n"
            );
            return Ok(Outcome::Exit(1));
        } else if arg == "-v" || arg == "--version" {
            let _ = write!(
                out,
                "\n{}{}\n\
                 Copyright (C) 2006 - 2021 T. Babej, P. Beckingham, F. Hernandez.\n\
                 \n\
                 Taskwarrior may be copied only under the terms of the MIT license, which may \
                 be found in the Taskwarrior source kit.\n\
                 \n",
                format("calc {1} built for ", &[&VERSION]),
                os_name()
            );
            return Ok(Outcome::Exit(1));
        } else if arg == "-d" || arg == "--debug" {
            e.debug(true);
        } else if arg == "-i" || arg == "--infix" {
            infix = true;
        } else if arg == "-p" || arg == "--postfix" {
            infix = false;
        } else {
            expression += arg;
            expression.push(' ');
        }
    }

    let result = if infix {
        e.evaluate_infix_expression(&expression, &mut source)?
    } else {
        e.evaluate_postfix_expression(&expression, &mut source)?
    };

    // Show any debug output.
    for i in &source.debug_messages {
        let _ = writeln!(out, "{i}");
    }

    // Show the result in string form.
    let _ = writeln!(out, "{result}");
    Ok(Outcome::Done)
}

/// C++ `main`: runs the `try` block over `args` (`argv`), writing to `out`
/// / `err` (`std::cout` / `std::cerr`), and returns the C++ status.
fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| try_block(args, out)));
    // `std::cerr` is tied to `std::cout`.
    let _ = out.flush();
    match result {
        Ok(Ok(Outcome::Exit(code))) => code,
        Ok(Ok(Outcome::Done)) => 0,

        // catch (const std::string& error)
        Ok(Err(
            error @ (TwError::Message(_)
            | TwError::Io(_)
            | TwError::NotImplemented(_)
            | TwError::TaskChampion(_)),
        )) => {
            let _ = writeln!(err, "{error}");
            -1
        }

        // catch (...)
        Ok(Err(TwError::Status(_) | TwError::Runtime(_))) | Err(_) => {
            let _ = writeln!(err, "Unknown error occured.  Oops.");
            -2
        }
    }
}

/// Deviation: arguments that are not valid UTF-8 are converted lossily
/// (the evaluator works on `&str`); C++ passes the raw bytes through.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let status = run(&args, &mut stdout, &mut stderr);
    let _ = stdout.flush();
    // The OS keeps the low byte of the C++ `int` status.
    ExitCode::from(status as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(list: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = std::iter::once("calc")
            .chain(list.iter().copied())
            .map(String::from)
            .collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let status = run(&args, &mut out, &mut err);
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn regular_math() {
        assert_eq!(
            calc(&["12 * 3600 + 34 * 60 + 56"]),
            (0, "45296\n".into(), String::new())
        );
        // Arguments are joined with spaces.
        assert_eq!(
            calc(&["12", "*", "3600"]),
            (0, "43200\n".into(), String::new())
        );
    }

    #[test]
    fn bare_numbers_are_not_durations() {
        // Duration::standaloneSecondsEnabled is off, as in C++ calc.
        assert_eq!(calc(&["100*2"]), (0, "200\n".into(), String::new()));
        assert_eq!(calc(&["64"]), (0, "64\n".into(), String::new()));
    }

    #[test]
    fn postfix_math() {
        assert_eq!(
            calc(&["--postfix", "12 3600 * 34 60 * 56 + +"]),
            (0, "45296\n".into(), String::new())
        );
        assert_eq!(
            calc(&["-p", "-i", "1+2"]),
            (0, "3\n".into(), String::new()),
            "last option wins"
        );
    }

    #[test]
    fn debug_output_precedes_result() {
        let (status, out, err) = calc(&["--debug", "2- -3"]);
        assert_eq!((status, err.as_str()), (0, ""));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.contains(&"Eval literal number ↑'2'"), "{out}");
        assert!(lines.contains(&"Eval _neg_ ↓'3' → ↑'-3'"), "{out}");
        assert!(
            lines[0].starts_with("\x1b[1;37;42mFILTER\x1b[0m Infix        "),
            "{out}"
        );
        assert_eq!(lines.last(), Some(&"5"));
        assert_eq!(calc(&["-d", "1"]).1.lines().last(), Some("1"));
    }

    #[test]
    fn duration_literal() {
        let (status, out, _) = calc(&["--debug", "15min"]);
        assert_eq!(status, 0);
        assert!(out.contains("Eval literal duration ↑'PT15M'"), "{out}");
        assert!(out.ends_with("\nPT15M\n"), "{out}");
    }

    #[test]
    fn help() {
        let expected = "\nUsage: calc [options] '<expression>'\n\nOptions:\n  -h|--help         Display this usage\n  -d|--debug        Debug mode\n  -i|--infix        Infix expression (default)\n  -p|--postfix      Postfix expression\n\n";
        assert_eq!(calc(&["--help"]), (1, expected.into(), String::new()));
        assert_eq!(
            calc(&["1+1", "-h", "--bogus"]),
            (1, expected.into(), String::new())
        );
    }

    #[test]
    fn version() {
        let expected = format!(
            "\ncalc {VERSION} built for {}\nCopyright (C) 2006 - 2021 T. Babej, P. Beckingham, F. Hernandez.\n\nTaskwarrior may be copied only under the terms of the MIT license, which may be found in the Taskwarrior source kit.\n\n",
            os_name()
        );
        assert_eq!(calc(&["--version"]), (1, expected.clone(), String::new()));
        assert_eq!(calc(&["-v"]), (1, expected, String::new()));
    }

    #[test]
    fn errors() {
        assert_eq!(
            calc(&[]),
            (-1, String::new(), "No expression to evaluate.\n".into())
        );
        // Debug messages are not shown when evaluation throws.
        let (status, out, err) = calc(&["-d"]);
        assert_eq!(
            (status, out.as_str(), err.as_str()),
            (-1, "", "No expression to evaluate.\n")
        );
    }
}
