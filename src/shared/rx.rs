//! Rust translation of `libshared src/RX.cpp` / `RX.h`.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.
//!
//! At that commit `RX` is a thin wrapper over `std::regex` with the
//! `ECMAScript` grammar (plus `icase` when not case sensitive); it no longer
//! uses POSIX `regcomp`. The dialect Taskwarrior users get on Linux is
//! therefore the libstdc++ (GCC 11) ECMAScript dialect, executed by the
//! libstdc++ backtracking executor over the bytes (`char`s) of the
//! `std::string`, in the "C" locale (Taskwarrior never calls `setlocale` or
//! `std::locale::global`).
//!
//! The Rust `regex` crate is a different dialect, so the pattern is not handed
//! to it verbatim. Instead [`translate`] is a port of the libstdc++
//! `std::__detail::_Scanner` (ECMAScript paths of `regex_scanner.tcc`) and
//! `_Compiler` (`regex_compiler.tcc`, `regex_compiler.h`): it accepts and
//! rejects exactly the patterns libstdc++ accepts and rejects, raises the
//! same `regex_error::what ()` texts, and emits an equivalent pattern for a
//! byte-oriented `regex::bytes::Regex` with Unicode disabled. Every single
//! character matcher (literal, `.`, `\d`/`\w`/`\s` and negations, bracket
//! expressions) is evaluated for all 256 byte values exactly as libstdc++
//! `_BracketMatcher::_M_make_cache` does, and emitted as an explicit byte
//! class, so:
//! - `icase` folds ASCII letters only (C-locale `tolower`/`toupper`): `é`
//!   does not match `É`, `[[:lower:]]` matches upper case letters, ranges
//!   compare `tolower (c)` and `toupper (c)`;
//! - `\d`, `\w`, `\s`, `\b` and the POSIX classes `[[:alpha:]]`, `[[:digit:]]`,
//!   `[[:space:]]`, `[[:upper:]]`, `[[:punct:]]`, `[[:w:]]`, `[[:d:]]`,
//!   `[[:s:]]` ... are ASCII-only (C locale); class names are case-insensitive
//!   (`[[:ALPHA:]]`), as `regex_traits::lookup_classname` lowercases them;
//! - `[[.name.]]` collating symbols use the libstdc++ name table
//!   (`[[.space.]]`, `[[.hyphen.]]`, `[[.a.]]`), `[[=x=]]` equivalence classes
//!   match `x` in either case;
//! - `.` matches any byte except `\n` and `\r`; non-ASCII text is matched byte
//!   by byte, and the offsets returned by [`RX::matches_offsets`] are byte
//!   offsets, exactly as in C++;
//! - ranges compare signed `char`s, so `[a-é]` is "Invalid range in bracket
//!   expression." as in C++;
//! - `\<` and `\>` are identity escapes, i.e. the literal characters `<` and
//!   `>`, as in libstdc++ ECMAScript. `\b`/`\B` are the word boundaries
//!   (ASCII word characters), and the C++ `CLI2` generates `\b...\b` for the
//!   `word`/`noword` modifiers on Linux, so this is what Taskwarrior on Linux
//!   does. Milestone Design Decision 2 option 1 assumed POSIX `regcomp`
//!   semantics (where `\<`/`\>` are word start/end). The reference binary
//!   treats them as literals, so the reference behaviour wins here and they
//!   are deliberately not translated to word boundaries.
//! - `\cX` matches the literal `X` (libstdc++ quirk), `\0` is NUL, `\xHH` and
//!   `\uHHHH` are one byte (`\uHHHH` keeps the low byte, as the libstdc++
//!   `_M_value.assign (1, int)` does), `[\b]` is backspace, `[]` matches
//!   nothing and `[^]` matches every byte;
//! - stacked quantifiers (`a+*`, `a{1}{2}`), lazy quantifiers and `(?:...)`
//!   behave as in libstdc++. A quantifier after an assertion is "Mismatched
//!   '(' and ')' in regular expression".
//! - The libstdc++ NFA is modelled state by state (including the clones made
//!   by `{n,m}`), so "Number of NFA states exceeds limit. ..." is raised for
//!   the same patterns (`_GLIBCXX_REGEX_STATE_LIMIT` 100000).
//!
//! Both engines use leftmost-first (Perl/ECMAScript) match priority, unlike
//! POSIX leftmost-longest, so alternation order and greedy/lazy quantifiers
//! select the same match.
//!
//! Deviation: back-references (`(a)\1`) and lookahead assertions (`(?=...)`,
//! `(?!...)`) are valid in C++ but cannot be expressed with the `regex`
//! crate. The pattern is still fully validated as libstdc++ does (a C++
//! error later in the pattern still wins), and then compilation fails with
//! [`BACKREF_UNSUPPORTED`] or [`LOOKAHEAD_UNSUPPORTED`].
//!
//! Deviation: when the preferred match at a position is empty while a
//! non-empty alternative exists there (lazy quantifiers such as `a??`/`a*?`,
//! empty-first alternations such as `|a`), C++ `sregex_iterator` retries with
//! `match_not_null | match_continuous` and also reports the non-empty match.
//! The `regex` crate cannot search for "a non-empty match only", so that
//! retry finds nothing and only the empty match is reported. Ordinary
//! patterns, including those that match empty strings such as `x*`, iterate
//! identically.
//!
//! Deviation: repetitions of sub-patterns that can match the empty string
//! (`(a*)*`, `(a|)+`) follow the `regex` crate's rules for empty iterations,
//! which may select a different (equally valid) overall match than the
//! libstdc++ executor in contrived cases.
//!
//! Deviation: the libstdc++ backtracking executor recurses per character and
//! can exhaust the stack on long subjects (a crash in C++). The `regex` crate
//! has no such failure mode. A translated pattern too big for the `regex`
//! crate's (raised) size limit is reported with the C++ NFA-limit message.
//!
//! Deviation: C++ throws `std::runtime_error (e.what ())`, which Taskwarrior
//! itself never prints (`Context::run`/`main` print "Unknown error. Please
//! report." from `catch (...)`). Here the text is returned as
//! [`TwError::Message`]; callers that need the Taskwarrior behaviour map it.
//!
//! C++ API to Rust:
//! - `RX ()` -> `RX::default ()` (empty pattern, case insensitive, as C++
//!   `_case_sensitive {false}`)
//! - `RX (pattern, case_sensitive = true)` -> [`RX::new`] /
//!   [`RX::with_pattern`]
//! - `RX (const RX&)`, `operator=` -> `Clone` (uncompiled copy)
//! - `compile ()` -> [`RX::compile`]
//! - `match (in)` -> [`RX::matches`]
//! - `match (matches, in)` -> [`RX::matches_captures`]
//! - `match (start, end, in)` -> [`RX::matches_offsets`]

use crate::error::{Result, TwError};
use regex::bytes::{Regex, RegexBuilder};
use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::fmt;

/// Error for a back-reference, valid in libstdc++ but unsupported by the
/// `regex` crate (see the module-level `Deviation:`).
pub const BACKREF_UNSUPPORTED: &str = "Back-references are not supported in regular expressions.";

/// Error for a lookahead assertion, valid in libstdc++ but unsupported by the
/// `regex` crate (see the module-level `Deviation:`).
pub const LOOKAHEAD_UNSUPPORTED: &str = "Lookahead assertions are not supported in regular expressions.";

/// libstdc++ `regex_error (error_space)` text.
const NFA_LIMIT: &str = "Number of NFA states exceeds limit. Please use shorter regex string, or use smaller brace expression, or make _GLIBCXX_REGEX_STATE_LIMIT larger.";

/// libstdc++ `__regex_errstr (error_paren)`, the text of a `regex_error`
/// thrown without an explicit message.
const PAREN_MISMATCH: &str = "Mismatched '(' and ')' in regular expression";

/// `_GLIBCXX_REGEX_STATE_LIMIT`.
const STATE_LIMIT: usize = 100_000;

/// The libshared `RX` class: a lazily compiled regular expression.
pub struct RX {
    /// C++ `_compiled` / `_regex`: filled by the first [`RX::compile`]. A
    /// failed compilation is cached too (C++ retries and throws again each
    /// time, with the same result).
    compiled: OnceCell<std::result::Result<Regex, String>>,
    pattern: String,
    case_sensitive: bool,
}

impl Default for RX {
    /// C++ `RX::RX () = default`: empty pattern, `_case_sensitive {false}`.
    fn default() -> Self {
        RX { compiled: OnceCell::new(), pattern: String::new(), case_sensitive: false }
    }
}

impl Clone for RX {
    /// C++ `RX (const RX&)` / `operator=`: copies pattern and flag, not the
    /// compiled regex (`_compiled = false`).
    fn clone(&self) -> Self {
        RX { compiled: OnceCell::new(), pattern: self.pattern.clone(), case_sensitive: self.case_sensitive }
    }
}

impl PartialEq for RX {
    /// Two `RX` are equal when they would compile the same regex.
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern && self.case_sensitive == other.case_sensitive
    }
}

impl Eq for RX {}

impl fmt::Debug for RX {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RX")
            .field("pattern", &self.pattern)
            .field("case_sensitive", &self.case_sensitive)
            .field("compiled", &self.compiled.get().is_some())
            .finish()
    }
}

impl RX {
    /// C++ `RX (const std::string& pattern, bool case_sensitive)`.
    ///
    /// Deviation: C++ compiles in the constructor (and throws from it); here
    /// compilation is deferred to the first match, or forced with
    /// [`RX::compile`], which returns the error C++ would have thrown.
    pub fn new(pattern: impl Into<String>, case_sensitive: bool) -> Self {
        RX { compiled: OnceCell::new(), pattern: pattern.into(), case_sensitive }
    }

    /// C++ `RX (const std::string& pattern)`: `case_sensitive = true`.
    pub fn with_pattern(pattern: impl Into<String>) -> Self {
        RX::new(pattern, true)
    }

    /// The pattern, as given.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The case-sensitivity flag, as given.
    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// C++ `void RX::compile ()`: compiles once; on failure returns the
    /// `std::regex_error::what ()` text C++ rethrows as
    /// `std::runtime_error`.
    pub fn compile(&self) -> Result<()> {
        self.regex().map(|_| ())
    }

    fn regex(&self) -> Result<&Regex> {
        self.compiled
            .get_or_init(|| build(&self.pattern, !self.case_sensitive))
            .as_ref()
            .map_err(|e| TwError::Message(e.clone()))
    }

    /// C++ `bool RX::match (const std::string& in)`: `std::regex_search`.
    pub fn matches(&self, text: &str) -> Result<bool> {
        Ok(self.regex()?.is_match(text.as_bytes()))
    }

