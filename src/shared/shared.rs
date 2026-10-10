//! Rust translation of `libshared src/shared.cpp`, plus the rest of
//! `shared.h`: the `listDiff`/`listIntersect` templates defined inline in the
//! header and the `isIPv4Address`/`isIPv6Address` functions it declares from
//! `ip.cpp`.
//!
//! The C++ functions operate on `std::string` byte strings. These translations
//! keep the same byte semantics: positions and lengths are byte offsets into
//! the UTF-8 encoding, and case folding (`tolower`, `toupper`, `strcasecmp`)
//! only affects ASCII, as it does in the C/UTF-8 locales on the reference
//! platforms. C++ default arguments become separate functions or explicit
//! parameters (noted on each function).
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.

use std::ffi::{CStr, CString};
use std::fmt::Display;
use std::io::{BufRead, Write};

use crate::error::{Result, TwError};
use crate::shared::utf8::{mk_wcwidth, utf8_next_char};

/// The default `edible` set of the C++ `trim`, `ltrim` and `rtrim`.
const DEFAULT_EDIBLE: &str = " \t\n\x0c\r";

/// C++ `std::string::operator[]` on a byte string: index `len` yields the
/// terminating NUL. Indices past the end also yield NUL here.
fn byte_at(input: &[u8], i: usize) -> u8 {
    input.get(i).copied().unwrap_or(0)
}

/// C++ `std::string::substr (pos, n)`: `n` is clamped to the end of the string.
/// `pos` beyond the end yields an empty string (C++ would throw
/// `std::out_of_range`, which no caller here can trigger).
fn substr(input: &[u8], pos: usize, n: usize) -> String {
    let start = pos.min(input.len());
    let end = start.saturating_add(n).min(input.len());
    bytes_to_string(&input[start..end])
}

