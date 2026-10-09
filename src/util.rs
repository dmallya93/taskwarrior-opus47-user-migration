//! Rust translation of `src/util.cpp`.
//!
//! Mostly not yet ported: the rest of this module is filled in by milestone 4
//! (Task model and TDB2). Only [`confirm4`] is here already, because
//! `Command::permission` (milestone 1) needs it.

use std::io::{BufRead, Write};

use crate::context::Context;
use crate::shared::shared::{auto_complete, lower_case, trim_with};

const STRING_UTIL_CONFIRM_YES: &str = "yes";
const STRING_UTIL_CONFIRM_YES_U: &str = "Yes";
const STRING_UTIL_CONFIRM_NO: &str = "no";
const STRING_UTIL_CONFIRM_ALL: &str = "all";
const STRING_UTIL_CONFIRM_ALL_U: &str = "All";
const STRING_UTIL_CONFIRM_QUIT: &str = "quit";

////////////////////////////////////////////////////////////////////////////////
/// C++ file-static `signal_handler` of `util.cpp`.
extern "C" fn signal_handler(s: libc::c_int) {
    if s == libc::SIGINT {
        let message = b"\n\nInterrupted: No changes made.\n";
        // SAFETY: write(2) and _exit(2) are async-signal-safe; the buffer is a
        // valid static byte string.
        unsafe {
            libc::write(libc::STDOUT_FILENO, message.as_ptr().cast(), message.len());
            libc::_exit(1);
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `confirm4`: asks `question` with a ` (yes/no/all/quit) ` suffix until
/// the answer autocompletes (minimum 1) to exactly one option.
///
/// Returns 0 = no, 1 = yes, 2 = all, 3 = quit. End of input means `quit`.
/// Each raw answer is recorded with `context.debug ("STDIN '<answer>'")`.
/// SIGINT while waiting prints `Interrupted: No changes made.` and exits 1.
pub fn confirm4(context: &mut Context, question: &str) -> i32 {
    let options = [
        STRING_UTIL_CONFIRM_YES_U,
        STRING_UTIL_CONFIRM_YES,
        STRING_UTIL_CONFIRM_NO,
        STRING_UTIL_CONFIRM_ALL_U,
        STRING_UTIL_CONFIRM_ALL,
        STRING_UTIL_CONFIRM_QUIT,
    ];
    let mut matches;

    let handler: extern "C" fn(libc::c_int) = signal_handler;
    // SAFETY: installing a handler that only calls async-signal-safe functions.
    unsafe {
        libc::signal(libc::SIGINT, handler as libc::sighandler_t);
    }

    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    loop {
        let mut stdout = std::io::stdout();
        let _ = write!(
            stdout,
            "{question} ({}/{}/{}/{}) ",
            options[1], options[2], options[4], options[5]
        );
        let _ = stdout.flush();

        // std::getline: reads up to '\n' (discarded); reaching end of input
        // before a newline sets eof.
        let mut buffer = Vec::new();
        let eof = match stdin.read_until(b'\n', &mut buffer) {
            Ok(_) if buffer.last() == Some(&b'\n') => {
                buffer.pop();
                false
            }
            _ => true,
        };

        let answer = String::from_utf8_lossy(&buffer).into_owned();
        context.debug(&format!("STDIN '{answer}'"));
        let answer = if eof {
            STRING_UTIL_CONFIRM_QUIT.to_string()
        } else {
            // Lexer::lowerCase (Lexer::trim (answer)): Lexer::trim trims spaces.
            lower_case(&trim_with(&answer, " "))
        };
        matches = auto_complete(&answer, &options, 1); // Hard-coded 1.

        if eof || matches.len() == 1 {
            break;
        }
    }

    // SAFETY: restoring the default disposition.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }

    if matches.len() == 1 {
        match matches[0].as_str() {
            STRING_UTIL_CONFIRM_YES_U | STRING_UTIL_CONFIRM_YES => return 1,
            STRING_UTIL_CONFIRM_ALL_U | STRING_UTIL_CONFIRM_ALL => return 2,
            STRING_UTIL_CONFIRM_QUIT => return 3,
            _ => {}
        }
    }

    0
}