    /// C++ `bool RX::match (std::vector<std::string>& matches, const
    /// std::string& in)`: appends the text of every match (the whole match,
    /// `it->str ()`, not sub-groups) found by `std::sregex_iterator`, and
    /// returns whether `matches` is non-empty afterwards (including entries
    /// it held before the call, as in C++).
    ///
    /// Deviation: a match that ends or starts inside a UTF-8 sequence (C++
    /// returns the raw bytes) is converted with `String::from_utf8_lossy`.
    pub fn matches_captures(&self, matches: &mut Vec<String>, text: &str) -> Result<bool> {
        let re = self.regex()?;
        let bytes = text.as_bytes();
        for_each_match(re, bytes, |s, e| matches.push(String::from_utf8_lossy(&bytes[s..e]).into_owned()));
        Ok(!matches.is_empty())
    }

    /// C++ `bool RX::match (std::vector<int>& start, std::vector<int>& end,
    /// const std::string& in)`: appends the byte offsets of the start and end
    /// of every match, and returns whether `start` is non-empty afterwards.
    /// Offsets are byte offsets and, as in C++, can fall inside a UTF-8
    /// sequence when the pattern matches part of a multi-byte character
    /// (e.g. `.`), so check `str::is_char_boundary` before slicing.
    pub fn matches_offsets(&self, start: &mut Vec<i32>, end: &mut Vec<i32>, text: &str) -> Result<bool> {
        let re = self.regex()?;
        // C++ `push_back (it->position ())` narrows `ptrdiff_t` to `int`.
        for_each_match(re, text.as_bytes(), |s, e| {
            start.push(s as i32);
            end.push(e as i32);
        });
        Ok(!start.is_empty())
    }
}

/// `std::sregex_iterator` over `text`: calls `f (start, end)` for every match.
///
/// libstdc++ `regex_iterator::operator++`: after a non-empty match the next
/// search starts at its end (an empty match there is reported); after an
/// empty match it stops at the end of the subject, else retries a non-empty
/// match at the same position (see the module-level `Deviation:`) and
/// otherwise searches again one byte further. `match_prev_avail` corresponds
/// to `find_at` honouring the bytes before the start for `\b` and `^`.
fn for_each_match(re: &Regex, text: &[u8], mut f: impl FnMut(usize, usize)) {
    let mut found = re.find_at(text, 0);
    while let Some(m) = found {
        f(m.start(), m.end());
        let mut start = m.end();
        if m.start() == m.end() {
            if start == text.len() {
                break;
            }
            start += 1;
        }
        found = re.find_at(text, start);
    }
}

/// Compiles `pattern` as libstdc++ `std::regex (pattern, ECMAScript [|
/// icase])` would, into a byte regex.
fn build(pattern: &str, icase: bool) -> std::result::Result<Regex, String> {
    let translated = translate(pattern.as_bytes(), icase)?;
    RegexBuilder::new(&translated)
        .unicode(false)
        .size_limit(1 << 28)
        .nest_limit(1000)
        .build()
        .map_err(|e| match e {
            regex::Error::CompiledTooBig(_) => NFA_LIMIT.to_string(),
            other => other.to_string(),
        })
}

/// Translates a libstdc++ ECMAScript pattern into `regex` crate syntax (to be
/// compiled with Unicode disabled), or returns the libstdc++ error text.
pub(crate) fn translate(pattern: &[u8], icase: bool) -> std::result::Result<String, String> {
    Compiler::compile(pattern, icase)
}

////////////////////////////////////////////////////////////////////////////////
// C locale <ctype.h> (glibc), as used by std::ctype<char> in the "C" locale.

const IS_UPPER: u16 = 1 << 0;
const IS_LOWER: u16 = 1 << 1;
const IS_ALPHA: u16 = 1 << 2;
const IS_DIGIT: u16 = 1 << 3;
const IS_XDIGIT: u16 = 1 << 4;
const IS_SPACE: u16 = 1 << 5;
const IS_PRINT: u16 = 1 << 6;
const IS_GRAPH: u16 = 1 << 7;
const IS_BLANK: u16 = 1 << 8;
const IS_CNTRL: u16 = 1 << 9;
const IS_PUNCT: u16 = 1 << 10;

/// `ctype_base` masks (libstdc++ config/os/gnu-linux/ctype_base.h).
const MASK_ALNUM: u16 = IS_ALPHA | IS_DIGIT;
const MASK_GRAPH: u16 = IS_ALPHA | IS_DIGIT | IS_PUNCT;

/// The C-locale classification bits of a byte.
fn ctype_bits(c: u8) -> u16 {
    let mut bits = 0;
    if c.is_ascii_uppercase() {
        bits |= IS_UPPER | IS_ALPHA;
    }
    if c.is_ascii_lowercase() {
        bits |= IS_LOWER | IS_ALPHA;
    }
    if c.is_ascii_digit() {
        bits |= IS_DIGIT;
    }
    if c.is_ascii_hexdigit() {
        bits |= IS_XDIGIT;
    }
    if matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        bits |= IS_SPACE;
    }
    if (0x20..=0x7e).contains(&c) {
        bits |= IS_PRINT;
    }
    if (0x21..=0x7e).contains(&c) {
        bits |= IS_GRAPH;
    }
    if c == b' ' || c == b'\t' {
        bits |= IS_BLANK;
    }
    if c < 0x20 || c == 0x7f {
        bits |= IS_CNTRL;
    }
    if c.is_ascii_punctuation() {
        bits |= IS_PUNCT;
    }
    bits
}

/// `ctype<char>::is (mask, c)`.
fn ctype_is(mask: u16, c: u8) -> bool {
    ctype_bits(c) & mask != 0
}

/// `regex_traits::char_class_type`: a ctype mask plus the `[[:w:]]`
/// underscore flag.
#[derive(Clone, Copy, Default)]
struct ClassMask {
    base: u16,
    under: bool,
}

/// `regex_traits::lookup_classname`.
fn lookup_classname(name: &[u8], icase: bool) -> Option<ClassMask> {
    let lower: Vec<u8> = name.iter().map(u8::to_ascii_lowercase).collect();
    let mask = match lower.as_slice() {
        b"d" => ClassMask { base: IS_DIGIT, under: false },
        b"w" => ClassMask { base: MASK_ALNUM, under: true },
        b"s" => ClassMask { base: IS_SPACE, under: false },
        b"alnum" => ClassMask { base: MASK_ALNUM, under: false },
        b"alpha" => ClassMask { base: IS_ALPHA, under: false },
        b"blank" => ClassMask { base: IS_BLANK, under: false },
        b"cntrl" => ClassMask { base: IS_CNTRL, under: false },
        b"digit" => ClassMask { base: IS_DIGIT, under: false },
        b"graph" => ClassMask { base: MASK_GRAPH, under: false },
        b"lower" => ClassMask { base: IS_LOWER, under: false },
        b"print" => ClassMask { base: IS_PRINT, under: false },
        b"punct" => ClassMask { base: IS_PUNCT, under: false },
        b"space" => ClassMask { base: IS_SPACE, under: false },
        b"upper" => ClassMask { base: IS_UPPER, under: false },
        b"xdigit" => ClassMask { base: IS_XDIGIT, under: false },
        _ => return None,
    };
    if icase && mask.base & (IS_LOWER | IS_UPPER) != 0 {
        return Some(ClassMask { base: IS_ALPHA, under: false });
    }
    Some(mask)
}

/// `regex_traits::isctype`.
fn isctype(c: u8, f: ClassMask) -> bool {
    ctype_is(f.base, c) || (f.under && c == b'_')
}

/// `regex_traits::lookup_collatename` name table: the index is the character.
const COLLATE_NAMES: [&str; 128] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "alert", "backspace", "tab", "newline", "vertical-tab",
    "form-feed", "carriage-return", "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN",
    "EM", "SUB", "ESC", "IS4", "IS3", "IS2", "IS1", "space", "exclamation-mark", "quotation-mark", "number-sign",
    "dollar-sign", "percent-sign", "ampersand", "apostrophe", "left-parenthesis", "right-parenthesis", "asterisk",
    "plus-sign", "comma", "hyphen", "period", "slash", "zero", "one", "two", "three", "four", "five", "six",
    "seven", "eight", "nine", "colon", "semicolon", "less-than-sign", "equals-sign", "greater-than-sign",
    "question-mark", "commercial-at", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O",
    "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "left-square-bracket", "backslash",
    "right-square-bracket", "circumflex", "underscore", "grave-accent", "a", "b", "c", "d", "e", "f", "g", "h",
    "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    "left-curly-bracket", "vertical-line", "right-curly-bracket", "tilde", "DEL",
];

/// `regex_traits::lookup_collatename`: the single character named `name`.
fn lookup_collatename(name: &[u8]) -> Option<u8> {
    COLLATE_NAMES.iter().position(|n| n.as_bytes() == name).map(|i| i as u8)
}

////////////////////////////////////////////////////////////////////////////////
// _Scanner (regex_scanner.h / regex_scanner.tcc), ECMAScript grammar.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Token {
    Anychar,
    Backref,
    BracketBegin,
    BracketDash,
    BracketEnd,
    BracketNegBegin,
    CharClassName,
    Closure0,
    Closure1,
    Collsymbol,
    Comma,
    DupCount,
    Eof,
    EquivClassName,
    HexNum,
    IntervalBegin,
    IntervalEnd,
    LineBegin,
    LineEnd,
    Opt,
    Or,
    OrdChar,
    QuotedClass,
    SubexprBegin,
    SubexprEnd,
    SubexprLookaheadBegin,
    SubexprNoGroupBegin,
    WordBound,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanState {
    Normal,
    InBracket,
    InBrace,
}

/// `_M_ecma_spec_char`.
const ECMA_SPEC_CHAR: &[u8] = b"^$\\.*+?()[]{}|";

/// `_M_ecma_escape_tbl`.
const ECMA_ESCAPE_TBL: [(u8, u8); 7] =
    [(b'0', 0), (b'b', 0x08), (b'f', 0x0c), (b'n', b'\n'), (b'r', b'\r'), (b't', b'\t'), (b'v', 0x0b)];

type Res<T> = std::result::Result<T, String>;

fn err<T>(message: &str) -> Res<T> {
    Err(message.to_string())
}

struct Scanner<'a> {
    input: &'a [u8],
    current: usize,
    state: ScanState,
    token: Token,
    value: Vec<u8>,
    at_bracket_start: bool,
}

impl<'a> Scanner<'a> {
    fn new(input: &'a [u8]) -> Res<Self> {
        let mut scanner = Scanner {
            input,
            current: 0,
            state: ScanState::Normal,
            token: Token::Eof,
            value: Vec::new(),
            at_bracket_start: false,
        };
        scanner.advance()?;
        Ok(scanner)
    }

    fn at_end(&self) -> bool {
        self.current == self.input.len()
    }

    /// `*_M_current`; the end reads the terminating NUL of the `std::string`.
    fn peek(&self) -> u8 {
        self.input.get(self.current).copied().unwrap_or(0)
    }

    fn next_char(&mut self) -> u8 {
        let c = self.input[self.current];
        self.current += 1;
        c
    }

    fn ord_char(&mut self, c: u8) {
        self.token = Token::OrdChar;
        self.value = vec![c];
    }