/// Byte slices taken at the offsets these algorithms compute are always on
/// UTF-8 boundaries for the inputs callers pass; should one not be (e.g. a
/// non-ASCII `edible` set), fall back to lossy conversion rather than panic.
fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// C++ `strerror (errno)` text for an OS error code.
pub(crate) fn strerror(code: i32) -> String {
    // SAFETY: strerror returns a pointer to a NUL-terminated static or
    // thread-local message buffer, which is copied out immediately.
    unsafe { CStr::from_ptr(libc::strerror(code)) }
        .to_string_lossy()
        .into_owned()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `wrapText`: appends to `lines` every line `extract_line` produces from
/// `text` at `width`.
pub fn wrap_text(lines: &mut Vec<String>, text: &str, width: i32, hyphenate: bool) {
    let mut offset = 0;
    while let Some(line) = extract_line(text, width, hyphenate, &mut offset) {
        lines.push(line);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `split (const std::string&, const char)`: split on a separator. Two
/// adjacent separators means empty token. An empty input yields no tokens.
pub fn split(input: &str, delimiter: char) -> Vec<String> {
    if input.is_empty() {
        return Vec::new();
    }

    input.split(delimiter).map(String::from).collect()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `split (const std::string&)`: split on words (`" \t\n\f\r"`). Adjacent
/// separators collapsed.
pub fn split_words(input: &str) -> Vec<String> {
    input
        .split(|c| DEFAULT_EDIBLE.contains(c))
        .filter(|token| !token.is_empty())
        .map(String::from)
        .collect()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `join`, both the `std::vector<int>` and `std::vector<std::string>`
/// overloads: items streamed with `separator` between them.
pub fn join<T: Display>(separator: &str, items: &[T]) -> String {
    let mut s = String::new();
    for (i, item) in items.iter().enumerate() {
        if i != 0 {
            s.push_str(separator);
        }

        s.push_str(&item.to_string());
    }

    s
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `str_replace (str, search, replacement)`: replaces every non-overlapping occurrence of `search`,
/// scanning left to right and resuming after each replacement.
///
/// An empty `search` returns `input` unchanged; the C++ loop never terminates in
/// that case.
pub fn str_replace(input: &str, search: &str, replacement: &str) -> String {
    if search.is_empty() {
        return input.to_string();
    }

    input.replace(search, replacement)
}

/// C++ `std::string::find_first_not_of (edible)`.
fn find_first_not_of(input: &[u8], edible: &[u8]) -> Option<usize> {
    input.iter().position(|b| !edible.contains(b))
}

/// C++ `std::string::find_last_not_of (edible)`.
fn find_last_not_of(input: &[u8], edible: &[u8]) -> Option<usize> {
    input.iter().rposition(|b| !edible.contains(b))
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `trim (input)` with the default edible set `" \t\n\f\r"`.
pub fn trim(input: &str) -> String {
    trim_with(input, DEFAULT_EDIBLE)
}

/// C++ `trim (input, edible)`: strips leading and trailing bytes found in
/// `edible`.
pub fn trim_with(input: &str, edible: &str) -> String {
    let bytes = input.as_bytes();
    let edible = edible.as_bytes();
    let start = find_first_not_of(bytes, edible);
    let end = find_last_not_of(bytes, edible);

    let Some(start) = start else {
        return String::new();
    };

    let Some(end) = end else {
        return substr(bytes, start, usize::MAX);
    };

    substr(bytes, start, end - start + 1)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `ltrim (input)` with the default edible set `" \t\n\f\r"`.
pub fn ltrim(input: &str) -> String {
    ltrim_with(input, DEFAULT_EDIBLE)
}

/// C++ `ltrim (input, edible)`: strips leading bytes found in `edible`.
pub fn ltrim_with(input: &str, edible: &str) -> String {
    let bytes = input.as_bytes();
    let Some(start) = find_first_not_of(bytes, edible.as_bytes()) else {
        return String::new();
    };

    substr(bytes, start, usize::MAX)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `rtrim (input)` with the default edible set `" \t\n\f\r"`.
pub fn rtrim(input: &str) -> String {
    rtrim_with(input, DEFAULT_EDIBLE)
}

/// C++ `rtrim (input, edible)`: strips trailing bytes found in `edible`.
pub fn rtrim_with(input: &str, edible: &str) -> String {
    let bytes = input.as_bytes();
    let edible = edible.as_bytes();
    if find_first_not_of(bytes, edible).is_none() {
        return String::new();
    }

    let Some(end) = find_last_not_of(bytes, edible) else {
        return input.to_string();
    };

    substr(bytes, 0, end + 1)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `longestWord`: the display width of the widest space-separated word.
pub fn longest_word(input: &str) -> i32 {
    longest_run(input, ' ')
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `longestLine`: the display width of the widest newline-separated line.
pub fn longest_line(input: &str) -> i32 {
    longest_run(input, '\n')
}

/// Shared body of C++ `longestWord` and `longestLine`, which differ only in
/// the separator character.
fn longest_run(input: &str, separator: char) -> i32 {
    let mut longest = 0;
    let mut length = 0;
    let mut i = 0;

    loop {
        let character = utf8_next_char(input, &mut i) as i32;
        if character == 0 {
            break;
        }

        if character == separator as i32 {
            if length > longest {
                longest = length;
            }

            length = 0;
        } else {
            length += mk_wcwidth(character);
        }
    }

    if length > longest {
        longest = length;
    }

    longest
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `extractLine (line, text, width, hyphenate, offset)` with the default
/// surrogate `'.'`. See [`extract_line_with_surrogate`].
pub fn extract_line(text: &str, width: i32, hyphenate: bool, offset: &mut usize) -> Option<String> {
    extract_line_with_surrogate(text, width, hyphenate, offset, '.')
}

/// C++ `extractLine`. Walk the input text looking for a break point.  A break
/// point is one of:
///   - EOS
///   - \n
///   - last space before 'length' characters
///   - last punctuation (, ; . :) before 'length' characters, even if not
///     followed by a space
///   - first 'length' characters
///
/// ```text
/// text       "one two three\n  four"
/// bytes       0123456789012 3456789
/// characters  1234567890a23 4567890
///
/// leading_ws
/// ws             ^   ^       ^^
/// punct
/// break                     ^
/// ```
///
/// Returns the extracted line and advances the byte `offset`, or `None` (C++
/// `false`, `line` untouched) once `offset` reaches the end of `text`.
pub fn extract_line_with_surrogate(
    text: &str,
    width: i32,
    hyphenate: bool,
    offset: &mut usize,
    surrogate: char,
) -> Option<String> {
    let bytes_of = text.as_bytes();

    // Terminate processing.
    // Note: bytes vs bytes.
    if *offset >= bytes_of.len() {
        return None;
    }

    let mut last_last_bytes;
    let mut last_bytes = *offset;
    let mut bytes = *offset;
    let mut last_ws: usize = 0;
    let mut line_width = 0;
    let line;
    loop {
        last_last_bytes = last_bytes;
        last_bytes = bytes;
        let character = utf8_next_char(text, &mut bytes) as i32;

        if character == 0 || character == '\n' as i32 {
            line = substr(bytes_of, *offset, last_bytes - *offset);
            *offset = bytes;
            break;
        } else if character == ' ' as i32 {
            last_ws = last_bytes;
        }

        let char_width = mk_wcwidth(character);
        if line_width + char_width > width {
            let last_last_character = byte_at(bytes_of, last_last_bytes);
            let last_character = byte_at(bytes_of, last_bytes);

            // [case 1] one| two --> last_last != 32, last == 32, ws == 0
            if last_last_character != b' ' && last_character == b' ' {
                line = substr(bytes_of, *offset, last_bytes - *offset);
                *offset = last_bytes + 1;
                break;
            }
            // [case 2] one |two --> last_last == 32, last != 32, ws != 0
            else if last_last_character == b' ' && last_character != b' ' && last_ws != 0 {
                line = substr(
                    bytes_of,
                    *offset,
                    (last_bytes - *offset).wrapping_sub(1),
                );
                *offset = last_bytes;
                break;
            } else if last_last_character != b' ' && last_character != b' ' {
                // [case 3] one t|wo --> last_last != 32, last != 32, ws != 0
                if last_ws != 0 {
                    line = substr(bytes_of, *offset, last_ws - *offset);
                    *offset = last_ws + 1;
                }
                // [case 4] on|e two --> last_last != 32, last != 32, ws == 0
                else if char_width > width && last_bytes == *offset {
                    // the first character is already too wide,
                    // no other way to split the line but to replace the character
                    // with a surrogate
                    line = surrogate.to_string();
                    *offset = bytes;
                } else if hyphenate {
                    // C++: `line_width + 1 <= width`.
                    if line_width < width {
                        // if the last good part + hyphen is short enough,
                        // i.e. the just read character is wider than one column
                        line = substr(bytes_of, *offset, last_bytes - *offset) + "-";
                        *offset = last_bytes;
                    } else if last_last_bytes - *offset > 0 {
                        // sacrifice last character from the last good part,
                        // but only if there is at least one character left
                        line = substr(bytes_of, *offset, last_last_bytes - *offset) + "-";
                        *offset = last_last_bytes;
                    } else {
                        // no other way to split the line but to omit the hyphen
                        line = substr(bytes_of, *offset, last_bytes - *offset);
                        *offset = last_bytes;
                    }
                } else {
                    // just use the last good part
                    line = substr(bytes_of, *offset, last_bytes - *offset);
                    *offset = last_bytes;
                }

                break;
            }
        }

        line_width += char_width;
    }

    Some(line)
}

/// The bytes of a C string built from `input.c_str ()`: everything before the
/// first embedded NUL.
fn c_str(input: &str) -> &[u8] {
    let bytes = input.as_bytes();
    match bytes.iter().position(|&b| b == 0) {
        Some(nul) => &bytes[..nul],
        None => bytes,
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `compare (left, right, sensitive = true)`. The case-insensitive form is
/// `strcasecmp` on the C strings: ASCII-only folding, stopping at an embedded
/// NUL.
pub fn compare(left: &str, right: &str, sensitive: bool) -> bool {
    // Use case-insensitive comparison if required.
    if !sensitive {
        return c_str(left).eq_ignore_ascii_case(c_str(right));
    }

    // Otherwise, just use std::string::operator==.
    left == right
}

/// C++ `compare` on byte strings (used where `closeEnough` takes a byte
/// `substr` that need not be valid UTF-8).
fn compare_bytes_insensitive(left: &[u8], right: &[u8]) -> bool {
    let cut = |b: &[u8]| -> usize { b.iter().position(|&c| c == 0).unwrap_or(b.len()) };
    left[..cut(left)].eq_ignore_ascii_case(&right[..cut(right)])
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `closeEnough (reference, attempt, minLength = 0)`: an exact
/// (case-insensitive) match, or `attempt` is a case-insensitive prefix of
/// `reference` at least `min_length` bytes long.
pub fn close_enough(reference: &str, attempt: &str, min_length: u32) -> bool {
    // An exact match is accepted first.
    if compare(reference, attempt, false) {
        return true;
    }

    // A partial match will suffice.
    if attempt.len() < reference.len() && attempt.len() >= min_length as usize {
        return compare_bytes_insensitive(&reference.as_bytes()[..attempt.len()], attempt.as_bytes());
    }

    false
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `matchLength`: the number of leading bytes `left` and `right` share,
/// stopping at the end of either (or an embedded NUL).
pub fn match_length(left: &str, right: &str) -> i32 {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut pos = 0;
    while byte_at(left, pos) != 0 && byte_at(right, pos) != 0 && left[pos] == right[pos] {
        pos += 1;
    }

    pos as i32
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `find (text, pattern, sensitive = true)`: byte offset of `pattern` in
/// `text`, or `None` for `std::string::npos`.
pub fn find(text: &str, pattern: &str, sensitive: bool) -> Option<usize> {
    find_from(text, pattern, 0, sensitive)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `find (text, pattern, begin, sensitive = true)`: byte offset of
/// `pattern` in `text` at or after byte offset `begin`, or `None` for
/// `std::string::npos`.
///
/// Note the C++ quirk preserved here: a case-insensitive search for an empty
/// pattern returns 0 regardless of `begin`.
pub fn find_from(text: &str, pattern: &str, begin: usize, sensitive: bool) -> Option<usize> {
    find_bytes_from(text.as_bytes(), pattern.as_bytes(), begin, sensitive)
}

////////////////////////////////////////////////////////////////////////////////
/// [`find_from`] over raw bytes, for callers (C++ `Task::substitute`) whose
/// `std::string` may hold a byte sequence that is not valid UTF-8 mid-way.
pub fn find_bytes_from(text: &[u8], p: &[u8], begin: usize, sensitive: bool) -> Option<usize> {

    // Implement a sensitive find, which is really just a loop withing a loop,
    // comparing lower-case versions of each character in turn.
    if !sensitive {
        // Handle empty pattern.
        let len = p.len();
        if len == 0 {
            return Some(0);
        }

        // Handle bad begin.
        if begin >= text.len() {
            return None;
        }

        if len > text.len() {
            return None;
        }

        return (begin..=text.len() - len).find(|&t| text[t..t + len].eq_ignore_ascii_case(p));
    }

    // Otherwise, just use std::string::find.
    if begin > text.len() {
        return None;
    }

    if p.is_empty() {
        return Some(begin);
    }

    text[begin..]
        .windows(p.len())
        .position(|window| window == p)
        .map(|pos| pos + begin)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `listDiff` (template in `shared.h`): returns `(leftOnly, rightOnly)`,
/// the elements of each list absent from the other, in original order.
pub fn list_diff<T: PartialEq + Clone>(left: &[T], right: &[T]) -> (Vec<T>, Vec<T>) {
    let left_only = left.iter().filter(|l| !right.contains(l)).cloned().collect();
    let right_only = right.iter().filter(|r| !left.contains(r)).cloned().collect();
    (left_only, right_only)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `listIntersect` (template in `shared.h`): elements of `left` also
/// present in `right`, in `left` order.
pub fn list_intersect<T: PartialEq + Clone>(left: &[T], right: &[T]) -> Vec<T> {
    left.iter().filter(|l| right.contains(l)).cloned().collect()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `lowerCase`: `tolower` applied to every byte (ASCII only).
pub fn lower_case(input: &str) -> String {
    input.to_ascii_lowercase()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `upperCase`: `toupper` applied to every byte (ASCII only).
pub fn upper_case(input: &str) -> String {
    input.to_ascii_uppercase()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `upperCaseFirst`: `toupper` applied to the first byte (ASCII only).
pub fn upper_case_first(input: &str) -> String {
    let mut output = input.to_string();
    if let Some(first) = output.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    output
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `autoComplete (partial, list, matches, minimum = 1)`: returns the
/// matches (the C++ return value is their count).
///
/// An exact match is returned alone, immediately. Otherwise every item that
/// `partial` is a byte prefix of is returned, provided `partial` is at least
/// `minimum` bytes long (a negative `minimum`, cast to `unsigned` in C++,
/// disables partial matching).
pub fn auto_complete<S: AsRef<str>>(partial: &str, list: &[S], minimum: i32) -> Vec<String> {
    let mut matches = Vec::new();

    // Handle trivial case.
    let length = partial.len();
    if length != 0 {
        for item in list {
            let item = item.as_ref();

            // An exact match is a special case.  Assume there is only one exact match
            // and return immediately.
            if partial == item {
                return vec![item.to_string()];
            }
            // Maintain a list of partial matches.
            else if length as u64 >= u64::from(minimum as u32)
                && length <= item.len()
                && item.as_bytes().starts_with(partial.as_bytes())
            {
                matches.push(item.to_string());
            }
        }
    }

    matches
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `signal_handler` installed by `confirm`.
extern "C" fn confirm_signal_handler(s: libc::c_int) {
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

/// C++ `confirm`: asks `question` with a ` (yes/no) ` suffix until the answer
/// autocompletes to exactly one of `yes`/`no`. End of input counts as `no`.
/// SIGINT while waiting prints `Interrupted: No changes made.` and exits 1.
///
/// Uses whole-line reads, because a newline must be detected so the prompt can
/// be re-written (the C++ `std::getline` rationale).
pub fn confirm(question: &str) -> bool {
    let options = ["yes", "no"];
    let mut matches;

    let handler: extern "C" fn(libc::c_int) = confirm_signal_handler;
    // SAFETY: installing a handler that only calls async-signal-safe functions.
    unsafe {
        libc::signal(libc::SIGINT, handler as libc::sighandler_t);
    }

    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    let mut eof = false;
    loop {
        let mut stdout = std::io::stdout();
        let _ = write!(stdout, "{question} (yes/no) ");
        let _ = stdout.flush();

        // std::getline: reads up to '\n' (discarded); hitting end of input
        // first sets eof.
        let mut buffer = Vec::new();
        match stdin.read_until(b'\n', &mut buffer) {
            Ok(_) if buffer.last() == Some(&b'\n') => {
                buffer.pop();
            }
            _ => eof = true,
        }

        let answer = if eof {
            "no".to_string()
        } else {
            lower_case(&trim(&String::from_utf8_lossy(&buffer)))
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
    matches.len() == 1 && matches[0] == "yes"
}

/// C++ `std::string::c_str ()` as a `CString`: everything before the first
/// embedded NUL.
fn c_string(input: &str) -> CString {
    CString::new(c_str(input)).expect("no interior NUL after truncation")
}

/// The current `errno`.
fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `errno` values the C++ retries on.
fn is_retryable(err: i32) -> bool {
    err == libc::EINTR || err == libc::EAGAIN || err == libc::EWOULDBLOCK
}

/// C++ `throw std::string (std::strerror (errno))`.
fn errno_error() -> TwError {
    TwError::Message(strerror(errno()))
}

/// C++ `close (fd)` where the C++ ignores the result.
fn close_fd(fd: libc::c_int) {
    // SAFETY: closing a descriptor this function owns.
    unsafe {
        libc::close(fd);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `execute (executable, args, input, output)`: run a binary with args,
/// capturing output. Returns `(exit status, output)`.
///
/// A direct translation of the POSIX branch: `fork`, `execvp` (so `executable`
/// is searched on `PATH`), `input` written to the child's stdin and its stdout
/// read through a `select` loop; the child's stderr is inherited. In
/// particular:
/// - SIGPIPE is ignored while the child runs (handled locally with EPIPE: a
///   child that exits without reading all of its input is not an error) and
///   reset to the default disposition afterwards.
/// - If `execvp` fails the child writes `Failed to execute '<executable>'
///   Error: <strerror>` to stderr and exits with status 255 (`_exit (-1)`).
/// - Each chunk read from the child is appended up to its first NUL byte (the
///   C++ appends a NUL-terminated buffer). Output that is not valid UTF-8 is
///   converted lossily.
/// - Pipe, fork, select, read, write and wait failures, and a child that does
///   not exit normally (e.g. is killed by a signal), are errors (the C++
///   `throw std::string`).
///
/// The pipe descriptors are additionally marked close-on-exec, so concurrent
/// `execute` calls from other threads cannot leak them into unrelated children
/// (the C++ program is single-threaded); the child's stdin/stdout are set with
/// `dup2`, which clears the flag on the new descriptors.
pub fn execute<S: AsRef<str>>(executable: &str, args: &[S], input: &str) -> Result<(i32, String)> {
    let input_bytes = input.as_bytes();

    // Prepare everything the child needs before forking, so the child only
    // calls async-signal-safe functions.
    let exe = c_string(executable);
    let arg_strings: Vec<CString> = args.iter().map(|arg| c_string(arg.as_ref())).collect();
    // Add executable as argv[0] and NULL-terminate the array for execvp().
    let mut argv: Vec<*const libc::c_char> = Vec::with_capacity(args.len() + 2);
    argv.push(exe.as_ptr());
    argv.extend(arg_strings.iter().map(|arg| arg.as_ptr()));
    argv.push(std::ptr::null());
    let failure_prefix = format!("Failed to execute '{executable}' Error: ");

    // SAFETY: changing the SIGPIPE disposition.
    if unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) } == libc::SIG_ERR {
        // Handled locally with EPIPE.
        return Err(errno_error());
    }

    let mut pin = [0 as libc::c_int; 2];
    let mut pout = [0 as libc::c_int; 2];
    // SAFETY: pipe() writes two descriptors into the provided array.
    if unsafe { libc::pipe(pin.as_mut_ptr()) } == -1 {
        return Err(errno_error());
    }

    // SAFETY: as above.
    if unsafe { libc::pipe(pout.as_mut_ptr()) } == -1 {
        let error = errno_error();
        close_fd(pin[0]);
        close_fd(pin[1]);
        return Err(error);
    }

    for fd in pin.iter().chain(pout.iter()) {
        // SAFETY: setting a descriptor flag on a descriptor this function owns.
        unsafe {
            libc::fcntl(*fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }

    // SAFETY: the child branch below only uses async-signal-safe calls
    // (close, dup2, execvp, write, strerror on a valid errno, _exit) on data
    // prepared before the fork.
    let pid = unsafe { libc::fork() };
    if pid == -1 {
        let error = errno_error();
        for fd in pin.iter().chain(pout.iter()) {
            close_fd(*fd);
        }
        return Err(error);
    }

    if pid == 0 {
        // This is only reached in the child
        // SAFETY: see the fork() comment.
        unsafe {
            libc::close(pin[1]); // Close the write end of the input pipe.
            libc::close(pout[0]); // Close the read end of the output pipe.

            // Parent writes to pin[1]. Set read end pin[0] as STDIN for child.
            // (The C++ throws from the child if dup2 fails; here the child
            // exits as for a failed exec.)
            if libc::dup2(pin[0], libc::STDIN_FILENO) == -1 {
                libc::_exit(-1);
            }
            libc::close(pin[0]);

            // Parent reads from pout[0]. Set write end pout[1] as STDOUT for child.
            if libc::dup2(pout[1], libc::STDOUT_FILENO) == -1 {
                libc::_exit(-1);
            }
            libc::close(pout[1]);

            let rc = libc::execvp(exe.as_ptr(), argv.as_ptr());
            let reason = CStr::from_ptr(libc::strerror(errno())).to_bytes();
            libc::write(libc::STDERR_FILENO, failure_prefix.as_ptr().cast(), failure_prefix.len());
            libc::write(libc::STDERR_FILENO, reason.as_ptr().cast(), reason.len());
            libc::write(libc::STDERR_FILENO, b"\n".as_ptr().cast(), 1);
            libc::_exit(rc);
        }
    }

    // This is only reached in the parent
    close_fd(pin[0]); // Close the read end of the input pipe.
    close_fd(pout[1]); // Close the write end of the output pipe.

    let result = execute_parent(pin[1], pout[0], input_bytes);
    close_fd(pout[0]); // Close the read end of the output pipe.
    let output = result?;

    let mut status: libc::c_int = -1;
    let wait_retval = loop {
        // SAFETY: waiting on the child forked above.
        let retval = unsafe { libc::waitpid(pid, &mut status, 0) };
        if !(retval == -1 && is_retryable(errno())) {
            break retval;
        }
    };
    if wait_retval == -1 {
        return Err(errno_error());
    }

    let status = if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        return Err(TwError::Message("Error: Could not get Hook exit status!".to_string()));
    };

    // SAFETY: We're done, return to default.
    if unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) } == libc::SIG_ERR {
        return Err(errno_error());
    }

    Ok((status, bytes_to_string(&output)))
}

/// The parent's half of C++ `execute`: the `select` loop that writes `input`
/// to `pin_write` and reads the child's output from `pout_read` until the
/// child closes its stdout. Closes `pin_write` once all input is written (or
/// immediately, when there is none).
fn execute_parent(pin_write: libc::c_int, pout_read: libc::c_int, input: &[u8]) -> Result<Vec<u8>> {
    let mut pin_open = true;
    let close_input = |pin_open: &mut bool| {
        if *pin_open {
            close_fd(pin_write);
            *pin_open = false;
        }
    };

    if input.is_empty() {
        // Nothing to send to the child, close the pipe early.
        close_input(&mut pin_open);
    }

    let result = (|| -> Result<Vec<u8>> {
        let mut output = Vec::new();
        let mut buf = [0u8; 16384];
        let mut read_retval: isize = -1;
        let mut written: usize = 0;
        while read_retval != 0 || input.len() != written {
            // SAFETY: fd_set is plain data; FD_ZERO/FD_SET/FD_ISSET operate
            // on descriptors below FD_SETSIZE owned by this function.
            let mut rfds: libc::fd_set = unsafe { std::mem::zeroed() };
            let mut wfds: libc::fd_set = unsafe { std::mem::zeroed() };
            unsafe {
                libc::FD_ZERO(&mut rfds);
                if read_retval != 0 {
                    libc::FD_SET(pout_read, &mut rfds);
                }

                libc::FD_ZERO(&mut wfds);
                if input.len() != written {
                    libc::FD_SET(pin_write, &mut wfds);
                }
            }

            // On Linux, tv may be overwritten by select().  Reset it each time.
            // NOTE: Timeout chosen arbitrarily - we don't time out execute() calls.
            // select() is run over and over again unless the child exits or closes
            // its pipes.
            let mut tv = libc::timeval { tv_sec: 5, tv_usec: 0 };

            // SAFETY: valid fd_set and timeval pointers.
            let select_retval = unsafe {
                libc::select(
                    pout_read.max(pin_write) + 1,
                    &mut rfds,
                    &mut wfds,
                    std::ptr::null_mut(),
                    &mut tv,
                )
            };

            if select_retval == -1 {
                // Haiku (and probably some very old-school UNIXs) fails with EINTR or
                // EAGAIN when SIGCHLD arrives during a blocking call. Retrying fixes it.
                if is_retryable(errno()) {
                    continue;
                }
                return Err(errno_error());
            }

            // Write data to child's STDIN
            // SAFETY: wfds was filled by select above.
            if input.len() != written && unsafe { libc::FD_ISSET(pin_write, &wfds) } {
                let remaining = &input[written..];
                // SAFETY: writing from a valid buffer of the given length.
                let mut write_retval =
                    unsafe { libc::write(pin_write, remaining.as_ptr().cast(), remaining.len()) };
                if write_retval == -1 {
                    let err = errno();
                    if err == libc::EPIPE {
                        // Child died (or closed the pipe) before reading all input.
                        // We don't really care; pretend we wrote it all.
                        write_retval = remaining.len() as isize;
                    } else if is_retryable(err) {
                        // Interrupted, try again.
                        write_retval = 0;
                    } else {
                        return Err(errno_error());
                    }
                }
                written += write_retval as usize;

                if written == input.len() {
                    // Let the child know that no more input is coming by closing the pipe.
                    close_input(&mut pin_open);
                }
            }

            // Read data from child's STDOUT
            // SAFETY: rfds was filled by select above.
            if read_retval != 0 && unsafe { libc::FD_ISSET(pout_read, &rfds) } {
                // SAFETY: reading into a valid buffer, leaving room for the C++ NUL.
                read_retval = unsafe { libc::read(pout_read, buf.as_mut_ptr().cast(), buf.len() - 1) };
                if read_retval == -1 {
                    if is_retryable(errno()) {
                        continue;
                    }
                    return Err(errno_error());
                }

                // buf[read_retval] = '\0'; output += buf;
                let chunk = &buf[..read_retval as usize];
                let end = chunk.iter().position(|&b| b == 0).unwrap_or(chunk.len());
                output.extend_from_slice(&chunk[..end]);
            }
        }

        Ok(output)
    })();

    close_input(&mut pin_open);
    result
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `osName`: the platform name selected at build time (the C++ build
/// defines one of `DARWIN`, `SOLARIS`, … via CMake). There is no Rust target
/// for GNU/kFreeBSD, so `"GNU/kFreeBSD"` cannot be produced.
pub fn os_name() -> String {
    let name = if cfg!(target_os = "macos") {
        "Darwin"
    } else if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        "Solaris"
    } else if cfg!(target_os = "cygwin") {
        "Cygwin"
    } else if cfg!(target_os = "haiku") {
        "Haiku"
    } else if cfg!(target_os = "openbsd") {
        "OpenBSD"
    } else if cfg!(target_os = "freebsd") {
        "FreeBSD"
    } else if cfg!(target_os = "netbsd") {
        "NetBSD"
    } else if cfg!(target_os = "dragonfly") {
        "Dragonfly"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "hurd") {
        "GNU/Hurd"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "<unknown>"
    };

    name.to_string()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `cppCompliance`: the C++ standard level the binary was compiled
/// against, as reported by `task diagnostics`.
///
/// There is no C++ compiler in this build. The reference Taskwarrior 3.5.0 is
/// built with `CMAKE_CXX_STANDARD 17`, so `__cplusplus` is `201703L` and the
/// C++ function returns `"C++17"`; that value is reproduced here.
pub fn cpp_compliance() -> String {
    "C++17".to_string()
}

// ip.cpp ----------------------------------------------------------------------

/// C++ `std::isdigit` on a byte.
fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

/// C++ `std::stoi` on a run of ASCII digits (at most five here, so no
/// overflow).
fn stoi_digits(digits: &[u8]) -> i32 {
    digits
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .fold(0i32, |n, &b| n * 10 + i32::from(b - b'0'))
}

////////////////////////////////////////////////////////////////////////////////
/// `ip.cpp` `isPort`: one to five digits.
fn is_port(input: &[u8], c: &mut usize) -> bool {
    let start = *c;
    while is_digit(byte_at(input, *c)) {
        *c += 1;
    }

    *c - start > 0 && *c - start < 6
}

////////////////////////////////////////////////////////////////////////////////
/// `ip.cpp` `isChar`.
fn is_char(input: &[u8], character: u8, c: &mut usize) -> bool {
    if byte_at(input, *c) == character {
        *c += 1;
        return true;
    }

    false
}

////////////////////////////////////////////////////////////////////////////////
/// `ip.cpp` `isEOS`.
fn is_eos(input: &[u8], c: usize) -> bool {
    c >= input.len()
}

////////////////////////////////////////////////////////////////////////////////
/// `ip.cpp` `isIPv4Block`: one to three digits with a value below 256.
fn is_ipv4_block(input: &[u8], c: &mut usize) -> bool {
    let start = *c;
    while is_digit(byte_at(input, *c)) {
        *c += 1;
    }

    if *c - start > 0 && *c - start < 4 {
        let byte = stoi_digits(&input[start..*c]);
        if byte < 256 {
            return true;
        }
    }

    *c = start;
    false
}

////////////////////////////////////////////////////////////////////////////////
/// `ip.cpp` `isIPv4BlockSet`: four dot-separated blocks.
fn is_ipv4_block_set(input: &[u8], c: &mut usize) -> bool {
    let start = *c;

    if is_ipv4_block(input, c)
        && is_char(input, b'.', c)
        && is_ipv4_block(input, c)
        && is_char(input, b'.', c)
        && is_ipv4_block(input, c)
        && is_char(input, b'.', c)
        && is_ipv4_block(input, c)
    {
        return true;
    }

    *c = start;
    false
}

////////////////////////////////////////////////////////////////////////////////
/// `ip.cpp` `isIPv6Block`: one to four hex digits.
fn is_ipv6_block(input: &[u8], c: &mut usize) -> bool {
    let start = *c;
    while byte_at(input, *c).is_ascii_hexdigit() {
        *c += 1;
    }

    if *c - start > 0 && *c - start < 5 {
        return true;
    }

    *c = start;
    false
}

////////////////////////////////////////////////////////////////////////////////
/// `ip.cpp` `isIPv6BlockSet`:
/// at least one non-empty block, or '::'
/// at least two colons
/// 8 or less blocks
fn is_ipv6_block_set(input: &[u8], c: &mut usize) -> bool {
    let start = *c;
    let mut count_colons = 0;
    let mut count_blocks = 0;

    loop {
        if is_eos(input, *c) {
            if *c == start {
                return false;
            }

            break;
        } else if is_char(input, b':', c) {
            count_colons += 1;
        } else if is_ipv4_block_set(input, c) || is_ipv6_block(input, c) {
            count_blocks += 1;
        } else if is_char(input, b'.', c) || is_char(input, b']', c) {
            *c -= 1;
            break;
        } else {
            break;
        }
    }

    // Note: the C++ compares `input.substr (start, c)`, i.e. `c` bytes from
    // `start`, not the range `start..c`.
    if (2..=7).contains(&count_colons)
        && ((count_blocks == 0 && substr(input, start, *c) == "::") || count_blocks >= 1)
        && count_blocks <= 8
    {
        return true;
    }

    *c = start;
    false
}

////////////////////////////////////////////////////////////////////////////////
/// `ip.cpp` `isIPv4Address`: recognizes
///   `<address>:<port>`
///   `<address>`
///
/// Returns `(address, port)` (port 0 when absent), or `None` (C++ `false`,
/// outputs untouched).
pub fn is_ipv4_address(input: &str) -> Option<(String, i32)> {
    let bytes = input.as_bytes();
    let mut c = 0;
    if is_ipv4_block_set(bytes, &mut c) {
        let colon = c;
        if is_char(bytes, b':', &mut c) && !is_port(bytes, &mut c) {
            return None;
        }

        if is_eos(bytes, c) {
            let address = substr(bytes, 0, c.min(colon));
            let mut port = 0;
            if !is_eos(bytes, colon) {
                port = stoi_digits(&bytes[colon + 1..]);
            }

            return Some((address, port));
        }
    }

    None
}

////////////////////////////////////////////////////////////////////////////////
/// `ip.cpp` `isIPv6Address`: recognizes
///   `[<address>]:<port>`
///   `<address>`
///
/// Returns `(address, port)` (port 0 when absent), or `None` (C++ `false`,
/// outputs untouched).
pub fn is_ipv6_address(input: &str) -> Option<(String, i32)> {
    let bytes = input.as_bytes();
    let mut c = 0;

    if is_char(bytes, b'[', &mut c) && is_ipv6_block_set(bytes, &mut c) && is_char(bytes, b']', &mut c) {
        let colon = c;
        if is_char(bytes, b':', &mut c) && is_port(bytes, &mut c) && is_eos(bytes, c) {
            let address = substr(bytes, 1, colon - 2);
            let port = stoi_digits(&bytes[colon + 1..]);
            return Some((address, port));
        }
    }

    c = 0;
    if is_ipv6_block_set(bytes, &mut c) && is_eos(bytes, c) {
        return Some((input.to_string(), 0));
    }

    None
}

////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    //! Ports of libshared `test/shared.t.cpp`, plus `test/autocomplete.t.cpp`
    //! and `test/list.t.cpp`, which also exercise functions declared in
    //! `shared.h`. Each C++ `t.is`/`t.ok`/`t.notok` is one assertion here,
    //! carrying the C++ test description as its message.

    use super::*;

    fn wrap(text: &str, width: i32, hyphenate: bool) -> Vec<String> {
        let mut lines = Vec::new();
        wrap_text(&mut lines, text, width, hyphenate);
        lines
    }

    #[test]
    fn wrap_text_cases() {
        // void wrapText (std::vector <std::string>& lines, const std::string& text, const int width, bool hyphenate)
        let lines = wrap("This is a test of the line wrapping code.", 10, true);
        assert_eq!(lines.len(), 5, "wrapText 'This is a test of the line wrapping code.' -> total 5 lines");
        assert_eq!(lines[0], "This is a", "wrapText line 0 -> 'This is a'");
        assert_eq!(lines[1], "test of", "wrapText line 1 -> 'test of'");
        assert_eq!(lines[2], "the line", "wrapText line 2 -> 'the line'");
        assert_eq!(lines[3], "wrapping", "wrapText line 3 -> 'wrapping'");
        assert_eq!(lines[4], "code.", "wrapText line 4 -> 'code.'");

        let lines = wrap("This ☺ is a test of utf8 line extraction.", 7, true);
        assert_eq!(lines.len(), 7, "wrapText 'This ☺ is a test of utf8 line extraction.' -> total 7 lines");
        assert_eq!(lines[0], "This ☺", "wrapText line 0 -> 'This ☺'");
        assert_eq!(lines[1], "is a", "wrapText line 1 -> 'is a'");
        assert_eq!(lines[2], "test of", "wrapText line 2 -> 'test of'");
        assert_eq!(lines[3], "utf8", "wrapText line 3 -> 'utf8'");
        assert_eq!(lines[4], "line", "wrapText line 4 -> 'line'");
        assert_eq!(lines[5], "extrac-", "wrapText line 5 -> 'extrac-'");
        assert_eq!(lines[6], "tion.", "wrapText line 6 -> 'tion.'");

        let lines = wrap("one two three\n  four", 13, true);
        assert_eq!(lines.len(), 2, "wrapText 'one two three\\n  four' -> 2 lines");
        assert_eq!(lines[0], "one two three", "wrapText line 0 -> 'one two three'");
        assert_eq!(lines[1], "  four", "wrapText line 1 -> '  four'");

        // wrapText appends to the caller's vector.
        let mut lines = vec!["existing".to_string()];
        wrap_text(&mut lines, "abc", 10, true);
        assert_eq!(lines, vec!["existing", "abc"]);
    }

    #[test]
    fn extract_line_cases() {
        // void extractLine (std::string& text, std::string& line, int length, bool hyphenate, unsigned int& offset)
        let text = "This ☺ is a test of utf8 line extraction.";
        let mut offset = 0;
        let line = extract_line(text, 7, true, &mut offset);
        assert_eq!(line.as_deref(), Some("This ☺"), "extractLine 7 'This ☺ is a test of utf8 line extraction.' -> 'This ☺'");

        let text = "line 1\nlengthy second line that exceeds width";
        let mut offset = 0;
        let line = extract_line(text, 10, true, &mut offset);
        assert_eq!(line.as_deref(), Some("line 1"), "extractLine 10 'line 1\\nlengthy second line that exceeds width' -> 'line 1'");

        let line = extract_line(text, 10, true, &mut offset);
        assert_eq!(line.as_deref(), Some("lengthy"), "extractLine 10 'lengthy second line that exceeds width' -> 'lengthy'");

        let line = extract_line(text, 10, true, &mut offset);
        assert_eq!(line.as_deref(), Some("second"), "extractLine 10 'second line that exceeds width' -> 'second'");

        let line = extract_line(text, 10, true, &mut offset);
        assert_eq!(line.as_deref(), Some("line that"), "extractLine 10 'line that exceeds width' -> 'line that'");

        let line = extract_line(text, 10, true, &mut offset);
        assert_eq!(line.as_deref(), Some("exceeds"), "extractLine 10 'exceeds width' -> 'exceeds'");

        let line = extract_line(text, 10, true, &mut offset);
        assert_eq!(line.as_deref(), Some("width"), "extractLine 10 'width' -> 'width'");

        assert!(extract_line(text, 10, true, &mut offset).is_none(), "extractLine 10 '' -> ''");

        let text = "AAAAAAAAAABBBBBBBBBB";
        let mut offset = 0;
        let line = extract_line(text, 10, true, &mut offset);
        assert_eq!(line.as_deref(), Some("AAAAAAAAA-"), "extractLine hyphenated unbreakable line");

        let text = "abc\n";
        let mut offset = 3;
        let line = extract_line(text, 10, true, &mut offset);
        assert!(line.is_some(), "extractLine succeeds at trailing newline");
        assert_eq!(line.as_deref(), Some(""), "recognized newline as a line ending");
        assert_eq!(offset, 4, "incremented offset");

        let text = "abc\0";
        let mut offset = 3;
        assert_eq!(text.len(), 4, "trailing NUL included in string length");
        let line = extract_line(text, 10, true, &mut offset);
        assert!(line.is_some(), "extractLine succeeds at NUL");
        assert_eq!(line.as_deref(), Some(""), "recognized NUL as a line ending");
        assert_eq!(offset, 4, "incremented ofset");
    }

    #[test]
    fn extract_line_branches() {
        // Not in shared.t.cpp: the remaining case-4 branches.

        // Without hyphenation, the last good part is used as is.
        let mut offset = 0;
        assert_eq!(extract_line("AAAAAAAAAABB", 10, false, &mut offset).as_deref(), Some("AAAAAAAAAA"));
        assert_eq!(offset, 10);

        // A first character wider than the width is replaced by the surrogate.
        let mut offset = 0;
        assert_eq!(extract_line("\u{4e00}x", 1, true, &mut offset).as_deref(), Some("."));
        assert_eq!(offset, 3);
        let mut offset = 0;
        assert_eq!(extract_line_with_surrogate("\u{4e00}x", 1, true, &mut offset, '?').as_deref(), Some("?"));

        // A wide character that does not fit, with room left for the hyphen.
        let mut offset = 0;
        assert_eq!(extract_line("AA\u{4e00}", 3, true, &mut offset).as_deref(), Some("AA-"));
        assert_eq!(offset, 2);
    }

    #[test]
    fn split_on_delimiter() {
        // std::vector <std::string> split (const std::string& input, const char delimiter)
        let items = split("", '-');
        assert_eq!(items.len(), 0, "split '' '-' -> 0 items");

        let items = split("a", '-');
        assert_eq!(items.len(), 1, "split 'a' '-' -> 1 item");
        assert_eq!(items[0], "a", "split 'a' '-' -> 'a'");

        let items = split("a", '-');
        assert_eq!(items.len(), 1, "split 'a' '-' -> 1 item");
        assert_eq!(items[0], "a", "split 'a' '-' -> 'a'");

        let items = split("-", '-');
        assert_eq!(items.len(), 2, "split '-' '-' -> '' ''");
        assert_eq!(items[0], "", "split '-' '-' -> [0] ''");
        assert_eq!(items[1], "", "split '-' '-' -> [1] ''");

        let items = split("-a-bc--def", '-');
        assert_eq!(items.len(), 5, "split '-a-bc--def' '-' -> '' 'a' 'bc' '' 'def'");
        assert_eq!(items[0], "", "split '-a-bc--def' '-' -> [0] ''");
        assert_eq!(items[1], "a", "split '-a-bc--def' '-' -> [1] 'a'");
        assert_eq!(items[2], "bc", "split '-a-bc--def' '-' -> [2] 'bc'");
        assert_eq!(items[3], "", "split '-a-bc--def' '-' -> [3] ''");
        assert_eq!(items[4], "def", "split '-a-bc--def' '-' -> [4] 'def'");
    }

    #[test]
    fn split_on_words() {
        // std::vector <std::string> split (const std::string& input);
        let items = split_words("");
        assert_eq!(items.len(), 0, "split '' -> 0 items");

        let items = split_words("abc");
        assert_eq!(items.len(), 1, "split 'abc' -> 1 item");
        assert_eq!(items[0], "abc", "split 'abc' -> [0] 'abc'");

        let items = split_words("a b c");
        assert_eq!(items.len(), 3, "split 'a b c' -> 3 items");
        assert_eq!(items[0], "a", "split 'a b c' -> [0] 'a'");
        assert_eq!(items[1], "b", "split 'a b c' -> [1] 'b'");
        assert_eq!(items[2], "c", "split 'a b c' -> [2] 'c'");

        let items = split_words("  a   b   c  ");
        assert_eq!(items.len(), 3, "split '  a  b  c  ' -> 3 items");
        assert_eq!(items[0], "a", "split '  a  b  c  ' -> [0] 'a'");
        assert_eq!(items[1], "b", "split '  a  b  c  ' -> [1] 'b'");
        assert_eq!(items[2], "c", "split '  a  b  c  ' -> [2] 'c'");
    }

    #[test]
    fn join_cases() {
        // std::string join (const std::string&r, const std::vector<int>&)
        // std::string join (const std::string&r, const std::vector<std::string>&)
        let unjoined: Vec<String> = Vec::new();
        let joined = join("", &unjoined);
        assert_eq!(joined.len(), 0, "join -> length 0");
        assert_eq!(joined, "", "join -> ''");

        let unjoined = ["", "a", "bc", "def"];
        let joined = join("", &unjoined);
        assert_eq!(joined.len(), 6, "join '' 'a' 'bc' 'def' -> length 6");
        assert_eq!(joined, "abcdef", "join '' 'a' 'bc' 'def' -> 'abcdef'");

        let joined = join("-", &unjoined);
        assert_eq!(joined.len(), 9, "join '' - 'a' - 'bc' - 'def' -> length 9");
        assert_eq!(joined, "-a-bc-def", "join '' - 'a' - 'bc' - 'def' -> '-a-bc-def'");

        let unjoined_ints = [1, 2, 3, 4];
        assert_eq!(join("-", &unjoined_ints), "1-2-3-4", "join 1 - 2 - 3 - 4 -> '1-2-3-4'");
    }

    #[test]
    fn trim_cases() {
        // std::string trim (const std::string&);
        assert_eq!(trim("one"), "one", "trim 'one' --> 'one'");
        assert_eq!(trim("  one"), "one", "trim '  one' --> 'one'");
        assert_eq!(trim("one  "), "one", "trim 'one  ' --> 'one'");
        assert_eq!(trim("  one  "), "one", "trim '  one  ' --> 'one'");
        assert_eq!(trim(""), "", "trim '' --> ''");
        assert_eq!(trim(" \t\r\x0c\nfoo\n\x0c\r\t "), "foo", "trim ' \\t\\r\\f\\nfoo\\n\\f\\r\\t ' --> 'foo'");

        assert_eq!(trim_with("abcdedcba", "abc"), "ded", "trim 'abcdedcba', 'abc' --> 'ded'");
    }

    #[test]
    fn ltrim_cases() {
        // std::string ltrim (const std::string&);
        assert_eq!(ltrim("one"), "one", "ltrim 'one' --> 'one'");
        assert_eq!(ltrim("  one"), "one", "ltrim '  one' --> 'one'");
        assert_eq!(ltrim("one  "), "one  ", "ltrim 'one  ' --> 'one  '");
        assert_eq!(ltrim("  one  "), "one  ", "ltrim '  one  ' --> 'one  '");
        assert_eq!(ltrim(""), "", "ltrim '' --> ''");
        assert_eq!(ltrim("  "), "", "ltrim '  ' --> ''");
        assert_eq!(
            ltrim(" \t\r\x0c\nfoo\n\x0c\r\t "),
            "foo\n\x0c\r\t ",
            "ltrim ' \\t\\r\\f\\nfoo\\n\\f\\r\\t ' --> 'foo\\n\\f\\r\\t '"
        );

        // Not in shared.t.cpp: explicit edible set.
        assert_eq!(ltrim_with("xxaxx", "x"), "axx");
    }

    #[test]
    fn rtrim_cases() {
        // std::string rtrim (const std::string&);
        assert_eq!(rtrim("one"), "one", "rtrim 'one' --> 'one'");
        assert_eq!(rtrim("  one"), "  one", "rtrim '  one' --> '  one'");
        assert_eq!(rtrim("one  "), "one", "rtrim 'one  ' --> 'one'");
        assert_eq!(rtrim("  one  "), "  one", "rtrim '  one' --> '  one'");
        assert_eq!(rtrim(""), "", "rtrim '' --> ''");
        assert_eq!(rtrim("  "), "", "rtrim '  ' --> ''");
        assert_eq!(
            rtrim(" \t\r\x0c\nfoo\n\x0c\r\t "),
            " \t\r\x0c\nfoo",
            "rtrim ' \\t\\r\\f\\nfoo\\n\\f\\r\\t ' --> ' \\t\\r\\f\\nfoo'"
        );

        // Not in shared.t.cpp: explicit edible set.
        assert_eq!(rtrim_with("xxaxx", "x"), "xxa");
    }

    #[test]
    fn longest_word_cases() {
        // int longestWord (const std::string&)
        assert_eq!(longest_word("    "), 0, "longestWord (    ) --> 0");
        assert_eq!(longest_word("this is a test"), 4, "longestWord (this is a test) --> 4");
        assert_eq!(longest_word("this is a better test"), 6, "longestWord (this is a better test) --> 6");
        assert_eq!(longest_word("house Çirçös clown"), 6, "longestWord (Çirçös) --> 6");
    }

    #[test]
    fn longest_line_cases() {
        // int longestLine (const std::string&)
        assert_eq!(longest_line("one two three four"), 18, "longestLine (one two three four) --> 18");
        assert_eq!(longest_line("one\ntwo three four"), 14, "longestLine (one\\ntwo three four) --> 14");
        assert_eq!(longest_line("one\ntwo\nthree\nfour"), 5, "longestLine (one\\ntwo\\nthree\\nfour) --> 5");
    }

    #[test]
    fn compare_cases() {
        // bool compare (const std::string&, const std::string&, bool sensitive = true);
        assert!(!compare("a", "b", true), "compare 'a' : 'b' --> false");
        assert!(!compare("a", "", true), "compare 'a' : ''  --> false");
        assert!(!compare("", "b", true), "compare ''  : 'b' --> false");
        assert!(compare("", "", true), "compare ''  : ''  --> true");
        assert!(!compare("A", "a", true), "compare            'A' : 'a' --> false");
        assert!(!compare("A", "a", true), "compare sensitive  'A' : 'a' --> false");
        assert!(compare("A", "a", false), "compare !sensitive 'A' : 'a' --> true");

        // Not in shared.t.cpp: strcasecmp stops at an embedded NUL.
        assert!(compare("ab\0x", "AB\0y", false));
        assert!(!compare("ab\0x", "ab\0y", true));
    }

    #[test]
    fn close_enough_cases() {
        // bool closeEnough (const std::string&, const std::string&, unsigned int minLength = 0);
        assert!(close_enough("foobar", "foobar", 0), "closeEnough foobar == foobar");
        assert!(close_enough("foobar", "foobar", 0), "closeEnough foobar == foobar,0");
        assert!(close_enough("foobar", "foobar", 1), "closeEnough foobar == foobar,1");
        assert!(close_enough("foobar", "foobar", 2), "closeEnough foobar == foobar,2");
        assert!(close_enough("foobar", "foobar", 3), "closeEnough foobar == foobar,3");
        assert!(close_enough("foobar", "foobar", 4), "closeEnough foobar == foobar,4");
        assert!(close_enough("foobar", "foobar", 5), "closeEnough foobar == foobar,5");
        assert!(close_enough("foobar", "foobar", 6), "closeEnough foobar == foobar,6");
        assert!(close_enough("foobar", "foo", 3), "closeEnough foobar == foo,3");

        // Not in shared.t.cpp: below minimum, case folding, non-prefix.
        assert!(!close_enough("foobar", "fo", 3));
        assert!(close_enough("foobar", "FOO", 3));
        assert!(!close_enough("foobar", "bar", 0));
    }

    #[test]
    fn match_length_cases() {
        // int matchLength (const std::string&, const std::string&, unsigned int minLength = 0);
        assert_eq!(match_length("", "foobar"), 0, "matchLength '' == foobar --> 0");
        assert_eq!(match_length("f", "foobar"), 1, "matchLength f == foobar --> 1");
        assert_eq!(match_length("fo", "foobar"), 2, "matchLength fo == foobar --> 2");
        assert_eq!(match_length("foo", "foobar"), 3, "matchLength foo == foobar --> 3");
        assert_eq!(match_length("foob", "foobar"), 4, "matchLength foob == foobar --> 4");
        assert_eq!(match_length("fooba", "foobar"), 5, "matchLength fooba == foobar --> 5");
        assert_eq!(match_length("foobar", "foobar"), 6, "matchLength foobar == foobar --> 6");
        assert_eq!(match_length("foobar", "fooba"), 5, "matchLength foobar == fooba --> 5");
        assert_eq!(match_length("foobar", "foob"), 4, "matchLength foobar == foob --> 4");
        assert_eq!(match_length("foobar", "foo"), 3, "matchLength foobar == foo --> 3");
        assert_eq!(match_length("foobar", "fo"), 2, "matchLength foobar == fo --> 2");
        assert_eq!(match_length("foobar", "f"), 1, "matchLength foobar == f --> 1");
        assert_eq!(match_length("foobar", ""), 0, "matchLength foobar == '' --> 0");

        assert_eq!(match_length("foobar", "foodmixer"), 3, "matchLength foobar == foodmixer --> 3");
        assert_eq!(match_length("foodmixer", "foobar"), 3, "matchLength foodmixer == foobar --> 3");
    }

    #[test]
    fn case_conversion() {
        // std::string lowerCase (const std::string&);
        assert_eq!(lower_case(""), "", "lowerCase '' --> ''");
        assert_eq!(lower_case("a"), "a", "lowerCase 'a' --> 'a'");
        assert_eq!(lower_case("aA"), "aa", "lowerCase 'aA' --> 'aa'");
        assert_eq!(lower_case("A"), "a", "lowerCase 'A' --> 'a'");
        assert_eq!(lower_case("$"), "$", "lowerCase '$' --> '$'");

        // std::string upperCase (const std::string&);
        assert_eq!(upper_case(""), "", "upperCase '' --> ''");
        assert_eq!(upper_case("a"), "A", "upperCase 'a' --> 'A'");
        assert_eq!(upper_case("aA"), "AA", "upperCase 'aA' --> 'AA'");
        assert_eq!(upper_case("A"), "A", "upperCase 'A' --> 'A'");
        assert_eq!(upper_case("$"), "$", "upperCase '$' --> '$'");

        // std::string upperCaseFirst (const std::string&);
        assert_eq!(upper_case_first(""), "", "upperCaseFirst '' --> ''");
        assert_eq!(upper_case_first("a"), "A", "upperCaseFirst 'a' --> 'A'");
        assert_eq!(upper_case_first("A"), "A", "upperCaseFirst 'A' --> 'A'");
        assert_eq!(upper_case_first("aa"), "Aa", "upperCaseFirst 'aa' --> 'Aa'");
        assert_eq!(upper_case_first("$"), "$", "upperCaseFirst '$' --> '$'");

        // Not in shared.t.cpp: only ASCII bytes are converted.
        assert_eq!(lower_case("ÇA"), "Ça");
        assert_eq!(upper_case_first("çb"), "çb");
    }

    #[test]
    fn str_replace_cases() {
        // std::string str_replace (const std::string&, const std::string&, const std::string&);
        let input = "Lorem ipsum dolor sit amet, est aliquip scaevola dignissim in, vim nominavi electram te.";
        assert_eq!(
            str_replace(input, "x", "X"),
            "Lorem ipsum dolor sit amet, est aliquip scaevola dignissim in, vim nominavi electram te.",
            "str_replace 'x' -- 'X' (NOP)"
        );

        assert_eq!(
            str_replace(input, "e", "E"),
            "LorEm ipsum dolor sit amEt, Est aliquip scaEvola dignissim in, vim nominavi ElEctram tE.",
            "str_replace 'e' -- 'E'"
        );

        // Not in shared.t.cpp: replacement containing the search text.
        assert_eq!(str_replace("aaa", "a", "aa"), "aaaaaa");
    }

    #[test]
    fn find_cases() {
        // std::string::size_type find (const std::string&, const std::string&, bool sensitive = true);
        // std::string::size_type find (const std::string&, const std::string&, std::string::size_type, bool sensitive = true);
        // Make sure degenerate cases are handled.
        assert_eq!(find("foo", "", true), Some(0), "find foo !contains ''");
        assert_eq!(find("", "foo", true), None, "find '' !contains foo");

        // Make sure the default is case-sensitive.
        assert_eq!(find("foo", "fo", true), Some(0), "find foo contains fo");
        assert_eq!(find("foo", "FO", true), None, "find foo !contains fo");

        // Test case-sensitive.
        assert_eq!(find("foo", "xx", true), None, "find foo !contains xx");
        assert_eq!(find("foo", "oo", true), Some(1), "find foo contains oo");

        assert_eq!(find("foo", "fo", true), Some(0), "find foo contains fo");
        assert_eq!(find("foo", "FO", true), None, "find foo !contains fo");
        assert_eq!(find("FOO", "fo", true), None, "find foo !contains fo");
        assert_eq!(find("FOO", "FO", true), Some(0), "find foo contains fo");

        // Test case-insensitive.
        assert_eq!(find("foo", "xx", false), None, "find foo !contains xx (caseless)");
        assert_eq!(find("foo", "oo", false), Some(1), "find foo contains oo (caseless)");

        assert_eq!(find("foo", "fo", false), Some(0), "find foo contains fo (caseless)");
        assert_eq!(find("foo", "FO", false), Some(0), "find foo contains FO (caseless)");
        assert_eq!(find("FOO", "fo", false), Some(0), "find FOO contains fo (caseless)");
        assert_eq!(find("FOO", "FO", false), Some(0), "find FOO contains FO (caseless)");

        // Test start offset.
        assert_eq!(find_from("one two three", "e", 3, true), Some(11), "find offset obeyed");
        assert_eq!(find_from("one two three", "e", 11, true), Some(11), "find offset obeyed");

        assert_eq!(find_from("one two three", "e", 3, false), Some(11), "find offset obeyed");
        assert_eq!(find_from("one two three", "e", 11, false), Some(11), "find offset obeyed");

        // Not in shared.t.cpp: begin/pattern edge cases of the two branches.
        assert_eq!(find_from("foo", "", 2, true), Some(2));
        assert_eq!(find_from("foo", "", 2, false), Some(0));
        assert_eq!(find_from("foo", "", 4, true), None);
        assert_eq!(find_from("foo", "o", 3, false), None);
        assert_eq!(find("fo", "foo", false), None);
        assert_eq!(find("aÇb", "b", true), Some(3));
    }

    #[test]
    fn os_name_and_compliance() {
        // Test osName actually recognizes OS.
        assert_ne!(os_name(), "<unknown>", "osName: Recognizes OS as: {}", os_name());

        // Test cppCompliance actually recognizes compliance level.
        assert_ne!(
            cpp_compliance(),
            "non-compliant",
            "cppCompliance: Recognizes compiler compliance level as: {}",
            cpp_compliance()
        );
    }

    /// One block of the C++ IP address assertions: `isIPv4Address` and
    /// `isIPv6Address` on `input`, expecting `expected` from the one named by
    /// `v6` and a rejection from the other.
    type IpParser = fn(&str) -> Option<(String, i32)>;

    fn check_ip(input: &str, v6: bool, address: &str, port: i32) {
        let (accepting, rejecting, name, other): (IpParser, IpParser, &str, &str) = if v6 {
                (is_ipv6_address, is_ipv4_address, "isIPv6Address", "isIPv4Address")
            } else {
                (is_ipv4_address, is_ipv6_address, "isIPv4Address", "isIPv6Address")
            };

        assert!(rejecting(input).is_none(), "{other} {input} --> no");
        let result = accepting(input);
        assert!(result.is_some(), "{name} {input} --> yes");
        let (got_address, got_port) = result.unwrap();
        assert_eq!(got_address, address, "{name} {input} --> address correct");
        assert_eq!(got_port, port, "{name} {input} --> port correct");
    }

    #[test]
    fn ip_addresses() {
        // Test IPv4/IPv6 address parsing.
        check_ip("127.0.0.1", false, "127.0.0.1", 0);
        check_ip("127.0.0.1:80", false, "127.0.0.1", 80);
        check_ip("::1", true, "::1", 0);
        check_ip("[::1]:80", true, "::1", 80);
        check_ip("2605:2700:0:3::4713:93e3", true, "2605:2700:0:3::4713:93e3", 0);
        check_ip("[2605:2700:0:3::4713:93e3]:80", true, "2605:2700:0:3::4713:93e3", 80);
        check_ip("2001:db8:85a3:0:0:8a2e:370:7334", true, "2001:db8:85a3:0:0:8a2e:370:7334", 0);
        check_ip("2001:db8:85a3::8a2e:370:7334", true, "2001:db8:85a3::8a2e:370:7334", 0);
        check_ip(
            "[2001:db8:85a3:8d3:1319:8a2e:370:7348]:443",
            true,
            "2001:db8:85a3:8d3:1319:8a2e:370:7348",
            443,
        );
        check_ip("::ffff:192.168.0.1", true, "::ffff:192.168.0.1", 0);
        check_ip("[::ffff:71.19.147.227]:80", true, "::ffff:71.19.147.227", 80);
        check_ip("::", true, "::", 0);

        // Not in shared.t.cpp: rejections.
        assert!(is_ipv4_address("256.0.0.1").is_none());
        assert!(is_ipv4_address("1.2.3.4:").is_none());
        assert!(is_ipv4_address("1.2.3.4:123456").is_none());
        assert!(is_ipv4_address("1.2.3").is_none());
        assert!(is_ipv6_address("").is_none());
        assert!(is_ipv6_address("1:2").is_none());
    }

    #[test]
    fn auto_complete_cases() {
        // libshared test/autocomplete.t.cpp
        let options = ["abc", "abcd", "abcde", "bcdef", "cdefg"];
        let matches = auto_complete("", &options, 1);
        assert_eq!(matches.len(), 0, "no match on empty string");

        let matches = auto_complete("x", &options, 1);
        assert_eq!(matches.len(), 0, "no match on wrong string");

        let matches = auto_complete("abcd", &options, 1);
        assert_eq!(matches.len(), 1, "exact match on 'abcd'");
        assert_eq!(matches[0], "abcd", "exact match on 'abcd'");

        let matches = auto_complete("ab", &options, 1);
        assert_eq!(matches.len(), 3, "partial match on 'ab'");
        assert_eq!(matches[0], "abc", "partial match on 'abc'");
        assert_eq!(matches[1], "abcd", "partial match on 'abcd'");
        assert_eq!(matches[2], "abcde", "partial match on 'abcde'");

        // Not in autocomplete.t.cpp: the minimum length.
        assert_eq!(auto_complete("ab", &options, 3).len(), 0);
        assert_eq!(auto_complete("ab", &options, -1).len(), 0);
        assert_eq!(auto_complete("abc", &options, 3), vec!["abc"]);
    }

    #[test]
    fn list_operations() {
        // libshared test/list.t.cpp
        // listDiff
        // 1,2,3  <=>  2,3,4
        let string_one = ["1", "2", "3"].map(String::from);
        let string_two = ["2", "3", "4"].map(String::from);

        // What are the differences?
        let (string_left_only, string_right_only) = list_diff(&string_one, &string_two);
        assert_eq!(string_left_only.len(), 1, "std::string (1,2,3) <=> (2,3,4) = 1<-");
        assert_eq!(string_left_only[0], "1", "std::string (1,2,3) <=> (2,3,4) = 1<-");

        assert_eq!(string_right_only.len(), 1, "std::string (1,2,3) <=> (2,3,4) = ->4");
        assert_eq!(string_right_only[0], "4", "std::string (1,2,3) <=> (2,3,4) = ->4");

        // Now do it all again, with integers.

        // 1,2,3  <=>  2,3,4
        let int_one = [1, 2, 3];
        let int_two = [2, 3, 4];

        // What are the differences?
        let (int_left_only, int_right_only) = list_diff(&int_one, &int_two);
        assert_eq!(int_left_only.len(), 1, "int (1,2,3) <=> (2,3,4) = 1<-");
        assert_eq!(int_left_only[0], 1, "int (1,2,3) <=> (2,3,4) = 1<-");

        assert_eq!(int_right_only.len(), 1, "int (1,2,3) <=> (2,3,4) = ->4");
        assert_eq!(int_right_only[0], 4, "int (1,2,3) <=> (2,3,4) = ->4");

        // listIntersect
        let intersection = list_intersect(&["a", "b", "c"], &["a", "d"]);
        assert_eq!(intersection.len(), 1, "intersect (a,b,c) <=> (a,d) --> (a)");
        assert_eq!(intersection[0], "a", "intersect (a,b,c) <=> (a,d) --> (a)");

        let empty: [&str; 0] = [];
        let intersection = list_intersect(&empty, &empty);
        assert_eq!(intersection.len(), 0, "intersect () <=> () --> ()");

        let intersection = list_intersect(&["a"], &empty);
        assert_eq!(intersection.len(), 0, "intersect (a) <=> () --> ()");

        let intersection = list_intersect(&["a"], &["b"]);
        assert_eq!(intersection.len(), 0, "intersect (a) <=> (b) --> ()");

        let intersection = list_intersect(&["a", "b"], &["a", "b"]);
        assert_eq!(intersection.len(), 2, "intersect (a,b) <=> (a,b) --> (a,b)");
        assert_eq!(intersection[0], "a", "intersect (a,b) <=> (a,b) --> (a,b)");
        assert_eq!(intersection[1], "b", "intersect (a,b) <=> (a,b) --> (a,b)");
    }

    #[test]
    fn execute_cases() {
        // Not in shared.t.cpp (which does not exercise execute).
        let (status, output) = execute("cat", &[] as &[&str], "hello\nworld\n").unwrap();
        assert_eq!(status, 0);
        assert_eq!(output, "hello\nworld\n");

        let (status, output) = execute("sh", &["-c", "printf out; exit 3"], "").unwrap();
        assert_eq!(status, 3);
        assert_eq!(output, "out");

        // Input the child never reads is not an error.
        let big = "x".repeat(1 << 20);
        let (status, output) = execute("sh", &["-c", "exit 0"], &big).unwrap();
        assert_eq!(status, 0);
        assert_eq!(output, "");

        // Output is appended chunk by chunk, each up to its first NUL.
        let (_, output) = execute("printf", &["a\\000b"], "").unwrap();
        assert_eq!(output, "a");

        // A child killed by a signal has no exit status.
        assert!(execute("sh", &["-c", "kill -9 $$"], "").is_err());

        // An executable that cannot be run reports status 255.
        let (status, output) = execute("taskwarrior-no-such-executable", &[] as &[&str], "").unwrap();
        assert_eq!(status, 255);
        assert_eq!(output, "");
    }
}
