//! Rust translation of `src/util.cpp` (declarations in `src/util.h`).
//!
//! C++ functions that read `Context::getContext ()` take the context
//! explicitly ([`confirm4`], [`optional_blank_line`],
//! [`set_header_underline`]). The C++ `uuid_unparse_lower` fallback for old
//! libuuid versions is not needed: [`uuid`] formats through the `uuid` crate,
//! which is always lowercase.

use std::ffi::CString;
use std::io::{BufRead, Write};

use crate::context::Context;
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::format::format;
use crate::shared::shared::{auto_complete, lower_case, trim_with};
use crate::shared::table::Table;
use crate::shared::unicode::unicode_whitespace;
use crate::shared::utf8::utf8_next_char;

const STRING_UTIL_CONFIRM_YES: &str = "yes";
const STRING_UTIL_CONFIRM_YES_U: &str = "Yes";
const STRING_UTIL_CONFIRM_NO: &str = "no";
const STRING_UTIL_CONFIRM_ALL: &str = "all";
const STRING_UTIL_CONFIRM_ALL_U: &str = "All";
const STRING_UTIL_CONFIRM_QUIT: &str = "quit";

/// C++ file-static `newline`.
const NEWLINE: &str = "\n";
/// C++ file-static `noline`.
const NOLINE: &str = "";

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

////////////////////////////////////////////////////////////////////////////////
/// C++ `uuid ()`: a new random (version 4) UUID in its lowercase, hyphenated
/// 36-character form (C++ `uuid_generate` + `uuid_unparse_lower`).
pub fn uuid() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

////////////////////////////////////////////////////////////////////////////////
/// Byte offset of the next `delimiter` in `haystack` at or after `from`
/// (C++ `std::string::find (char, pos)`; `None` is `npos`).
fn find_byte(haystack: &[u8], delimiter: u8, from: usize) -> Option<usize> {
    haystack.get(from..)?.iter().position(|&b| b == delimiter).map(|p| p + from)
}