    /// `_M_advance`.
    fn advance(&mut self) -> Res<()> {
        if self.at_end() {
            self.token = Token::Eof;
            return Ok(());
        }
        match self.state {
            ScanState::Normal => self.scan_normal(),
            ScanState::InBracket => self.scan_in_bracket(),
            ScanState::InBrace => self.scan_in_brace(),
        }
    }

    /// `_M_scan_normal`.
    fn scan_normal(&mut self) -> Res<()> {
        let c = self.next_char();

        // `std::strchr (_M_spec_char, c)` also finds the terminating NUL.
        if c != 0 && !ECMA_SPEC_CHAR.contains(&c) {
            self.ord_char(c);
            return Ok(());
        }
        if c == b'\\' {
            if self.at_end() {
                return err("Unexpected end of regex when escaping.");
            }
            return self.eat_escape_ecma();
        }
        if c == b'(' {
            if self.peek() == b'?' {
                self.current += 1;
                if self.at_end() {
                    return err("Unexpected end of regex when in an open parenthesis.");
                }
                match self.peek() {
                    b':' => {
                        self.current += 1;
                        self.token = Token::SubexprNoGroupBegin;
                    }
                    b'=' => {
                        self.current += 1;
                        self.token = Token::SubexprLookaheadBegin;
                        self.value = vec![b'p'];
                    }
                    b'!' => {
                        self.current += 1;
                        self.token = Token::SubexprLookaheadBegin;
                        self.value = vec![b'n'];
                    }
                    _ => return err("Invalid special open parenthesis."),
                }
            } else {
                self.token = Token::SubexprBegin;
            }
        } else if c == b')' {
            self.token = Token::SubexprEnd;
        } else if c == b'[' {
            self.state = ScanState::InBracket;
            self.at_bracket_start = true;
            if !self.at_end() && self.peek() == b'^' {
                self.token = Token::BracketNegBegin;
                self.current += 1;
            } else {
                self.token = Token::BracketBegin;
            }
        } else if c == b'{' {
            self.state = ScanState::InBrace;
            self.token = Token::IntervalBegin;
        } else if c == 0 {
            self.ord_char(c);
        } else if c != b']' && c != b'}' {
            self.token = match c {
                b'^' => Token::LineBegin,
                b'$' => Token::LineEnd,
                b'.' => Token::Anychar,
                b'*' => Token::Closure0,
                b'+' => Token::Closure1,
                b'?' => Token::Opt,
                _ => Token::Or, // '|'
            };
        } else {
            self.ord_char(c);
        }
        Ok(())
    }

    /// `_M_scan_in_bracket`.
    fn scan_in_bracket(&mut self) -> Res<()> {
        if self.at_end() {
            return err("Unexpected end of regex when in bracket expression.");
        }
        let c = self.next_char();

        if c == b'-' {
            self.token = Token::BracketDash;
        } else if c == b'[' {
            if self.at_end() {
                return err("Unexpected character class open bracket.");
            }
            match self.peek() {
                b'.' => {
                    self.token = Token::Collsymbol;
                    self.current += 1;
                    self.eat_class(b'.')?;
                }
                b':' => {
                    self.token = Token::CharClassName;
                    self.current += 1;
                    self.eat_class(b':')?;
                }
                b'=' => {
                    self.token = Token::EquivClassName;
                    self.current += 1;
                    self.eat_class(b'=')?;
                }
                _ => self.ord_char(c),
            }
        } else if c == b']' {
            // ECMAScript: "[]" is empty and "[^]" matches everything.
            self.token = Token::BracketEnd;
            self.state = ScanState::Normal;
        } else if c == b'\\' {
            self.eat_escape_ecma()?;
        } else {
            self.ord_char(c);
        }
        self.at_bracket_start = false;
        Ok(())
    }

    /// `_M_scan_in_brace`.
    fn scan_in_brace(&mut self) -> Res<()> {
        if self.at_end() {
            return err("Unexpected end of regex when in brace expression.");
        }
        let c = self.next_char();

        if c.is_ascii_digit() {
            self.token = Token::DupCount;
            self.value = vec![c];
            while !self.at_end() && self.peek().is_ascii_digit() {
                let d = self.next_char();
                self.value.push(d);
            }
        } else if c == b',' {
            self.token = Token::Comma;
        } else if c == b'}' {
            self.state = ScanState::Normal;
            self.token = Token::IntervalEnd;
        } else {
            return err("Unexpected character in brace expression.");
        }
        Ok(())
    }

    /// `_M_eat_escape_ecma`.
    fn eat_escape_ecma(&mut self) -> Res<()> {
        if self.at_end() {
            return err("Unexpected end of regex when escaping.");
        }
        let c = self.next_char();
        let pos = ECMA_ESCAPE_TBL.iter().find(|(k, _)| *k == c).map(|(_, v)| *v);

        if let Some(v) = pos.filter(|_| c != b'b' || self.state == ScanState::InBracket) {
            self.ord_char(v);
        } else if c == b'b' {
            self.token = Token::WordBound;
            self.value = vec![b'p'];
        } else if c == b'B' {
            self.token = Token::WordBound;
            self.value = vec![b'n'];
        } else if matches!(c, b'd' | b'D' | b's' | b'S' | b'w' | b'W') {
            self.token = Token::QuotedClass;
            self.value = vec![c];
        } else if c == b'c' {
            if self.at_end() {
                return err("Unexpected end of regex when reading control code.");
            }
            let x = self.next_char();
            self.ord_char(x);
        } else if c == b'x' || c == b'u' {
            self.value.clear();
            for _ in 0..(if c == b'x' { 2 } else { 4 }) {
                if self.at_end() || !self.peek().is_ascii_hexdigit() {
                    return err("Unexpected end of regex when ascii character.");
                }
                let d = self.next_char();
                self.value.push(d);
            }
            self.token = Token::HexNum;
        } else if c.is_ascii_digit() {
            // ECMAScript recognizes multi-digit back-references.
            self.value = vec![c];
            while !self.at_end() && self.peek().is_ascii_digit() {
                let d = self.next_char();
                self.value.push(d);
            }
            self.token = Token::Backref;
        } else {
            self.ord_char(c);
        }
        Ok(())
    }

    /// `_M_eat_class`: reads up to `ch` followed by `]`.
    fn eat_class(&mut self, ch: u8) -> Res<()> {
        self.value.clear();
        while !self.at_end() && self.peek() != ch {
            let c = self.next_char();
            self.value.push(c);
        }
        let ok = !self.at_end() && self.next_char() == ch && !self.at_end() && self.next_char() == b']';
        if !ok {
            // error_ctype for ':', error_collate otherwise; same text.
            return err("Unexpected end of character class.");
        }
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////
// _NFA (regex_automaton.h / .tcc): only the graph shape is modelled, to
// reproduce the state count (and the clones made by `{n,m}`) of libstdc++.

#[derive(Clone)]
struct NState {
    /// `_M_has_alt ()`: alternative, repeat and lookahead states.
    has_alt: bool,
    next: Option<usize>,
    alt: Option<usize>,
}

struct Nfa {
    states: Vec<NState>,
    subexpr_count: usize,
    paren_stack: Vec<usize>,
}

impl Nfa {
    /// `_M_insert_state`.
    fn insert(&mut self, has_alt: bool, next: Option<usize>, alt: Option<usize>) -> Res<usize> {
        self.states.push(NState { has_alt, next, alt });
        if self.states.len() > STATE_LIMIT {
            return err(NFA_LIMIT);
        }
        Ok(self.states.len() - 1)
    }

    /// Dummy, accept, matcher, line and word assertions.
    fn insert_plain(&mut self) -> Res<usize> {
        self.insert(false, None, None)
    }

    fn insert_alt(&mut self, next: usize, alt: usize) -> Res<usize> {
        self.insert(true, Some(next), Some(alt))
    }

    fn insert_repeat(&mut self, next: Option<usize>, alt: usize) -> Res<usize> {
        self.insert(true, next, Some(alt))
    }

    fn insert_lookahead(&mut self, alt: usize) -> Res<usize> {
        self.insert(true, None, Some(alt))
    }

    fn insert_subexpr_begin(&mut self) -> Res<usize> {
        let id = self.subexpr_count;
        self.subexpr_count += 1;
        self.paren_stack.push(id);
        self.insert_plain()
    }

    fn insert_subexpr_end(&mut self) -> Res<usize> {
        self.paren_stack.pop();
        self.insert_plain()
    }

    /// `_M_insert_backref`.
    fn insert_backref(&mut self, index: usize) -> Res<usize> {
        if index >= self.subexpr_count {
            return err("Back-reference index exceeds current sub-expression count.");
        }
        if self.paren_stack.contains(&index) {
            return err("Back-reference referred to an opened sub-expression.");
        }
        self.insert_plain()
    }

    /// `_StateSeq::_M_append (_StateIdT)`.
    fn append_id(&mut self, seq: &mut Seq, id: usize) {
        self.states[seq.end].next = Some(id);
        seq.end = id;
    }

    /// `_StateSeq::_M_append (const _StateSeq&)`, plus the pattern text.
    fn append(&mut self, seq: &mut Seq, other: Seq) {
        self.states[seq.end].next = Some(other.start);
        seq.end = other.end;
        seq.re.push_str(&other.re);
        seq.atomic = false;
    }

    /// `_StateSeq::_M_clone`.
    fn clone_seq(&mut self, seq: &Seq) -> Res<(usize, usize)> {
        let mut map: BTreeMap<usize, usize> = BTreeMap::new();
        let mut stack = vec![seq.start];
        while let Some(u) = stack.pop() {
            let dup = self.states[u].clone();
            let id = self.insert(dup.has_alt, dup.next, dup.alt)?;
            map.insert(u, id);
            if dup.has_alt {
                if let Some(alt) = dup.alt.filter(|a| !map.contains_key(a)) {
                    stack.push(alt);
                }
            }
            if u == seq.end {
                continue;
            }
            if let Some(next) = dup.next.filter(|n| !map.contains_key(n)) {
                stack.push(next);
            }
        }
        for &v in map.values() {
            if let Some(next) = self.states[v].next {
                if let Some(&mapped) = map.get(&next) {
                    self.states[v].next = Some(mapped);
                }
            }
            if self.states[v].has_alt {
                if let Some(alt) = self.states[v].alt {
                    if let Some(&mapped) = map.get(&alt) {
                        self.states[v].alt = Some(mapped);
                    }
                }
            }
        }
        Ok((map[&seq.start], map[&seq.end]))
    }
}

/// `_StateSeq`, carrying the translated pattern text of the sequence.
struct Seq {
    start: usize,
    end: usize,
    re: String,
    /// The text is a single `regex` atom that a quantifier can follow.
    atomic: bool,
}

impl Seq {
    fn single(id: usize, re: String, atomic: bool) -> Self {
        Seq { start: id, end: id, re, atomic }
    }

    /// The text as an operand of a quantifier.
    fn operand(&self) -> String {
        if self.atomic {
            self.re.clone()
        } else {
            format!("(?:{})", self.re)
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Matchers (regex_compiler.h): each evaluated over all 256 bytes.

/// `_RegexTranslator::_M_translate`: `translate_nocase` (C `tolower`) when
/// icase, identity otherwise.
fn translate_char(icase: bool, c: u8) -> u8 {
    if icase {
        c.to_ascii_lowercase()
    } else {
        c
    }
}

/// `_BracketMatcher`.
struct BracketMatcher {
    icase: bool,
    is_non_matching: bool,
    char_set: Vec<u8>,
    range_set: Vec<(i8, i8)>,
    class_set: ClassMask,
    equiv_set: Vec<u8>,
    neg_class_set: Vec<ClassMask>,
}

impl BracketMatcher {
    fn new(is_non_matching: bool, icase: bool) -> Self {
        BracketMatcher {
            icase,
            is_non_matching,
            char_set: Vec::new(),
            range_set: Vec::new(),
            class_set: ClassMask::default(),
            equiv_set: Vec::new(),
            neg_class_set: Vec::new(),
        }
    }

    fn add_char(&mut self, c: u8) {
        self.char_set.push(translate_char(self.icase, c));
    }

    fn add_collate_element(&mut self, s: &[u8]) -> Res<u8> {
        let Some(st) = lookup_collatename(s) else {
            return err("Invalid collate element.");
        };
        self.char_set.push(translate_char(self.icase, st));
        Ok(st)
    }

    /// `transform_primary` in the C locale is `tolower`.
    fn add_equivalence_class(&mut self, s: &[u8]) -> Res<()> {
        let Some(st) = lookup_collatename(s) else {
            return err("Invalid equivalence class.");
        };
        self.equiv_set.push(st.to_ascii_lowercase());
        Ok(())
    }

    /// `__neg` is true for `\D`, `\S` and `\W` inside a bracket only.
    fn add_character_class(&mut self, s: &[u8], neg: bool) -> Res<()> {
        let Some(mask) = lookup_classname(s, self.icase) else {
            return err("Invalid character class.");
        };
        if !neg {
            self.class_set.base |= mask.base;
            self.class_set.under |= mask.under;
        } else {
            self.neg_class_set.push(mask);
        }
        Ok(())
    }

    /// `char` is signed: bytes >= 0x80 compare below ASCII.
    fn make_range(&mut self, l: u8, r: u8) -> Res<()> {
        if (l as i8) > (r as i8) {
            return err("Invalid range in bracket expression.");
        }
        self.range_set.push((l as i8, r as i8));
        Ok(())
    }

    /// `_RegexTranslator::_M_match_range`.
    fn match_range(&self, first: i8, last: i8, ch: u8) -> bool {
        if !self.icase {
            let c = ch as i8;
            return first <= c && c <= last;
        }
        let lower = ch.to_ascii_lowercase() as i8;
        let upper = ch.to_ascii_uppercase() as i8;
        (first <= lower && lower <= last) || (first <= upper && upper <= last)
    }

    /// `_M_apply (ch, false_type)`.
    fn apply(&self, ch: u8) -> bool {
        let found = (|| {
            if self.char_set.contains(&translate_char(self.icase, ch)) {
                return true;
            }
            if self.range_set.iter().any(|&(f, l)| self.match_range(f, l, ch)) {
                return true;
            }
            if isctype(ch, self.class_set) {
                return true;
            }
            if self.equiv_set.contains(&ch.to_ascii_lowercase()) {
                return true;
            }
            self.neg_class_set.iter().any(|&m| !isctype(ch, m))
        })();
        found ^ self.is_non_matching
    }

    /// `_M_make_cache`.
    fn cache(&self) -> [bool; 256] {
        let mut set = [false; 256];
        for (b, slot) in set.iter_mut().enumerate() {
            *slot = self.apply(b as u8);
        }
        set
    }
}

/// `regex` syntax for a set of bytes (Unicode disabled).
fn byte_set_re(set: &[bool; 256]) -> String {
    let members: Vec<u8> = (0..=255u8).filter(|&b| set[b as usize]).collect();
    if members.len() == 1 {
        let b = members[0];
        return if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("\\x{b:02X}") };
    }
    if members.is_empty() {
        return "[^\\x00-\\xFF]".to_string();
    }
    let mut re = String::from("[");
    let mut i = 0;
    while i < members.len() {
        let lo = members[i];
        let mut hi = lo;
        while i + 1 < members.len() && members[i + 1] == hi + 1 {
            i += 1;
            hi = members[i];
        }
        if lo == hi {
            re.push_str(&format!("\\x{lo:02X}"));
        } else {
            re.push_str(&format!("\\x{lo:02X}-\\x{hi:02X}"));
        }
        i += 1;
    }
    re.push(']');
    re
}

////////////////////////////////////////////////////////////////////////////////
// _Compiler (regex_compiler.tcc), ECMAScript grammar.

/// `_BracketState`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BracketState {
    None,
    Char(u8),
    Class,
}

struct Compiler<'a> {
    scanner: Scanner<'a>,
    icase: bool,
    value: Vec<u8>,
    stack: Vec<Seq>,
    nfa: Nfa,
    /// First construct the `regex` crate cannot express.
    unsupported: Option<&'static str>,
}

impl<'a> Compiler<'a> {
    /// `_Compiler::_Compiler`.
    fn compile(pattern: &'a [u8], icase: bool) -> Res<String> {
        let scanner = Scanner::new(pattern)?;
        let mut c = Compiler {
            scanner,
            icase,
            value: Vec::new(),
            stack: Vec::new(),
            nfa: Nfa { states: Vec::new(), subexpr_count: 0, paren_stack: Vec::new() },
            unsupported: None,
        };
        // `_StateSeqT __r (*_M_nfa, _M_nfa->_M_start ())`, start state 0.
        let mut r = Seq::single(0, String::new(), false);
        let begin = c.nfa.insert_subexpr_begin()?;
        c.nfa.append_id(&mut r, begin);
        c.disjunction()?;
        if !c.match_token(Token::Eof)? {
            return err(PAREN_MISMATCH);
        }
        let body = c.pop();
        c.nfa.append(&mut r, body);
        let end = c.nfa.insert_subexpr_end()?;
        c.nfa.append_id(&mut r, end);
        let accept = c.nfa.insert_plain()?;
        c.nfa.append_id(&mut r, accept);
        if let Some(message) = c.unsupported {
            return err(message);
        }
        Ok(r.re)
    }

    fn pop(&mut self) -> Seq {
        self.stack.pop().expect("regex compiler stack underflow")
    }

    fn note_unsupported(&mut self, message: &'static str) {
        self.unsupported.get_or_insert(message);
    }

    /// `_M_match_token`.
    fn match_token(&mut self, token: Token) -> Res<bool> {
        if token == self.scanner.token {
            self.value = self.scanner.value.clone();
            self.scanner.advance()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// `_M_disjunction`.
    fn disjunction(&mut self) -> Res<()> {
        self.alternative()?;
        let mut alternatives: Vec<String> = Vec::new();
        while self.match_token(Token::Or)? {
            let mut alt1 = self.pop();
            if alternatives.is_empty() {
                alternatives.push(std::mem::take(&mut alt1.re));
            }
            self.alternative()?;
            let mut alt2 = self.pop();
            alternatives.push(std::mem::take(&mut alt2.re));
            let end = self.nfa.insert_plain()?;
            self.nfa.append_id(&mut alt1, end);
            self.nfa.append_id(&mut alt2, end);
            // alt2 is `_M_next`, alt1 is `_M_alt`: alt1 is tried first.
            let alt = self.nfa.insert_alt(alt2.start, alt1.start)?;
            self.stack.push(Seq { start: alt, end, re: String::new(), atomic: true });
        }
        if !alternatives.is_empty() {
            let top = self.stack.last_mut().expect("regex compiler stack underflow");
            top.re = format!("(?:{})", alternatives.join("|"));
        }
        Ok(())
    }

    /// `_M_alternative`, with the right recursion unrolled: the terms are
    /// compiled left to right, then the closing dummy, then the sequences
    /// are appended right to left, as the recursive C++ does.
    fn alternative(&mut self) -> Res<()> {
        let mut terms: Vec<Seq> = Vec::new();
        while self.term()? {
            terms.push(self.pop());
        }
        let dummy = self.nfa.insert_plain()?;
        let mut acc = Seq::single(dummy, String::new(), false);
        while let Some(mut re) = terms.pop() {
            self.nfa.append(&mut re, acc);
            acc = re;
        }
        acc.atomic = false;
        self.stack.push(acc);
        Ok(())
    }

    /// `_M_term`.
    fn term(&mut self) -> Res<bool> {
        if self.assertion()? {
            return Ok(true);
        }
        if self.atom()? {
            while self.quantifier()? {}
            return Ok(true);
        }
        Ok(false)
    }

    /// `_M_assertion`.
    fn assertion(&mut self) -> Res<bool> {
        if self.match_token(Token::LineBegin)? {
            let id = self.nfa.insert_plain()?;
            self.stack.push(Seq::single(id, "^".into(), false));
        } else if self.match_token(Token::LineEnd)? {
            let id = self.nfa.insert_plain()?;
            self.stack.push(Seq::single(id, "$".into(), false));
        } else if self.match_token(Token::WordBound)? {
            // `_M_value[0] == 'n'` means "not word boundary".
            let re = if self.value[0] == b'n' { "\\B" } else { "\\b" };
            let id = self.nfa.insert_plain()?;
            self.stack.push(Seq::single(id, re.into(), false));
        } else if self.match_token(Token::SubexprLookaheadBegin)? {
            self.disjunction()?;
            if !self.match_token(Token::SubexprEnd)? {
                return err("Parenthesis is not closed.");
            }
            let mut tmp = self.pop();
            let accept = self.nfa.insert_plain()?;
            self.nfa.append_id(&mut tmp, accept);
            let id = self.nfa.insert_lookahead(tmp.start)?;
            self.note_unsupported(LOOKAHEAD_UNSUPPORTED);
            self.stack.push(Seq::single(id, String::new(), false));
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    /// `_M_quantifier`.
    fn quantifier(&mut self) -> Res<bool> {
        // ECMAScript: a trailing '?' makes the quantifier non-greedy.
        let lazy = |c: &mut Self| -> Res<&'static str> {
            if c.stack.is_empty() {
                return err("Nothing to repeat before a quantifier.");
            }
            Ok(if c.match_token(Token::Opt)? { "?" } else { "" })
        };
        if self.match_token(Token::Closure0)? {
            let neg = lazy(self)?;
            let mut e = self.pop();
            let id = self.nfa.insert_repeat(None, e.start)?;
            let r = Seq::single(id, format!("{}*{}", e.operand(), neg), false);
            self.nfa.append_id(&mut e, id);
            self.stack.push(r);
        } else if self.match_token(Token::Closure1)? {
            let neg = lazy(self)?;
            let mut e = self.pop();
            let id = self.nfa.insert_repeat(None, e.start)?;
            self.nfa.append_id(&mut e, id);
            e.re = format!("{}+{}", e.operand(), neg);
            e.atomic = false;
            self.stack.push(e);
        } else if self.match_token(Token::Opt)? {
            let neg = lazy(self)?;
            let mut e = self.pop();
            let end = self.nfa.insert_plain()?;
            let id = self.nfa.insert_repeat(None, e.start)?;
            let mut r = Seq::single(id, format!("{}?{}", e.operand(), neg), false);
            self.nfa.append_id(&mut e, end);
            self.nfa.append_id(&mut r, end);
            self.stack.push(r);
        } else if self.match_token(Token::IntervalBegin)? {
            if self.stack.is_empty() {
                return err("Nothing to repeat before a quantifier.");
            }
            if !self.match_token(Token::DupCount)? {
                return err("Unexpected token in brace expression.");
            }
            let r = self.pop();
            let dummy = self.nfa.insert_plain()?;
            let mut e = Seq::single(dummy, String::new(), false);
            let min_rep = self.cur_int_value(10)?;
            let mut infi = false;
            let mut n: i64 = 0;
            let mut max_rep = min_rep;
            // {3
            if self.match_token(Token::Comma)? {
                if self.match_token(Token::DupCount)? {
                    // {3,7}
                    max_rep = self.cur_int_value(10)?;
                    n = i64::from(max_rep) - i64::from(min_rep);
                } else {
                    infi = true;
                }
            }
            if !self.match_token(Token::IntervalEnd)? {
                return err("Unexpected end of brace expression.");
            }
            let neg = if self.match_token(Token::Opt)? { "?" } else { "" };
            for _ in 0..min_rep {
                let (s, t) = self.nfa.clone_seq(&r)?;
                self.nfa.states[e.end].next = Some(s);
                e.end = t;
            }
            if infi {
                let (s, t) = self.nfa.clone_seq(&r)?;
                let id = self.nfa.insert_repeat(None, s)?;
                self.nfa.states[t].next = Some(id);
                self.nfa.append_id(&mut e, id);
            } else {
                if n < 0 {
                    return err("Invalid range in brace expression.");
                }
                let end = self.nfa.insert_plain()?;
                let mut alts: Vec<usize> = Vec::new();
                for _ in 0..n {
                    let (s, t) = self.nfa.clone_seq(&r)?;
                    let alt = self.nfa.insert_repeat(Some(s), end)?;
                    alts.push(alt);
                    self.nfa.states[e.end].next = Some(alt);
                    e.end = t;
                }
                self.nfa.append_id(&mut e, end);
                while let Some(id) = alts.pop() {
                    let state = &mut self.nfa.states[id];
                    std::mem::swap(&mut state.next, &mut state.alt);
                }
            }
            let bounds = if infi {
                format!("{min_rep},")
            } else if max_rep == min_rep {
                format!("{min_rep}")
            } else {
                format!("{min_rep},{max_rep}")
            };
            e.re = format!("{}{{{}}}{}", r.operand(), bounds, neg);
            self.stack.push(e);
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    /// Pushes a single-character matcher.
    fn push_matcher(&mut self, set: &[bool; 256]) -> Res<()> {
        let id = self.nfa.insert_plain()?;
        self.stack.push(Seq::single(id, byte_set_re(set), true));
        Ok(())
    }

    /// `_M_atom`.
    fn atom(&mut self) -> Res<bool> {
        if self.match_token(Token::Anychar)? {
            // `_AnyMatcher<_TraitsT, true, ...>`: not '\n', not '\r'.
            let icase = self.icase;
            let mut set = [false; 256];
            for (b, slot) in set.iter_mut().enumerate() {
                let c = translate_char(icase, b as u8);
                *slot = c != translate_char(icase, b'\n') && c != translate_char(icase, b'\r');
            }
            self.push_matcher(&set)?;
        } else if self.try_char()? {
            // `_CharMatcher`.
            let icase = self.icase;
            let ch = translate_char(icase, self.value[0]);
            let mut set = [false; 256];
            for (b, slot) in set.iter_mut().enumerate() {
                *slot = translate_char(icase, b as u8) == ch;
            }
            self.push_matcher(&set)?;
        } else if self.match_token(Token::Backref)? {
            let index = self.cur_int_value(10)?;
            let id = self.nfa.insert_backref(index as usize)?;
            self.note_unsupported(BACKREF_UNSUPPORTED);
            self.stack.push(Seq::single(id, String::new(), false));
        } else if self.match_token(Token::QuotedClass)? {
            let mut matcher = BracketMatcher::new(self.value[0].is_ascii_uppercase(), self.icase);
            let value = self.value.clone();
            matcher.add_character_class(&value, false)?;
            self.push_matcher(&matcher.cache())?;
        } else if self.match_token(Token::SubexprNoGroupBegin)? {
            let dummy = self.nfa.insert_plain()?;
            let mut r = Seq::single(dummy, String::new(), false);
            self.disjunction()?;
            if !self.match_token(Token::SubexprEnd)? {
                return err("Parenthesis is not closed.");
            }
            let body = self.pop();
            self.nfa.append(&mut r, body);
            r.re = format!("(?:{})", r.re);
            r.atomic = true;
            self.stack.push(r);
        } else if self.match_token(Token::SubexprBegin)? {
            let begin = self.nfa.insert_subexpr_begin()?;
            let mut r = Seq::single(begin, String::new(), false);
            self.disjunction()?;
            if !self.match_token(Token::SubexprEnd)? {
                return err("Parenthesis is not closed.");
            }
            let body = self.pop();
            self.nfa.append(&mut r, body);
            let end = self.nfa.insert_subexpr_end()?;
            self.nfa.append_id(&mut r, end);
            // Groups are never read back, so they do not capture.
            r.re = format!("(?:{})", r.re);
            r.atomic = true;
            self.stack.push(r);
        } else if !self.bracket_expression()? {
            return Ok(false);
        }
        Ok(true)
    }

    /// `_M_bracket_expression`.
    fn bracket_expression(&mut self) -> Res<bool> {
        let neg = self.match_token(Token::BracketNegBegin)?;
        if !(neg || self.match_token(Token::BracketBegin)?) {
            return Ok(false);
        }
        self.insert_bracket_matcher(neg)?;
        Ok(true)
    }

    /// `_M_insert_bracket_matcher`.
    fn insert_bracket_matcher(&mut self, neg: bool) -> Res<()> {
        let mut matcher = BracketMatcher::new(neg, self.icase);
        let mut last_char = BracketState::None;
        if self.try_char()? {
            last_char = BracketState::Char(self.value[0]);
        } else if self.match_token(Token::BracketDash)? {
            // Dash as first character is a normal character.
            last_char = BracketState::Char(b'-');
        }
        while self.expression_term(&mut last_char, &mut matcher)? {}
        if let BracketState::Char(c) = last_char {
            matcher.add_char(c);
        }
        self.push_matcher(&matcher.cache())
    }

    /// `_M_expression_term`.
    fn expression_term(&mut self, last_char: &mut BracketState, matcher: &mut BracketMatcher) -> Res<bool> {
        if self.match_token(Token::BracketEnd)? {
            return Ok(false);
        }

        // Add any previously cached char into the matcher and update cache.
        let push_char = |last_char: &mut BracketState, matcher: &mut BracketMatcher, ch: u8| {
            if let BracketState::Char(c) = *last_char {
                matcher.add_char(c);
            }
            *last_char = BracketState::Char(ch);
        };
        // Record that the last thing processed was a class.
        let push_class = |last_char: &mut BracketState, matcher: &mut BracketMatcher| {
            if let BracketState::Char(c) = *last_char {
                matcher.add_char(c);
            }
            *last_char = BracketState::Class;
        };

        if self.match_token(Token::Collsymbol)? {
            let value = self.value.clone();
            let symbol = matcher.add_collate_element(&value)?;
            // Every libstdc++ collating name is a single character.
            push_char(last_char, matcher, symbol);
        } else if self.match_token(Token::EquivClassName)? {
            push_class(last_char, matcher);
            let value = self.value.clone();
            matcher.add_equivalence_class(&value)?;
        } else if self.match_token(Token::CharClassName)? {
            push_class(last_char, matcher);
            let value = self.value.clone();
            matcher.add_character_class(&value, false)?;
        } else if self.try_char()? {
            let ch = self.value[0];
            push_char(last_char, matcher, ch);
        }
        // ECMAScript treats a '-' after a range as a normal character.
        else if self.match_token(Token::BracketDash)? {
            if self.match_token(Token::BracketEnd)? {
                // For "-]" the dash is a literal character.
                push_char(last_char, matcher, b'-');
                return Ok(false);
            } else if *last_char == BracketState::Class {
                // "\\w-" is invalid, start of range must be a single char.
                return err("Invalid start of range in bracket expression.");
            } else if let BracketState::Char(first) = *last_char {
                if self.try_char()? {
                    // "x-y"
                    matcher.make_range(first, self.value[0])?;
                    *last_char = BracketState::None;
                } else if self.match_token(Token::BracketDash)? {
                    // "x--"
                    matcher.make_range(first, b'-')?;
                    *last_char = BracketState::None;
                } else {
                    return err("Invalid end of range in bracket expression.");
                }
            } else {
                // A dash that is not part of an existing range.
                push_char(last_char, matcher, b'-');
            }
        } else if self.match_token(Token::QuotedClass)? {
            push_class(last_char, matcher);
            let value = self.value.clone();
            matcher.add_character_class(&value, value[0].is_ascii_uppercase())?;
        } else {
            return err("Unexpected character in bracket expression.");
        }
        Ok(true)
    }

    /// `_M_try_char` (ECMAScript never produces `_S_token_oct_num`).
    fn try_char(&mut self) -> Res<bool> {
        if self.match_token(Token::HexNum)? {
            // `_M_value.assign (1, int)` keeps the low byte.
            let v = self.cur_int_value(16)?;
            self.value = vec![v as u8];
            return Ok(true);
        }
        self.match_token(Token::OrdChar)
    }

    /// `_M_cur_int_value`.
    fn cur_int_value(&self, radix: u32) -> Res<i32> {
        let mut v: i32 = 0;
        for &c in &self.value {
            let digit = (c as char).to_digit(radix).map_or(-1, |d| d as i32);
            v = match v.checked_mul(radix as i32).and_then(|x| x.checked_add(digit)) {
                Some(x) => x,
                None => return err("invalid back reference"),
            };
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    //! Port of libshared (d899eba6) `test/rx.t.cpp`, plus comparisons against
    //! the C++ `RX` built with GCC 11 libstdc++ on Linux.
    //!
    //! The C++ `UnitTest` harness is mirrored by [`UnitTest`]: every `ut.ok`,
    //! `ut.is` and `ut.skip` of rx.t.cpp becomes one call with the identical
    //! message, in order, and the run checks the number of executed assertions
    //! against the static count of the C++ file on Linux (the `#else` branches:
    //! `\b` word boundaries and `ut.skip` for `D\d`).
    //!
    //! [`GOLDEN`] holds, for each (pattern, case_sensitive, subject), the
    //! `std::sregex_iterator` match spans (byte offsets) or the
    //! `regex_error::what ()` text produced by the C++ `RX` logic
    //! (`std::regex (pattern, ECMAScript [| icase])`) compiled with g++ 11.4.
    use super::*;
    use std::fmt::Debug;

    /// rx.t.cpp: `UnitTest ut (26)`; 26 assertions execute on Linux.
    const RX_T_ASSERTIONS: usize = 26;

    /// The C++ `UnitTest` (test/test.cpp), recording instead of printing.
    struct UnitTest {
        counter: usize,
        failures: Vec<String>,
    }

    impl UnitTest {
        fn new() -> Self {
            UnitTest { counter: 0, failures: Vec::new() }
        }

        fn ok(&mut self, expression: bool, name: impl AsRef<str>) {
            self.counter += 1;
            if !expression {
                self.failures.push(format!("not ok {} - {}", self.counter, name.as_ref()));
            }
        }

        fn is<A: PartialEq<E> + Debug, E: Debug>(&mut self, actual: A, expected: E, name: impl AsRef<str>) {
            self.counter += 1;
            if actual != expected {
                self.failures.push(format!(
                    "not ok {} - {}\n# expected: {:?}\n#      got: {:?}",
                    self.counter,
                    name.as_ref(),
                    expected,
                    actual
                ));
            }
        }

        fn skip(&mut self, _name: impl AsRef<str>) {
            self.counter += 1;
        }

        /// Fails on any failed assertion, or when the number of executed
        /// assertions differs from the C++ file's.
        fn finish(self, expected: usize) {
            assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
            assert_eq!(self.counter, expected, "assertion count differs from the C++ test");
        }
    }

    /// The `regex_error` text of a failed `RX` operation.
    fn message(e: TwError) -> String {
        match e {
            TwError::Message(m) => m,
            other => panic!("unexpected error variant {other:?}"),
        }
    }

    /// rx.t.cpp `main`, line by line (Linux branches).
    #[test]
    fn rx_t_cpp() {
        let mut ut = UnitTest::new();

        let mut text = String::from("This is a test.");
        let r1 = RX::new("i. ", true);
        ut.ok(r1.matches(&text).unwrap(), format!("{text} =~ /i. /"));

        let mut matches: Vec<String> = Vec::new();
        ut.ok(r1.matches_captures(&mut matches, &text).unwrap(), format!("{text} =~ /i. /"));
        ut.ok(matches.len() == 2, "2 match");
        ut.is(matches.first().map(String::as_str), Some("is "), "$1 == is\\s");
        ut.is(matches.get(1).map(String::as_str), Some("is "), "$1 == is\\s");

        text = String::from("abcdefghijklmnopqrstuvwxyz");

        let r3 = RX::new("t..", true);
        ut.ok(r3.matches(&text).unwrap(), "t..");

        let r4 = RX::new("T..", false);
        ut.ok(r4.matches(&text).unwrap(), "T..");

        let r5 = RX::new("T..", true);
        ut.ok(!r5.matches(&text).unwrap(), "! T..");

        text = String::from("this is a test of the regex engine.");
        //                   |...:....|....:....|....:....|....:

        let r6 = RX::with_pattern("^this");
        ut.ok(r6.matches(&text).unwrap(), "^this matches");

        let r7 = RX::with_pattern("engine\\.$");
        ut.ok(r7.matches(&text).unwrap(), "engine\\.$ matches");

        let mut results: Vec<String> = Vec::new();
        let mut start: Vec<i32> = Vec::new();
        let mut end: Vec<i32> = Vec::new();
        let r8 = RX::new("e..", true);
        ut.ok(r8.matches_captures(&mut results, &text).unwrap(), "e.. there are matches");
        ut.ok(r8.matches_offsets(&mut start, &mut end, &text).unwrap(), "e.. there are matches");
        ut.is(results.len(), 4usize, "e.. == 4 matches");
        ut.is(results.first().map(String::as_str), Some("est"), "e..[0] == 'est'");
        ut.is(start.first().copied(), Some(11), "e..[0] == 11->");
        ut.is(end.first().copied(), Some(14), "e..[0] == ->14");

        results.clear();
        let r9 = RX::new("e", true);
        ut.ok(r9.matches_captures(&mut results, &text).unwrap(), "e there are matches");
        ut.is(results.len(), 6usize, "e == 6 matches");

        start.clear();
        end.clear();
        ut.ok(r9.matches_offsets(&mut start, &mut end, &text).unwrap(), "e there are matches");
        ut.is(start.len(), 6usize, "e == 6 matches");

        // #else (Linux): \b word boundaries.
        let r10 = RX::with_pattern("\\bthe");
        text = String::from("this is the end.");
        ut.ok(r10.matches(&text).unwrap(), format!("{text} =~ /\\bthe/"));

        let r11 = RX::with_pattern("the\\b");
        ut.ok(r11.matches(&text).unwrap(), format!("{text} =~ /the\\b/"));

        let r12 = RX::with_pattern("\\bthe\\b");
        ut.ok(r12.matches(&text).unwrap(), format!("{text} =~ /\\bthe\\b/"));

        // #else (not DARWIN).
        ut.skip(" =~ /D\\d/");

        text = String::from("D0");
        let r14 = RX::with_pattern("D[[:digit:]]");
        ut.ok(r14.matches(&text).unwrap(), format!("{text} =~ /D[[:digit:]]/"));

        text = String::from("D0");
        let r15 = RX::with_pattern("D[0-9]");
        ut.ok(r15.matches(&text).unwrap(), format!("{text} =~ /D[0-9]/"));

        ut.finish(RX_T_ASSERTIONS);
    }

    /// (pattern, case_sensitive, subject, C++ spans or C++ error text).
    type Golden = (&'static str, bool, &'static str, std::result::Result<&'static [(i32, i32)], &'static str>);

    /// Output of the C++ `RX` logic under GCC 11.4 libstdc++ (see the module
    /// documentation).
    const GOLDEN: &[Golden] = &[
        ("i. ", true, "This is a test.", Ok(&[(2, 5), (5, 8)])),
        ("e..", true, "this is a test of the regex engine.", Ok(&[(11, 14), (20, 23), (23, 26), (28, 31)])),
        ("\\bthe\\b", true, "this is the end.", Ok(&[(8, 11)])),
        ("\\Bhe", true, "the hello", Ok(&[(1, 3)])),
        ("\\<the", true, "this <the end", Ok(&[(5, 9)])),
        ("the\\>", true, "the> x", Ok(&[(0, 4)])),
        ("\\<the\\>", true, "this is the end.", Ok(&[])),
        ("a.b", true, "a\x0db a\x0ab axb", Ok(&[(8, 11)])),
        ("[]a]", true, "]a", Ok(&[])),
        ("[^]", true, "a\x0ab", Ok(&[(0, 1), (1, 2), (2, 3)])),
        ("[[:alpha:]]+", true, "ab1cD", Ok(&[(0, 2), (3, 5)])),
        ("[[:ALPHA:]]+", true, "ab1", Ok(&[(0, 2)])),
        ("[[:w:]]+", true, "a_b 1", Ok(&[(0, 3), (4, 5)])),
        ("[[:d:]][[:s:]]", true, "x1 2", Ok(&[(1, 3)])),
        ("[[:digit:]]+", true, "a12b3", Ok(&[(1, 3), (4, 5)])),
        ("[[:space:]]+", true, "a \x09\x0b\x0cb\x0d\x0a", Ok(&[(1, 5), (6, 8)])),
        ("[[:upper:]]+", true, "abCDe", Ok(&[(2, 4)])),
        ("[[:lower:]]+", true, "abCDe", Ok(&[(0, 2), (4, 5)])),
        ("[[:punct:]]+", true, "!@a#$%^&*()_+-={}[]|\\:;\"'<>,.?/~`", Ok(&[(0, 2), (3, 33)])),
        ("[[:blank:]]", true, "a b\x09c\x0ad", Ok(&[(1, 2), (3, 4)])),
        ("[[:xdigit:]]+", true, "fG0aZ9", Ok(&[(0, 1), (2, 4), (5, 6)])),
        ("[[:graph:]]+", true, "a b", Ok(&[(0, 1), (2, 3)])),
        ("[[:print:]]+", true, "a b\x09", Ok(&[(0, 3)])),
        ("[[:cntrl:]]", true, "a\x7f\x01", Ok(&[(1, 2), (2, 3)])),
        ("[[:alnum:]]+", true, "a1_", Ok(&[(0, 2)])),
        ("[^[:alpha:]]+", true, "ab12cd", Ok(&[(2, 4)])),
        ("[[.a.]]", true, "a", Ok(&[(0, 1)])),
        ("[[=a=]]", true, "aAb", Ok(&[(0, 1), (1, 2)])),
        ("[[.space.]]", true, "a b", Ok(&[(1, 2)])),
        ("[[.hyphen.]-z]", true, "-az", Ok(&[(0, 1), (1, 2), (2, 3)])),
        ("[[:foo:]]", true, "a", Err("Invalid character class.")),
        ("[[.foo.]]", true, "a", Err("Invalid collate element.")),
        ("[[=ab=]]", true, "a", Err("Invalid equivalence class.")),
        ("\\d\\w\\s", true, "1a é", Ok(&[(0, 3)])),
        ("\\w+", true, "café", Ok(&[(0, 3)])),
        ("\\W", true, "é", Ok(&[(0, 1), (1, 2)])),
        ("\\S+", true, "é x", Ok(&[(0, 2), (3, 4)])),
        ("\\D", true, "1é", Ok(&[(1, 2), (2, 3)])),
        ("[^a]", true, "é", Ok(&[(0, 1), (1, 2)])),
        (".", true, "é", Ok(&[(0, 1), (1, 2)])),
        ("é", true, "café", Ok(&[(3, 5)])),
        ("[é]", true, "é", Ok(&[(0, 1), (1, 2)])),
        ("\\bé", true, "x é", Ok(&[])),
        ("\\b", true, "é", Ok(&[])),
        ("\\B", true, "ab", Ok(&[(1, 1)])),
        ("é", false, "É", Ok(&[])),
        ("[a-z]+", false, "ABC", Ok(&[(0, 3)])),
        ("[[:lower:]]+", false, "ABC", Ok(&[(0, 3)])),
        ("[[:upper:]]", false, "a", Ok(&[(0, 1)])),
        ("A", false, "a", Ok(&[(0, 1)])),
        ("[^a]", false, "A", Ok(&[])),
        ("[A-C]+", false, "abc", Ok(&[(0, 3)])),
        ("\\x41", false, "a", Ok(&[(0, 1)])),
        ("[\\x41]", false, "a", Ok(&[(0, 1)])),
        ("T..", false, "abcdefghijklmnopqrstuvwxyz", Ok(&[(19, 22)])),
        ("k", false, "KKk", Ok(&[(0, 1), (4, 5)])),
        ("[Z-a]", false, "z_A", Ok(&[(0, 1), (1, 2), (2, 3)])),
        ("[[=a=]]", false, "A", Ok(&[(0, 1)])),
        ("x*", true, "ab", Ok(&[(0, 0), (1, 1), (2, 2)])),
        ("", true, "ab", Ok(&[(0, 0), (1, 1), (2, 2)])),
        ("a*", true, "aab", Ok(&[(0, 2), (2, 2), (3, 3)])),
        ("a**", true, "a", Ok(&[(0, 1), (1, 1)])),
        ("a+*", true, "aa", Ok(&[(0, 2), (2, 2)])),
        ("a?+", true, "aa", Ok(&[(0, 2), (2, 2)])),
        ("a{1}{2}", true, "aa", Ok(&[(0, 2)])),
        ("a{2}?", true, "aaa", Ok(&[(0, 2)])),
        ("a{1,3}?", true, "aaa", Ok(&[(0, 1), (1, 2), (2, 3)])),
        ("a{2}", true, "aaa", Ok(&[(0, 2)])),
        ("a{2,}", true, "aaaaa", Ok(&[(0, 5)])),
        ("a{0}", true, "a", Ok(&[(0, 0), (1, 1)])),
        ("(?:ab)+", true, "abab", Ok(&[(0, 4)])),
        ("ab+?", true, "abb", Ok(&[(0, 2)])),
        ("a|", true, "ab", Ok(&[(0, 1), (1, 1), (2, 2)])),
        ("(|)", true, "a", Ok(&[(0, 0), (1, 1)])),
        ("()", true, "a", Ok(&[(0, 0), (1, 1)])),
        ("()*", true, "a", Ok(&[(0, 0), (1, 1)])),
        ("(\\b)*a", true, "a", Ok(&[(0, 1)])),
        ("a|b|c|d", true, "xdcba", Ok(&[(1, 2), (2, 3), (3, 4), (4, 5)])),
        ("(a|ab)(c|bcd)(d*)", true, "abcd", Ok(&[(0, 4)])),
        ("(a)|b", true, "b", Ok(&[(0, 1)])),
        ("^this", true, "this is", Ok(&[(0, 4)])),
        ("engine\\.$", true, "the regex engine.", Ok(&[(10, 17)])),
        ("$a", true, "a", Ok(&[])),
        ("a^", true, "a", Ok(&[])),
        ("^", true, "ab", Ok(&[(0, 0)])),
        ("$", true, "ab", Ok(&[(2, 2)])),
        ("\\cJ", true, "J\x0a", Ok(&[(0, 1)])),
        ("\\c1", true, "c1", Ok(&[(1, 2)])),
        ("\\0", true, "a\x00", Ok(&[(1, 2)])),
        ("\\x41\\u0042", true, "AB", Ok(&[(0, 2)])),
        ("\\u0141", true, "A", Ok(&[(0, 1)])),
        ("[\\b]", true, "\x08b", Ok(&[(0, 1)])),
        ("\\/\\q\\e\\a\\<\\>", true, "/qea<>", Ok(&[(0, 6)])),
        ("\\f\\v\\t\\n\\r", true, "\x0c\x0b\x09\x0a\x0d", Ok(&[(0, 5)])),
        ("[\\]]", true, "]", Ok(&[(0, 1)])),
        ("[a\\-z]", true, "-b", Ok(&[(0, 1)])),
        ("[[]", true, "[", Ok(&[(0, 1)])),
        ("[&&]", true, "&", Ok(&[(0, 1)])),
        ("[a~~b]", true, "~", Ok(&[(0, 1)])),
        ("[a--]", true, "-+", Err("Invalid range in bracket expression.")),
        ("[-a]", true, "-", Ok(&[(0, 1)])),
        ("[a-]", true, "-", Ok(&[(0, 1)])),
        ("[a-b-c]", true, "-c", Ok(&[(0, 1), (1, 2)])),
        ("[\\s\\d]", true, " 1x", Ok(&[(0, 1), (1, 2)])),
        ("[^\\d]", true, "1a", Ok(&[(1, 2)])),
        ("[\\D]", true, "1a", Ok(&[(1, 2)])),
        ("[\\W]", true, "a.", Ok(&[(1, 2)])),
        ("[\\x41-\\x43]", true, "ABCD", Ok(&[(0, 1), (1, 2), (2, 3)])),
        ("[.]", true, "a.", Ok(&[(1, 2)])),
        ("[$^|()*+?{}]+", true, "$^|()*+?{}", Ok(&[(0, 10)])),
        ("]}", true, "]}", Ok(&[(0, 2)])),
        ("[^]]", true, "a]", Ok(&[(0, 2)])),
        ("x[]", true, "x", Ok(&[])),
        ("[]", true, "x", Ok(&[])),
        ("[z-a]", true, "a", Err("Invalid range in bracket expression.")),
        ("[a-é]", true, "a", Err("Invalid range in bracket expression.")),
        ("[\\x00-\\xff]", true, "a", Err("Invalid range in bracket expression.")),
        ("[\\x80-\\xff]+", true, "aéb", Ok(&[(1, 3)])),
        ("[\\d-z]", true, "-", Err("Invalid start of range in bracket expression.")),
        ("[\\w-a]", true, "-", Err("Invalid start of range in bracket expression.")),
        ("[a-\\w]", true, "-", Err("Invalid end of range in bracket expression.")),
        ("[a-", true, "a", Err("Invalid end of range in bracket expression.")),
        ("[a-[:alpha:]]", true, "a", Err("Invalid end of range in bracket expression.")),
        ("[[:alpha:]-z]", true, "a", Err("Invalid start of range in bracket expression.")),
        ("[[:alpha:]", true, "a", Err("Unexpected character in bracket expression.")),
        ("[[:alpha", true, "a", Err("Unexpected end of character class.")),
        ("[[", true, "a", Err("Unexpected character class open bracket.")),
        ("[\\B]", true, "a", Err("Unexpected character in bracket expression.")),
        ("[\\1]", true, "a", Err("Unexpected character in bracket expression.")),
        ("[", true, "a", Err("Unexpected character in bracket expression.")),
        ("[^", true, "a", Err("Unexpected character in bracket expression.")),
        ("[a", true, "a", Err("Unexpected character in bracket expression.")),
        ("[\\", true, "a", Err("Unexpected end of regex when escaping.")),
        ("(", true, "a", Err("Parenthesis is not closed.")),
        (")", true, "a", Err("Mismatched '(' and ')' in regular expression")),
        ("(a", true, "a", Err("Parenthesis is not closed.")),
        ("(?:", true, "a", Err("Parenthesis is not closed.")),
        ("(?", true, "a", Err("Unexpected end of regex when in an open parenthesis.")),
        ("(?:a", true, "a", Err("Parenthesis is not closed.")),
        ("(?i)a", true, "a", Err("Invalid special open parenthesis.")),
        ("(?<n>a)", true, "a", Err("Invalid special open parenthesis.")),
        ("(*)", true, "a", Err("Parenthesis is not closed.")),
        ("*a", true, "a", Err("Mismatched '(' and ')' in regular expression")),
        ("a|*", true, "a", Err("Mismatched '(' and ')' in regular expression")),
        ("^*", true, "a", Err("Mismatched '(' and ')' in regular expression")),
        ("\\b*", true, "a", Err("Mismatched '(' and ')' in regular expression")),
        ("$+", true, "a", Err("Mismatched '(' and ')' in regular expression")),
        ("{", true, "{", Err("Mismatched '(' and ')' in regular expression")),
        ("a{", true, "a", Err("Unexpected token in brace expression.")),
        ("a{1", true, "a", Err("Unexpected end of brace expression.")),
        ("a{1,", true, "a", Err("Unexpected end of brace expression.")),
        ("a{1,2", true, "a", Err("Unexpected end of brace expression.")),
        ("a{,2}", true, "a", Err("Unexpected token in brace expression.")),
        ("a{x}", true, "a", Err("Unexpected character in brace expression.")),
        ("a{1x}", true, "a", Err("Unexpected character in brace expression.")),
        ("a{2,1}", true, "a", Err("Invalid range in brace expression.")),
        ("a{99999}", true, "a", Err("Number of NFA states exceeds limit. Please use shorter regex string, or use smaller brace expression, or make _GLIBCXX_REGEX_STATE_LIMIT larger.")),
        ("(a{1000}){100}", true, "a", Err("Number of NFA states exceeds limit. Please use shorter regex string, or use smaller brace expression, or make _GLIBCXX_REGEX_STATE_LIMIT larger.")),
        ("(?:a{300}){300}", true, "a", Ok(&[])),
        ("a{2147483648}", true, "a", Err("invalid back reference")),
        ("\\", true, "a", Err("Unexpected end of regex when escaping.")),
        ("a\\", true, "a", Err("Unexpected end of regex when escaping.")),
        ("\\c", true, "c", Err("Unexpected end of regex when reading control code.")),
        ("\\x4", true, "a", Err("Unexpected end of regex when ascii character.")),
        ("\\x4g", true, "a", Err("Unexpected end of regex when ascii character.")),
        ("\\u12", true, "a", Err("Unexpected end of regex when ascii character.")),
        ("\\x", true, "x", Err("Unexpected end of regex when ascii character.")),
        ("\\u", true, "u", Err("Unexpected end of regex when ascii character.")),
        ("\\1", true, "a", Err("Back-reference index exceeds current sub-expression count.")),
        ("(a\\1)", true, "a", Err("Back-reference referred to an opened sub-expression.")),
        ("(a)\\2", true, "a", Err("Back-reference index exceeds current sub-expression count.")),
        ("\\8", true, "8", Err("Back-reference index exceeds current sub-expression count.")),
        ("(a)\\99999999999", true, "a", Err("invalid back reference")),
        ("(a)\\1(", true, "aa", Err("Parenthesis is not closed.")),
        ("(?=a)(", true, "a", Err("Parenthesis is not closed.")),
        ("(?=a", true, "a", Err("Parenthesis is not closed.")),
        ("(?!a)*", true, "a", Err("Mismatched '(' and ')' in regular expression")),
        ("(a)\\01", true, "a\x001", Ok(&[(0, 3)])),
        ("\\00", true, "\x000", Ok(&[(0, 2)])),
        ("D[[:digit:]]", true, "D0", Ok(&[(0, 2)])),
        ("D[0-9]", true, "D0", Ok(&[(0, 2)])),
        ("D\\d", true, "D0", Ok(&[(0, 2)])),
        ("t..", true, "abcdefghijklmnopqrstuvwxyz", Ok(&[(19, 22)])),
        ("T..", true, "abcdefghijklmnopqrstuvwxyz", Ok(&[])),
        ("\\bHello\\b", true, "Hello, world. Hellos", Ok(&[(0, 5)])),
        ("(foo|bar)+baz", true, "foobarbaz xbaz", Ok(&[(0, 9)])),
        ("a.*b", true, "axxbyyb", Ok(&[(0, 7)])),
        ("a.*?b", true, "axxbyyb", Ok(&[(0, 4)])),
        ("(a+)+$", true, "aaaa", Ok(&[(0, 4)])),
        ("[a-z]+@[a-z]+\\.com", true, "x: foo@bar.com, baz@qux.com", Ok(&[(3, 14), (16, 27)])),
    ];

    #[test]
    fn golden_libstdcxx() {
        let mut failures = Vec::new();
        for &(pattern, case_sensitive, text, expected) in GOLDEN {
            let rx = RX::new(pattern, case_sensitive);
            let label = format!("/{pattern}/ cs={case_sensitive} on {text:?}");
            match expected {
                Ok(spans) => {
                    let mut start = Vec::new();
                    let mut end = Vec::new();
                    match rx.matches_offsets(&mut start, &mut end, text) {
                        Ok(any) => {
                            let got: Vec<(i32, i32)> = start.iter().copied().zip(end.iter().copied()).collect();
                            if got != spans || any == spans.is_empty() {
                                failures.push(format!("{label}: spans {got:?}, C++ {spans:?}"));
                            }
                            if rx.matches(text).unwrap() == spans.is_empty() {
                                failures.push(format!("{label}: matches () differs"));
                            }
                            let mut captured = Vec::new();
                            rx.matches_captures(&mut captured, text).unwrap();
                            let bytes = text.as_bytes();
                            let want: Vec<String> = spans
                                .iter()
                                .map(|&(s, e)| String::from_utf8_lossy(&bytes[s as usize..e as usize]).into_owned())
                                .collect();
                            if captured != want {
                                failures.push(format!("{label}: captures {captured:?}, C++ {want:?}"));
                            }
                        }
                        Err(e) => failures.push(format!("{label}: error {:?}, C++ spans {spans:?}", message(e))),
                    }
                }
                Err(what) => {
                    let errors = [
                        rx.compile().err().map(message),
                        rx.matches(text).err().map(message),
                        rx.matches_captures(&mut Vec::new(), text).err().map(message),
                        rx.matches_offsets(&mut Vec::new(), &mut Vec::new(), text).err().map(message),
                    ];
                    for got in errors {
                        if got.as_deref() != Some(what) {
                            failures.push(format!("{label}: {got:?}, C++ error {what:?}"));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// POSIX bracket classes are the C-locale (ASCII) classes.
    #[test]
    fn translation_posix_classes() {
        let m = |p: &str, t: &str| RX::with_pattern(p).matches(t).unwrap();
        assert!(m("^[[:alpha:]]+$", "abcXYZ"));
        assert!(!m("[[:alpha:]]", "é1_"));
        assert!(m("^[[:digit:]]+$", "0123456789"));
        assert!(!m("[[:digit:]]", "٣"));
        assert!(m("^[[:space:]]+$", " \t\n\x0b\x0c\r"));
        assert!(!m("[[:space:]]", "\u{a0}"));
        assert!(m("^[[:upper:]]+$", "ABC"));
        assert!(!m("[[:upper:]]", "abcÉ"));
        assert!(m("^[[:punct:]]+$", "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"));
        assert!(m("^[[:w:]]+$", "a_1"));
        assert!(m("^[[:d:]][[:s:]]$", "1 "));
        assert!(m("^[^[:alnum:]]$", "_"));
        assert_eq!(translate(b"[[:digit:]]", false).unwrap(), "[\\x30-\\x39]");
        assert_eq!(translate(b"\\d", false).unwrap(), "[\\x30-\\x39]");
        assert_eq!(translate(b"\\w", false).unwrap(), "[\\x30-\\x39\\x41-\\x5A\\x5F\\x61-\\x7A]");
        assert_eq!(translate(b"\\s", false).unwrap(), "[\\x09-\\x0D\\x20]");
        assert_eq!(translate(b".", false).unwrap(), "[\\x00-\\x09\\x0B-\\x0C\\x0E-\\xFF]");
    }

    /// `\<` and `\>` are the literal `<` and `>` (libstdc++ ECMAScript
    /// identity escapes); `\b` / `\B` are the word boundaries.
    #[test]
    fn translation_word_boundaries() {
        let m = |p: &str, t: &str| RX::with_pattern(p).matches(t).unwrap();
        assert!(!m("\\<the\\>", "this is the end."));
        assert!(m("\\<the\\>", "a <the> tag"));
        assert_eq!(translate(b"\\<\\>", false).unwrap(), "\\x3C\\x3E");
        assert!(m("\\bthe\\b", "this is the end."));
        assert!(!m("\\bthe\\b", "other theme"));
        assert!(m("\\Bthe", "other"));
        assert!(!m("\\Bthe", "the"));
        // ASCII word characters only: 'é' is not a word character.
        assert!(m("caf\\b", "café"));
        // CLI2 `desc.word:Hello` on Linux.
        assert!(m("\\bHello\\b", "Say Hello."));
        assert!(!m("\\bHello\\b", "Say Hellos."));
    }

    /// `icase` folds ASCII letters only, as the C-locale `tolower`.
    #[test]
    fn translation_case_insensitive() {
        let ci = |p: &str, t: &str| RX::new(p, false).matches(t).unwrap();
        let cs = |p: &str, t: &str| RX::new(p, true).matches(t).unwrap();
        assert!(ci("hello", "HeLLo"));
        assert!(!cs("hello", "HeLLo"));
        assert!(ci("[a-c]+$", "xABC"));
        assert!(ci("[[:lower:]]", "A"));
        assert!(ci("[[:upper:]]", "a"));
        assert!(!cs("[[:upper:]]", "a"));
        assert!(!ci("[^a]", "A"));
        assert!(!ci("é", "É"));
        assert!(!ci("k", "\u{212a}"));
        assert!(!ci("s", "\u{17f}"));
        assert_eq!(translate(b"a", true).unwrap(), "[\\x41\\x61]");
        assert_eq!(translate(b"a", false).unwrap(), "a");
        assert_eq!(translate(b"1", true).unwrap(), "1");
    }

    /// The C++ constructors, copy and the lazy compilation.
    #[test]
    fn api() {
        let d = RX::default();
        assert_eq!(d.pattern(), "");
        assert!(!d.case_sensitive());
        assert!(d.matches("anything").unwrap());
        assert!(d.matches("").unwrap());

        let r = RX::with_pattern("a.c");
        assert!(r.case_sensitive());
        assert_eq!(r, RX::new("a.c", true));
        assert_ne!(r, RX::new("a.c", false));
        assert_ne!(r, RX::new("a.d", true));
        assert!(r.matches("xabc").unwrap());
        let copy = r.clone();
        assert_eq!(copy, r);
        assert!(copy.matches("abc").unwrap());
        let mut assigned = RX::default();
        assert_ne!(assigned, r);
        assigned = r.clone();
        assert_eq!(assigned.pattern(), "a.c");
        assert!(format!("{r:?}").contains("a.c"));

        // Construction never fails; the C++ throw surfaces on use.
        let bad = RX::new("(", true);
        assert_eq!(message(bad.compile().unwrap_err()), "Parenthesis is not closed.");
        assert_eq!(message(bad.matches("(").unwrap_err()), "Parenthesis is not closed.");
        assert_eq!(message(bad.clone().compile().unwrap_err()), "Parenthesis is not closed.");
        assert!(RX::new("a", true).compile().is_ok());
    }

    /// The vectors are appended to, and the result reflects their size, as in
    /// C++ (`return !matches.empty ()`).
    #[test]
    fn vectors_accumulate() {
        let rx = RX::with_pattern("z");
        let mut matches = vec![String::from("earlier")];
        assert!(rx.matches_captures(&mut matches, "abc").unwrap());
        assert_eq!(matches, ["earlier"]);
        let mut start = vec![7];
        let mut end = vec![8];
        assert!(rx.matches_offsets(&mut start, &mut end, "abc").unwrap());
        assert_eq!((start, end), (vec![7], vec![8]));

        let rx = RX::with_pattern("b");
        let mut matches = vec![String::from("x")];
        assert!(rx.matches_captures(&mut matches, "abcb").unwrap());
        assert_eq!(matches, ["x", "b", "b"]);
        let mut start = Vec::new();
        let mut end = Vec::new();
        assert!(!RX::with_pattern("z").matches_offsets(&mut start, &mut end, "abc").unwrap());
        assert!(start.is_empty() && end.is_empty());
    }

    /// Matching is per byte, as in C++; offsets are byte offsets and a match
    /// inside a UTF-8 sequence is converted lossily for the `String` vector.
    #[test]
    fn bytes_and_utf8() {
        let rx = RX::with_pattern("f.");
        let mut start = Vec::new();
        let mut end = Vec::new();
        assert!(rx.matches_offsets(&mut start, &mut end, "fé").unwrap());
        assert_eq!((start, end), (vec![0], vec![2]));
        let mut matches = Vec::new();
        assert!(rx.matches_captures(&mut matches, "fé").unwrap());
        assert_eq!(matches, ["f\u{fffd}"]);

        let mut start = Vec::new();
        let mut end = Vec::new();
        assert!(RX::with_pattern("é").matches_offsets(&mut start, &mut end, "aé").unwrap());
        assert_eq!((start, end), (vec![1], vec![3]));
    }

    /// Deviations from C++ (see the module documentation).
    #[test]
    fn deviations() {
        // Back-references and lookahead are valid in C++.
        assert_eq!(message(RX::with_pattern("(a)\\1").compile().unwrap_err()), BACKREF_UNSUPPORTED);
        assert_eq!(message(RX::with_pattern("(?=a)a").compile().unwrap_err()), LOOKAHEAD_UNSUPPORTED);
        assert_eq!(message(RX::with_pattern("a(?!b)").compile().unwrap_err()), LOOKAHEAD_UNSUPPORTED);
        // The first unsupported construct is reported...
        assert_eq!(message(RX::with_pattern("(?=a)(a)\\1").compile().unwrap_err()), LOOKAHEAD_UNSUPPORTED);
        // ...unless C++ fails later in the pattern.
        assert_eq!(message(RX::with_pattern("(a)\\1[").compile().unwrap_err()), "Unexpected character in bracket expression.");

        // C++ `a??` on "aa": (0,0) (0,1) (1,1) (1,2) (2,2); the non-null retry
        // after an empty match finds nothing here.
        let mut start = Vec::new();
        let mut end = Vec::new();
        RX::with_pattern("a??").matches_offsets(&mut start, &mut end, "aa").unwrap();
        assert_eq!((start, end), (vec![0, 1, 2], vec![0, 1, 2]));

        // C++ `a||b` on "ab": (0,1) (1,1) (1,2) (2,2); the empty middle
        // alternative wins at 1, and the C++ non-null retry finds "b".
        let mut start = Vec::new();
        let mut end = Vec::new();
        RX::with_pattern("a||b").matches_offsets(&mut start, &mut end, "ab").unwrap();
        assert_eq!((start, end), (vec![0, 1, 2], vec![1, 1, 2]));
    }
}