/// The C++ `char` value of `delimiter`. C++ takes a single byte, so only an
/// ASCII delimiter is meaningful (every caller passes `.`); a non-ASCII
/// `char` is reduced to its first UTF-8 byte.
fn delimiter_byte(delimiter: char) -> u8 {
    let mut buffer = [0u8; 4];
    delimiter.encode_utf8(&mut buffer).as_bytes()[0]
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `indentProject`: accept a project, and return it indented to reflect
/// the hierarchy.
///
/// ```text
///      Input  - "one"
///               "one.two"
///               "one.two.three"
///               "one.four"
///               "two"
///      Output - "one"
///               "  one.two"
///               "    one.two.three"
///               "  one.four"
///               "two"
/// ```
///
/// - `whitespace` is the string used to build the prefixes of indented items
///   (C++ default: two spaces, `"  "`).
/// - `delimiter` is the character used to split up projects into subprojects
///   (C++ default: the period, `'.'`).
///
/// As in C++ the search starts at byte 1 (a leading delimiter never counts)
/// and a trailing delimiter is not counted either.
pub fn indent_project(project: &str, whitespace: &str, delimiter: char) -> String {
    let bytes = project.as_bytes();
    let delimiter = delimiter_byte(delimiter);

    // Count the delimiters in *i.
    let mut prefix = String::new();
    let mut pos: usize = 0;
    let mut lastpos: usize = 0;
    while let Some(found) = find_byte(bytes, delimiter, pos + 1) {
        pos = found;
        if pos != bytes.len() - 1 {
            prefix.push_str(whitespace);
            lastpos = pos;
        }
    }

    let child = if lastpos == 0 {
        project.to_string()
    } else {
        String::from_utf8_lossy(&bytes[lastpos + 1..]).into_owned()
    };

    prefix + &child
}

/// [`indent_project`] with the C++ default arguments (`"  "`, `'.'`).
pub fn indent_project_default(project: &str) -> String {
    indent_project(project, "  ", '.')
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `extractParents`: all ancestor projects of `project`, outermost
/// first (`"a.b.c"` gives `["a", "a.b"]`). A trailing delimiter does not
/// produce a parent; the search starts at byte 1, as in C++.
pub fn extract_parents(project: &str, delimiter: char) -> Vec<String> {
    let bytes = project.as_bytes();
    let delimiter = delimiter_byte(delimiter);

    let mut vec = Vec::new();
    let mut pos: usize = 0;
    while let Some(copy_until) = find_byte(bytes, delimiter, pos + 1) {
        if copy_until != bytes.len() - 1 {
            vec.push(String::from_utf8_lossy(&bytes[..copy_until]).into_owned());
        }
        pos = copy_until;
    }
    vec
}

/// [`extract_parents`] with the C++ default delimiter `'.'`.
pub fn extract_parents_default(project: &str) -> Vec<String> {
    extract_parents(project, '.')
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `nontrivial`: true if `input` contains any non-whitespace character
/// (Unicode whitespace as defined by `unicodeWhitespace`). Like the C++ loop,
/// scanning stops at an embedded NUL.
pub fn nontrivial(input: &str) -> bool {
    let mut i: usize = 0;
    loop {
        let character = utf8_next_char(input, &mut i);
        if character == 0 {
            break;
        }
        if !unicode_whitespace(character) {
            return true;
        }
    }

    false
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `optionalBlankLine`: `"\n"` when the `blank` verbosity token is
/// active (`context.verbose ("blank")`), otherwise `""`.
pub fn optional_blank_line(context: &mut Context) -> &'static str {
    if context.verbose("blank") {
        NEWLINE
    } else {
        NOLINE
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `setHeaderUnderline`: styles the header (and alternate rows) of
/// `table` from `color`, `color.alternate`, `color.label` and
/// `fontunderline`.
///
/// Deviation: C++ `Color (std::string)` throws on an unrecognized colour
/// name; here that error (same text) is returned as `Err`, and `table` keeps
/// whatever was applied before the failing colour.
pub fn set_header_underline(context: &mut Context, table: &mut Table) -> Result<()> {
    // If an alternating row color is specified, notify the table.
    if context.color() {
        let alternate = Color::parse(&context.config.get("color.alternate"))?;
        table.color_odd(alternate);
        table.intra_color_odd(alternate);

        if context.config.get_boolean("fontunderline") {
            table.color_header(Color::parse(&("underline ".to_string() + &context.config.get("color.label")))?);
        } else {
            table.color_header(Color::parse(&context.config.get("color.label"))?);
            table.underline_headers();
        }
    } else if context.config.get_boolean("fontunderline") {
        table.color_header(Color::parse("underline")?);
    } else {
        table.underline_headers();
    }
    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `extractLongInteger`: perform `strtol` (base 10) on a string and check
/// that the extracted value, formatted back, matches the input exactly.
///
/// Returns the value on a match, `None` otherwise (C++ returns `false`; its
/// out-parameter is then only meaningful on success, which is all callers
/// use). `strtol` semantics are kept by calling libc: leading whitespace and
/// `+` are accepted by `strtol` but then fail the round trip, out-of-range
/// values saturate to `LONG_MIN`/`LONG_MAX` and so fail too. An input with an
/// embedded NUL can never round-trip (C++ `c_str ()` stops there), so it is
/// rejected directly.
pub fn extract_long_integer(input: &str) -> Option<i64> {
    let c_input = CString::new(input).ok()?;
    // SAFETY: `c_input` is a valid NUL-terminated string; a null `endptr` is
    // permitted by strtol.
    let output = unsafe { libc::strtol(c_input.as_ptr(), std::ptr::null_mut(), 10) } as i64;
    if format("{1}", &[&output]) == input {
        Some(output)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    //! Port of `test/util_test.cpp` plus tests for the functions it leaves
    //! uncovered. Tests that build a [`Context`] hold
    //! [`crate::test_util::env_lock`] (`Context::new` reads `$HOME`).
    use super::*;
    use crate::test_util::env_lock;
    use std::fmt::Debug;

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
    }

    /// Assertions of util_test.cpp ported here: 1 `Task::operator==`
    /// ("Detected changes"), 4 `indentProject` + 9 `nontrivial`. Not ported
    /// here (they test functions deferred to later milestones): the other 5
    /// `Task::diff` assertions (`Task::diff` is restored by milestone 7) and
    /// the 3 `checked_add_datetime` assertions (recur.cpp, milestone 7).
    const UTIL_TEST_ASSERTIONS: usize = 14;

    #[test]
    fn util_test_cpp() {
        let mut t = UnitTest::new();

        // These are in feedback.cpp, not util.cpp (C++ comment).
        let mut left = crate::task::Task::new();
        left.set("zero", "0");
        left.set_int("one", 1);
        left.set_int("two", 2);

        let mut right = crate::task::Task::new();
        right.set("zero", "00");
        right.set_int("two", 2);
        right.set_int("three", 3);

        // `left.diff (right)` (milestone 7) is not computed here; the
        // `operator==` assertion does not depend on it.
        t.ok(!(left == right), "Detected changes");

        // std::vector<std::string> indentProject (const std::string&, const std::string whitespace="  ",
        // char delimiter='.');
        t.is(indent_project_default(""), "", "indentProject '' -> ''");
        t.is(indent_project_default("one"), "one", "indentProject 'one' -> 'one'");
        t.is(indent_project_default("one.two"), "  two", "indentProject 'one.two' -> '  two'");
        t.is(
            indent_project_default("one.two.three"),
            "    three",
            "indentProject 'one.two.three' -> '    three'",
        );

        // bool nontrivial (const std::string&);
        t.notok(nontrivial(""), "nontrivial '' -> false");
        t.notok(nontrivial("   "), "nontrivial '   ' -> false");
        t.notok(nontrivial("\t\t"), "nontrivial '\\t\\t' -> false");
        t.notok(nontrivial(" \t \t"), "nontrivial ' \\t \\t' -> false");
        t.ok(nontrivial("a"), "nontrivial 'a' -> true");
        t.ok(nontrivial("   a"), "nontrivial '   a' -> true");
        t.ok(nontrivial("a   "), "nontrivial 'a   ' -> true");
        t.ok(nontrivial("  \t\ta"), "nontrivial '  \\t\\ta' -> true");
        t.ok(nontrivial("a\t\t  "), "nontrivial 'a\\t\\t  ' -> true");

        assert!(t.failures.is_empty(), "{}", t.failures.join("\n"));
        assert_eq!(t.counter, UTIL_TEST_ASSERTIONS, "number of assertions executed");
    }

    #[test]
    fn uuid_is_lowercase_v4() {
        let a = uuid();
        let b = uuid();
        assert_eq!(a.len(), 36);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
        assert!(!a.chars().any(|c| c.is_ascii_uppercase()));
        for (i, c) in a.char_indices() {
            if [8, 13, 18, 23].contains(&i) {
                assert_eq!(c, '-');
            }
        }
        assert_eq!(&a[14..15], "4", "version nibble");
        assert!("89ab".contains(&a[19..20]), "variant nibble");
    }

    #[test]
    fn indent_project_edge_cases() {
        // Trailing delimiter is not counted.
        assert_eq!(indent_project_default("one.two."), "  two.");
        assert_eq!(indent_project_default("one."), "one.");
        // A leading delimiter is never found (search starts at 1).
        assert_eq!(indent_project_default(".one"), ".one");
        assert_eq!(indent_project_default(".one.two"), "  two");
        // Custom whitespace and delimiter.
        assert_eq!(indent_project("a/b/c", "--", '/'), "----c");
        assert_eq!(indent_project("a.b/c", "--", '/'), "--c");
        assert_eq!(indent_project("x..y", " ", '.'), "  y");
    }

    #[test]
    fn extract_parents_cases() {
        assert!(extract_parents_default("").is_empty());
        assert!(extract_parents_default("one").is_empty());
        assert_eq!(extract_parents_default("one.two"), vec!["one"]);
        assert_eq!(extract_parents_default("one.two.three"), vec!["one", "one.two"]);
        assert_eq!(extract_parents_default("one.two."), vec!["one"]);
        assert!(extract_parents_default(".one").is_empty());
        assert_eq!(extract_parents("a/b/c", '/'), vec!["a", "a/b"]);
    }

    #[test]
    fn nontrivial_unicode() {
        // U+3000 IDEOGRAPHIC SPACE and U+00A0 NO-BREAK SPACE are whitespace.
        assert!(!nontrivial("\u{3000}\u{a0} "));
        assert!(nontrivial("\u{3000}é"));
    }

    #[test]
    fn extract_long_integer_strtol_semantics() {
        assert_eq!(extract_long_integer("0"), Some(0));
        assert_eq!(extract_long_integer("123"), Some(123));
        assert_eq!(extract_long_integer("-42"), Some(-42));
        assert_eq!(extract_long_integer("9223372036854775807"), Some(i64::MAX));
        assert_eq!(extract_long_integer("-9223372036854775808"), Some(i64::MIN));
        // Saturation fails the round trip.
        assert_eq!(extract_long_integer("9223372036854775808"), None);
        assert_eq!(extract_long_integer(""), None);
        assert_eq!(extract_long_integer("+1"), None);
        assert_eq!(extract_long_integer(" 1"), None);
        assert_eq!(extract_long_integer("01"), None);
        assert_eq!(extract_long_integer("1a"), None);
        assert_eq!(extract_long_integer("abc"), None);
        assert_eq!(extract_long_integer("1\u{0}"), None);
    }

    fn context_with(settings: &[(&str, &str)]) -> Context {
        let mut context = Context::new();
        for (name, value) in settings {
            context.config.set(name, *value);
        }
        context
    }

    #[test]
    fn optional_blank_line_follows_verbosity() {
        let _lock = env_lock();
        let mut context = context_with(&[("verbose", "blank,label")]);
        assert_eq!(optional_blank_line(&mut context), "\n");
        let mut context = context_with(&[("verbose", "label")]);
        assert_eq!(optional_blank_line(&mut context), "");
        let mut context = context_with(&[("verbose", "on")]);
        assert_eq!(optional_blank_line(&mut context), "\n");
        let mut context = context_with(&[("verbose", "nothing")]);
        assert_eq!(optional_blank_line(&mut context), "");
    }

    fn styled(settings: &[(&str, &str)]) -> Result<Table> {
        let mut context = context_with(settings);
        let mut table = Table::new();
        set_header_underline(&mut context, &mut table)?;
        Ok(table)
    }

    #[test]
    fn set_header_underline_variants() {
        let _lock = env_lock();
        let dbg = |t: &Table| format!("{t:?}");

        // No colour, no fontunderline: underlined headers only.
        let mut expected = Table::new();
        expected.underline_headers();
        let table = styled(&[("color", "off"), ("_forcecolor", "off"), ("fontunderline", "off")]).unwrap();
        assert_eq!(dbg(&table), dbg(&expected));

        // No colour, fontunderline: underline colour header.
        let mut expected = Table::new();
        expected.color_header(Color::parse("underline").unwrap());
        let table = styled(&[("color", "off"), ("_forcecolor", "off"), ("fontunderline", "on")]).unwrap();
        assert_eq!(dbg(&table), dbg(&expected));

        // Colour, fontunderline.
        let colour = [
            ("_forcecolor", "on"),
            ("color.alternate", "on gray2"),
            ("color.label", "bold"),
            ("fontunderline", "on"),
        ];
        let mut expected = Table::new();
        let alternate = Color::parse("on gray2").unwrap();
        expected.color_odd(alternate);
        expected.intra_color_odd(alternate);
        expected.color_header(Color::parse("underline bold").unwrap());
        assert_eq!(dbg(&styled(&colour).unwrap()), dbg(&expected));

        // Colour, no fontunderline: label colour plus underlined headers.
        let mut settings = colour.to_vec();
        settings[3] = ("fontunderline", "off");
        let mut expected = Table::new();
        expected.color_odd(alternate);
        expected.intra_color_odd(alternate);
        expected.color_header(Color::parse("bold").unwrap());
        expected.underline_headers();
        assert_eq!(dbg(&styled(&settings).unwrap()), dbg(&expected));

        // Empty colours are valid (no colour).
        let table = styled(&[
            ("_forcecolor", "on"),
            ("color.alternate", ""),
            ("color.label", ""),
            ("fontunderline", "on"),
        ])
        .unwrap();
        let mut expected = Table::new();
        expected.color_odd(Color::new());
        expected.intra_color_odd(Color::new());
        expected.color_header(Color::parse("underline ").unwrap());
        assert_eq!(dbg(&table), dbg(&expected));

        // A bad colour name is the C++ exception text.
        let err = styled(&[("_forcecolor", "on"), ("color.alternate", "nosuchcolor")]).unwrap_err();
        assert_eq!(err.to_string(), "The color 'nosuchcolor' is not recognized.");
    }
}
