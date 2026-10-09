//! Rust translation of `src/Lexer.h` / `src/Lexer.cpp`.
//!
//! `Lexer` is a UTF8 lexical analyzer for every construct used on the
//! Taskwarrior command line, with additional recognized types for
//! disambiguation.
//!
//! # Statics
//! The C++ class statics `Lexer::dateFormat`, `Lexer::minimumMatchLength` and
//! `Lexer::attributes` follow the `shared::datetime` pattern (milestone Design
//! Decision 3): one `thread_local!` `Cell`/`RefCell` per static, with the C++
//! initializers as defaults, accessed only through associated functions:
//! * `Lexer::dateFormat` -> [`Lexer::date_format`] / [`Lexer::set_date_format`]
//! * `Lexer::minimumMatchLength` -> [`Lexer::minimum_match_length`] /
//!   [`Lexer::set_minimum_match_length`]
//! * `Lexer::attributes` -> [`Lexer::attributes`] (a copy),
//!   [`Lexer::with_attributes`] / [`Lexer::with_attributes_mut`] (borrow),
//!   [`Lexer::set_attributes`], and [`Lexer::insert_attribute`] for the
//!   Context idiom `Lexer::attributes[name] = type`.
//!
//! # API mapping
//! * `Lexer::Type` -> [`Type`] (`lexer::Type`).
//! * Overloads become distinct names: `hexToInt` -> [`Lexer::hex_to_int`] /
//!   [`Lexer::hex_to_int2`] / [`Lexer::hex_to_int4`]; `commonLength` ->
//!   [`Lexer::common_length`] / [`Lexer::common_length_at`]; `readWord` ->
//!   [`Lexer::read_word_quoted`] (with quotes) / [`Lexer::read_word`]
//!   (unquoted); static `isDOM (string)` -> [`Lexer::is_dom_str`] (the stream
//!   classifier is [`Lexer::is_dom`]); `isOneOf` -> [`Lexer::is_one_of`]
//!   (vector) / [`Lexer::is_one_of_map`] (map keys).
//! * Default arguments: `dequote (s)` -> [`Lexer::dequote`], `dequote (s, q)`
//!   -> [`Lexer::dequote_with`]; `trimLeft`/`trimRight`/`trim (in)` ->
//!   [`Lexer::trim_left`] / [`Lexer::trim_right`] / [`Lexer::trim`], and with
//!   an explicit set `*_with`.
//! * Out-parameters stay `&mut` out-params returning `bool`, written exactly
//!   where the C++ writes them (so partial writes on failure are preserved).
//! * `int` character arguments are `i32`. Characters read from the text are
//!   bytes widened as a signed C++ `char` (so a UTF-8 continuation byte such
//!   as `0x85` never classifies as U+0085 whitespace, matching C++), and
//!   `std::string::operator[]` at or past the end yields `0`.
//! * `_cursor` / `_eos` are byte offsets; unsigned C++ arithmetic such as
//!   `_eos - marker` uses `wrapping_sub` like `size_t`.
//! * `isprint`, `tolower`, `toupper` are the C locale (ASCII) versions.
//!
//! # Deliberate deviations
//! * `std::string::substr` past the end throws in C++; here it clamps (only
//!   reachable after an unterminated trailing `\` escape advanced a cursor
//!   past the end, which C++ handles with undefined behaviour).
//! * Words built from raw bytes (`readWord` escape of a non-ASCII lead byte,
//!   e.g. `\é`) can be invalid UTF-8 in C++; the bytes are kept and converted
//!   with U+FFFD replacement, per the libshared port conventions.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use crate::shared::datetime::Datetime;
use crate::shared::duration::Duration;
use crate::shared::unicode::{unicode_hex_digit, unicode_latin_alpha, unicode_latin_digit, unicode_whitespace};
use crate::shared::utf8::{utf8_character, utf8_next_char};

/// C++ file-static `uuid_pattern`.
const UUID_PATTERN: &str = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx";
/// C++ file-static `uuid_min_length`.
const UUID_MIN_LENGTH: usize = 8;

thread_local! {
    /// C++ `Lexer::dateFormat`.
    static DATE_FORMAT: RefCell<String> = const { RefCell::new(String::new()) };
    /// C++ `Lexer::minimumMatchLength`.
    static MINIMUM_MATCH_LENGTH: Cell<usize> = const { Cell::new(3) };
    /// C++ `Lexer::attributes`.
    static ATTRIBUTES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
}

/// C++ `enum class Lexer::Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[allow(non_camel_case_types)]
pub enum Type {
    uuid,
    number,
    hex,
    string,
    url,
    pair,
    set,
    separator,
    tag,
    path,
    substitution,
    pattern,
    op,
    dom,
    identifier,
    word,
    date,
    duration,
}

/// C++ `class Lexer`.
#[derive(Debug, Clone)]
pub struct Lexer {
    _text: String,
    _cursor: usize,
    _eos: usize,
}

////////////////////////////////////////////////////////////////////////////////
// Byte helpers reproducing C++ `std::string` / `char` semantics.

/// C++ `text[i]` widened to `int`: the byte as a signed `char`, or `0` at or
/// past the end (`operator[] (size ())` is the terminating NUL).
fn ch(text: &[u8], i: usize) -> i32 {
    text.get(i).map_or(0, |&b| b as i8 as i32)
}

/// Bytes to `String`, replacing invalid UTF-8 with U+FFFD.
fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// C++ `text.substr (pos, len)` on bytes (clamped; see module docs).
fn substr(text: &str, pos: usize, len: usize) -> String {
    let bytes = text.as_bytes();
    let pos = pos.min(bytes.len());
    let end = pos.saturating_add(len).min(bytes.len());
    bytes_to_string(&bytes[pos..end])
}

/// C++ `text.substr (pos)`.
fn substr_from(text: &str, pos: usize) -> String {
    substr(text, pos, usize::MAX)
}

/// C++ `set.find (c) != std::string::npos` for a `char` widened to `int`.
fn find_char(set: &str, c: i32) -> bool {
    set.as_bytes().contains(&(c as u8))
}

/// C++ `unicodeWhitespace (int)` (the `int` converts to `unsigned int`).
fn ws(c: i32) -> bool {
    unicode_whitespace(c as u32)
}

/// C++ `isprint` in the C locale.
fn isprint(c: i32) -> bool {
    (0x20..=0x7E).contains(&c)
}

/// The escape `switch` shared verbatim by both C++ `readWord` overloads:
/// maps the character after a `\` to the byte appended to the word.
fn escaped_byte(c: i32) -> u8 {
    match c as u8 {
        b'"' => 0x22,
        b'\'' => 0x27,
        b'\\' => 0x5C,
        b'b' => 0x08,
        b'f' => 0x0C,
        b'n' => 0x0A,
        b'r' => 0x0D,
        b't' => 0x09,
        b'v' => 0x0B,

        // This pass-through default case means that anything can be escaped
        // harmlessly. In particular 'quote' is included, if it not one of the
        // above characters.
        other => other,
    }
}

/// The `U+XXXX` / `\uXXXX` test shared verbatim by both `readWord` overloads.
fn is_unicode_escape(text: &[u8], cursor: usize, eos: usize) -> bool {
    eos.wrapping_sub(cursor) >= 6
        && ((ch(text, cursor) == b'U' as i32 && ch(text, cursor + 1) == b'+' as i32)
            || (ch(text, cursor) == b'\\' as i32 && ch(text, cursor + 1) == b'u' as i32))
        && unicode_hex_digit(ch(text, cursor + 2) as u32)
        && unicode_hex_digit(ch(text, cursor + 3) as u32)
        && unicode_hex_digit(ch(text, cursor + 4) as u32)
        && unicode_hex_digit(ch(text, cursor + 5) as u32)
}

/// `a <= b` where `b == npos` (None) always holds.
fn le_npos(a: usize, b: Option<usize>) -> bool {
    b.is_none_or(|b| a <= b)
}

impl Lexer {
    ////////////////////////////////////////////////////////////////////////////
    // Statics.

    /// C++ `Lexer::dateFormat` (read).
    pub fn date_format() -> String {
        DATE_FORMAT.with(|f| f.borrow().clone())
    }

    /// C++ `Lexer::dateFormat = value`.
    pub fn set_date_format(value: &str) {
        DATE_FORMAT.with(|f| *f.borrow_mut() = value.to_string());
    }

    /// C++ `Lexer::minimumMatchLength` (read).
    pub fn minimum_match_length() -> usize {
        MINIMUM_MATCH_LENGTH.with(Cell::get)
    }

    /// C++ `Lexer::minimumMatchLength = value`.
    pub fn set_minimum_match_length(value: usize) {
        MINIMUM_MATCH_LENGTH.with(|c| c.set(value));
    }

    /// A copy of C++ `Lexer::attributes`.
    pub fn attributes() -> BTreeMap<String, String> {
        ATTRIBUTES.with(|a| a.borrow().clone())
    }

    /// Borrows C++ `Lexer::attributes` without copying.
    pub fn with_attributes<R>(f: impl FnOnce(&BTreeMap<String, String>) -> R) -> R {
        ATTRIBUTES.with(|a| f(&a.borrow()))
    }

    /// Mutably borrows C++ `Lexer::attributes`.
    pub fn with_attributes_mut<R>(f: impl FnOnce(&mut BTreeMap<String, String>) -> R) -> R {
        ATTRIBUTES.with(|a| f(&mut a.borrow_mut()))
    }

    /// C++ `Lexer::attributes = value`.
    pub fn set_attributes(value: BTreeMap<String, String>) {
        ATTRIBUTES.with(|a| *a.borrow_mut() = value);
    }

    /// C++ `Lexer::attributes[name] = type`.
    pub fn insert_attribute(name: &str, type_: &str) {
        ATTRIBUTES.with(|a| a.borrow_mut().insert(name.to_string(), type_.to_string()));
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer (const std::string&)`.
    pub fn new(text: &str) -> Self {
        Lexer { _text: text.to_string(), _cursor: 0, _eos: text.len() }
    }

    /// C++ `_text[i]`.
    fn at(&self, i: usize) -> i32 {
        ch(self._text.as_bytes(), i)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// When a Lexer object is constructed with a string, this method walks
    /// through the stream of low-level tokens.
    pub fn token(&mut self, token: &mut String, type_: &mut Type) -> bool {
        // Eat white space.
        while ws(self.at(self._cursor)) {
            utf8_next_char(&self._text, &mut self._cursor);
        }

        // Terminate at EOS.
        if self.is_eos() {
            return false;
        }

        // The sequence is specific, and must follow these rules:
        //   - date < duration < uuid < identifier
        //   - dom < uuid
        //   - uuid < hex < number
        //   - url < pair < identifier
        //   - hex < number
        //   - separator < tag < operator
        //   - path < substitution < pattern
        //   - set < number
        //   - word last
        if self.is_string(token, type_, "'\"")
            || self.is_date(token, type_)
            || self.is_duration(token, type_)
            || self.is_url(token, type_)
            || self.is_pair(token, type_)
            || self.is_uuid(token, type_, true)
            || self.is_set(token, type_)
            || self.is_dom(token, type_)
            || self.is_hex_number(token, type_)
            || self.is_number(token, type_)
            || self.is_separator(token, type_)
            || self.is_tag(token, type_)
            || self.is_path(token, type_)
            || self.is_substitution(token, type_)
            || self.is_pattern(token, type_)
            || self.is_operator(token, type_)
            || self.is_identifier(token, type_)
            || self.is_word(token, type_)
        {
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// This static method tokenizes the input, but discards the type
    /// information.
    pub fn split(text: &str) -> Vec<String> {
        let mut all = Vec::new();
        let mut token = String::new();
        let mut ignored = Type::word;
        let mut l = Lexer::new(text);
        while l.token(&mut token, &mut ignored) {
            all.push(token.clone());
        }

        all
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::typeName`. No L10N - these are for internal purposes.
    pub fn type_name(type_: &Type) -> String {
        match type_ {
            Type::uuid => "uuid",
            Type::number => "number",
            Type::hex => "hex",
            Type::string => "string",
            Type::url => "url",
            Type::pair => "pair",
            Type::set => "set",
            Type::separator => "separator",
            Type::tag => "tag",
            Type::path => "path",
            Type::substitution => "substitution",
            Type::pattern => "pattern",
            Type::op => "op",
            Type::dom => "dom",
            Type::identifier => "identifier",
            Type::word => "word",
            Type::date => "date",
            Type::duration => "duration",
        }
        .to_string()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isIdentifierStart`.
    pub fn is_identifier_start(c: i32) -> bool {
        c != 0 // Include null character check.
            && !ws(c)
            && !unicode_latin_digit(c as u32)
            && !Self::is_single_char_operator(c)
            && !Self::is_punctuation(c)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isIdentifierNext`.
    pub fn is_identifier_next(c: i32) -> bool {
        c != 0 // Include null character check.
            && c != b':' as i32 // Used in isPair.
            && c != b'=' as i32 // Used in isPair.
            && !ws(c)
            && !Self::is_single_char_operator(c)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isSingleCharOperator`.
    pub fn is_single_char_operator(c: i32) -> bool {
        c == b'+' as i32 || // Addition
        c == b'-' as i32 || // Subtraction or unary minus = ambiguous
        c == b'*' as i32 || // Multiplication
        c == b'/' as i32 || // Division
        c == b'(' as i32 || // Precedence open parenthesis
        c == b')' as i32 || // Precedence close parenthesis
        c == b'<' as i32 || // Less than
        c == b'>' as i32 || // Greater than
        c == b'^' as i32 || // Exponent
        c == b'!' as i32 || // Unary not
        c == b'%' as i32 || // Modulus
        c == b'=' as i32 || // Partial match
        c == b'~' as i32 // Pattern match
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isDoubleCharOperator`.
    pub fn is_double_char_operator(c0: i32, c1: i32, c2: i32) -> bool {
        let is = |c: i32, x: u8| c == x as i32;
        (is(c0, b'=') && is(c1, b'='))
            || (is(c0, b'!') && is(c1, b'='))
            || (is(c0, b'<') && is(c1, b'='))
            || (is(c0, b'>') && is(c1, b'='))
            || (is(c0, b'o') && is(c1, b'r') && Self::is_boundary(c1, c2))
            || (is(c0, b'|') && is(c1, b'|'))
            || (is(c0, b'&') && is(c1, b'&'))
            || (is(c0, b'!') && is(c1, b'~'))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isTripleCharOperator`.
    pub fn is_triple_char_operator(c0: i32, c1: i32, c2: i32, c3: i32) -> bool {
        let is = |c: i32, x: u8| c == x as i32;
        (is(c0, b'a') && is(c1, b'n') && is(c2, b'd') && Self::is_boundary(c2, c3))
            || (is(c0, b'x') && is(c1, b'o') && is(c2, b'r') && Self::is_boundary(c2, c3))
            || (is(c0, b'!') && is(c1, b'=') && is(c2, b'='))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isBoundary`.
    pub fn is_boundary(left: i32, right: i32) -> bool {
        // EOS
        if right == 0 {
            return true;
        }

        // XOR
        if unicode_latin_alpha(left as u32) != unicode_latin_alpha(right as u32) {
            return true;
        }
        if unicode_latin_digit(left as u32) != unicode_latin_digit(right as u32) {
            return true;
        }
        if ws(left) != ws(right) {
            return true;
        }

        // OR
        if Self::is_punctuation(left) || Self::is_punctuation(right) {
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isHardBoundary`.
    pub fn is_hard_boundary(left: i32, right: i32) -> bool {
        // EOS
        if right == 0 {
            return true;
        }

        // FILTER operators that don't need to be surrounded by whitespace.
        if left == b'(' as i32 || left == b')' as i32 || right == b'(' as i32 || right == b')' as i32 {
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isPunctuation`.
    pub fn is_punctuation(c: i32) -> bool {
        isprint(c)
            && c != b' ' as i32
            && c != b'@' as i32
            && c != b'#' as i32
            && c != b'$' as i32
            && c != b'_' as i32
            && !unicode_latin_digit(c as u32)
            && !unicode_latin_alpha(c as u32)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::dequote (input)` with the default quotes `'"`.
    pub fn dequote(input: &mut String) {
        Self::dequote_with(input, "'\"");
    }

    /// C++ `Lexer::dequote (input, quotes)`.
    ///
    /// Assumes that quotes is a string containing a non-trivial set of quote
    /// characters.
    pub fn dequote_with(input: &mut String, quotes: &str) {
        let bytes = input.as_bytes();
        let quote = ch(bytes, 0);
        if find_char(quotes, quote) {
            let len = bytes.len();
            if quote == ch(bytes, len.wrapping_sub(1)) {
                *input = substr(input, 1, len.wrapping_sub(2));
            }
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Detects characters in an input string that indicate quotes were
    /// required, or escapes, to get them past the shell.
    pub fn was_quoted(input: &str) -> bool {
        if input.bytes().any(|b| b" \t()<>&~".contains(&b)) {
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isEOS`.
    pub fn is_eos(&self) -> bool {
        self._cursor >= self._eos
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Converts '0'     -> 0
    ///          '9'     -> 9
    ///          'a'/'A' -> 10
    ///          'f'/'F' -> 15
    pub fn hex_to_int(c: i32) -> i32 {
        if (b'0' as i32..=b'9' as i32).contains(&c) {
            c - b'0' as i32
        } else if (b'a' as i32..=b'f' as i32).contains(&c) {
            c - b'a' as i32 + 10
        } else {
            c - b'A' as i32 + 10
        }
    }

    /// C++ `Lexer::hexToInt (int, int)`.
    pub fn hex_to_int2(c0: i32, c1: i32) -> i32 {
        (Self::hex_to_int(c0) << 4) + Self::hex_to_int(c1)
    }

    /// C++ `Lexer::hexToInt (int, int, int, int)`.
    pub fn hex_to_int4(c0: i32, c1: i32, c2: i32, c3: i32) -> i32 {
        (Self::hex_to_int(c0) << 12) + (Self::hex_to_int(c1) << 8) + (Self::hex_to_int(c2) << 4) + Self::hex_to_int(c3)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Compares two strings, and returns the number bytes in common.
    ///
    /// ```text
    /// left:   wonderful
    /// right:  wonderbread
    /// returns:     ^ 6
    /// ```
    pub fn common_length(left: &str, right: &str) -> usize {
        Self::common_length_at(left, 0, right, 0)
    }

    /// Compares two strings with offsets, and returns the number bytes in
    /// common.
    ///
    /// ```text
    /// left:   wonderful
    /// l:      ^
    /// right:  prowonderbread
    /// r:         ^
    /// returns:        ^ 6
    /// ```
    pub fn common_length_at(left: &str, mut l: usize, right: &str, mut r: usize) -> usize {
        let (lb, rb) = (left.as_bytes(), right.as_bytes());
        while ch(lb, l) == ch(rb, r) && utf8_next_char(left, &mut l) != 0 && utf8_next_char(right, &mut r) != 0 {}

        l
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::commify`.
    pub fn commify(data: &str) -> String {
        let data = data.as_bytes();
        let digit = |i: i32| unicode_latin_digit(ch(data, i as usize) as u32);

        // First scan for decimal point and end of digits.
        let mut decimal_point: i32 = -1;
        let mut end: i32 = -1;

        for i in 0..data.len() as i32 {
            if digit(i) {
                end = i;
            }

            if data[i as usize] == b'.' {
                decimal_point = i;
            }
        }

        let mut result: Vec<u8> = Vec::new();
        let mut i: i32 = data.len() as i32 - 1;
        if decimal_point != -1 {
            // In reverse order, transfer all digits up to, and including the
            // decimal point.
            while i >= decimal_point {
                result.push(data[i as usize]);
                i -= 1;
            }
        } else {
            // In reverse order, transfer all digits up to, but not including
            // the last digit.
            while i > end {
                result.push(data[i as usize]);
                i -= 1;
            }
        }

        // (Both C++ branches continue with this identical loop.)
        let mut consecutive_digits = 0;
        while i >= 0 {
            if digit(i) {
                result.push(data[i as usize]);

                consecutive_digits += 1;
                if consecutive_digits == 3 && i != 0 && digit(i - 1) {
                    result.push(b',');
                    consecutive_digits = 0;
                }
            } else {
                result.push(data[i as usize]);
            }
            i -= 1;
        }

        // reverse result into data.
        result.reverse();
        bytes_to_string(&result)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::lowerCase` (`tolower` per byte, C locale).
    pub fn lower_case(input: &str) -> String {
        input.to_ascii_lowercase()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::ucFirst` (`toupper` on the first byte, C locale).
    pub fn uc_first(input: &str) -> String {
        let mut output = input.to_string();

        if let Some(first) = output.get_mut(0..1) {
            first.make_ascii_uppercase();
        }

        output
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::trimLeft (in)` with the default `" "`.
    pub fn trim_left(input: &str) -> String {
        Self::trim_left_with(input, " ")
    }

    /// C++ `Lexer::trimLeft (in, t)`.
    pub fn trim_left_with(input: &str, t: &str) -> String {
        let t = t.as_bytes();
        // find_first_not_of: npos (> 0) erases everything.
        match input.bytes().position(|b| !t.contains(&b)) {
            Some(0) => input.to_string(),
            Some(ws) => substr_from(input, ws),
            None => String::new(),
        }
    }

    /// C++ `Lexer::trimRight (in)` with the default `" "`.
    pub fn trim_right(input: &str) -> String {
        Self::trim_right_with(input, " ")
    }

    /// C++ `Lexer::trimRight (in, t)`.
    pub fn trim_right_with(input: &str, t: &str) -> String {
        let t = t.as_bytes();
        // find_last_not_of: npos + 1 == 0 erases everything.
        match input.bytes().rposition(|b| !t.contains(&b)) {
            Some(last) => substr(input, 0, last + 1),
            None => String::new(),
        }
    }

    /// C++ `Lexer::trim (in)` with the default `" "`.
    pub fn trim(input: &str) -> String {
        Self::trim_with(input, " ")
    }

    /// C++ `Lexer::trim (in, t)`.
    pub fn trim_with(input: &str, t: &str) -> String {
        Self::trim_left_with(&Self::trim_right_with(input, t), t)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::string
    ///   '|"
    ///   [ U+XXXX | \uXXXX | \" | \' | \\ | \/ | \b | \f | \n | \r | \t | . ]
    ///   '|"
    pub fn is_string(&mut self, token: &mut String, type_: &mut Type, quotes: &str) -> bool {
        let mut marker = self._cursor;
        if Self::read_word_quoted(&self._text, quotes, &mut marker, token) {
            *type_ = Type::string;
            self._cursor = marker;
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::date
    ///   <Datetime>
    pub fn is_date(&mut self, token: &mut String, type_: &mut Type) -> bool {
        // Try an ISO date parse.
        let mut iso_i: usize = 0;
        let mut iso = Datetime::new();
        if iso.parse(&substr_from(&self._text, self._cursor), &mut iso_i, &Self::date_format()) {
            *type_ = Type::date;
            *token = substr(&self._text, self._cursor, iso_i);
            self._cursor += iso_i;
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::duration
    ///   <Duration>
    pub fn is_duration(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;

        let mut extracted_token = String::new();
        let mut extracted_type = Type::word;
        if self.is_operator(&mut extracted_token, &mut extracted_type) {
            self._cursor = marker;
            return false;
        }

        marker = 0;
        let mut iso = Duration::new();
        if iso.parse(&substr_from(&self._text, self._cursor), &mut marker) {
            *type_ = Type::duration;
            *token = substr(&self._text, self._cursor, marker);
            self._cursor += marker;
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::uuid
    /// ```text
    ///   XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX
    ///   XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXX
    ///   XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXX
    ///   XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXX
    ///   ...
    ///   XXXXXXXX-XX
    ///   XXXXXXXX-X
    ///   XXXXXXXX-
    ///   XXXXXXXX
    /// ```
    ///   Followed only by EOS, whitespace, or single character operator.
    pub fn is_uuid(&mut self, token: &mut String, type_: &mut Type, end_boundary: bool) -> bool {
        let marker = self._cursor;
        let pattern = UUID_PATTERN.as_bytes();

        // Greedy.
        let mut i: usize = 0;
        while i < 36 && marker + i < self._eos {
            if pattern[i] == b'x' {
                if !unicode_hex_digit(self.at(marker + i) as u32) {
                    break;
                }
            } else if pattern[i] as i32 != self.at(marker + i) {
                break;
            }
            i += 1;
        }

        let next = self.at(marker + i);
        if i >= UUID_MIN_LENGTH && (!end_boundary || next == 0 || ws(next) || Self::is_single_char_operator(next)) {
            *token = substr(&self._text, self._cursor, i);
            *type_ = Type::uuid;
            self._cursor += i;
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::hex
    ///   0xX+
    pub fn is_hex_number(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;

        if self._eos.wrapping_sub(marker) >= 3 && self.at(marker) == b'0' as i32 && self.at(marker + 1) == b'x' as i32 {
            marker += 2;

            while unicode_hex_digit(self.at(marker) as u32) {
                marker += 1;
            }

            if marker - self._cursor > 2 {
                *token = substr(&self._text, self._cursor, marker - self._cursor);
                *type_ = Type::hex;
                self._cursor = marker;
                return true;
            }
        }

        false
    }

    /// `while (unicodeLatinDigit (_text[marker])) utf8_next_char (_text, marker);`
    fn skip_digits(&self, marker: &mut usize) {
        while unicode_latin_digit(self.at(*marker) as u32) {
            utf8_next_char(&self._text, marker);
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::number
    ///   0
    ///   [1-9]\d*
    ///   [ . \d+ ]
    ///   [ e|E [ +|- ] \d+ [ . \d+ ] ]
    ///   not followed by non-operator.
    pub fn is_number(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;

        let leading_zero = self.at(marker) == b'0' as i32;

        if unicode_latin_digit(self.at(marker) as u32) {
            marker += 1;

            // Two (or more) digit number with a leading zero are not allowed
            if leading_zero && unicode_latin_digit(self.at(marker) as u32) {
                return false;
            }

            self.skip_digits(&mut marker);

            if self.at(marker) == b'.' as i32 {
                marker += 1;
                if unicode_latin_digit(self.at(marker) as u32) {
                    marker += 1;
                    self.skip_digits(&mut marker);
                }
            }

            if self.at(marker) == b'e' as i32 || self.at(marker) == b'E' as i32 {
                marker += 1;

                if self.at(marker) == b'+' as i32 || self.at(marker) == b'-' as i32 {
                    marker += 1;
                }

                if unicode_latin_digit(self.at(marker) as u32) {
                    marker += 1;
                    self.skip_digits(&mut marker);

                    if self.at(marker) == b'.' as i32 {
                        marker += 1;
                        if unicode_latin_digit(self.at(marker) as u32) {
                            marker += 1;
                            self.skip_digits(&mut marker);
                        }
                    }
                }
            }

            // Lookahead: !<unicodeWhitespace> | !<isSingleCharOperator>
            // If there is an immediately consecutive character, that is not an
            // operator, fail.
            if self._eos > marker && !ws(self.at(marker)) && !Self::is_single_char_operator(self.at(marker)) {
                return false;
            }

            *token = substr(&self._text, self._cursor, marker - self._cursor);
            *type_ = Type::number;
            self._cursor = marker;
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::number
    ///   0
    ///   [1-9]\d*
    ///   Integers do not start with a leading 0, unless they are zero.
    pub fn is_integer(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;

        let leading_zero = self.at(marker) == b'0' as i32;

        if unicode_latin_digit(self.at(marker) as u32) {
            marker += 1;
            self.skip_digits(&mut marker);

            // Leading zero is only allowed in the case of number 0
            if leading_zero && marker - self._cursor > 1 {
                return false;
            }

            *token = substr(&self._text, self._cursor, marker - self._cursor);
            *type_ = Type::number;
            self._cursor = marker;
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::separator
    ///   --
    pub fn is_separator(&mut self, token: &mut String, type_: &mut Type) -> bool {
        if self._eos.wrapping_sub(self._cursor) >= 2
            && self.at(self._cursor) == b'-' as i32
            && self.at(self._cursor + 1) == b'-' as i32
        {
            self._cursor += 2;
            *type_ = Type::separator;
            *token = "--".to_string();
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::url
    ///   http [s] :// ...
    pub fn is_url(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;
        let is = |c: i32, lower: u8, upper: u8| c == lower as i32 || c == upper as i32;

        if self._eos.wrapping_sub(self._cursor) > 9 // length 'https://*'
            && is(self.at(marker), b'h', b'H')
            && is(self.at(marker + 1), b't', b'T')
            && is(self.at(marker + 2), b't', b'T')
            && is(self.at(marker + 3), b'p', b'P')
        {
            marker += 4;
            if is(self.at(marker), b's', b'S') {
                marker += 1;
            }

            if self.at(marker) == b':' as i32 && self.at(marker + 1) == b'/' as i32 && self.at(marker + 2) == b'/' as i32 {
                marker += 3;

                while marker < self._eos && !ws(self.at(marker)) {
                    utf8_next_char(&self._text, &mut marker);
                }

                *token = substr(&self._text, self._cursor, marker - self._cursor);
                *type_ = Type::url;
                self._cursor = marker;
                return true;
            }
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::pair
    ///   <identifier> <separator> [ <string> | <word> ]
    ///   separator '::' | ':=' | ':' | '='
    pub fn is_pair(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let marker = self._cursor;

        let mut ignored_token = String::new();
        let mut ignored_type = Type::word;
        if self.is_identifier(&mut ignored_token, &mut ignored_type) {
            // Look for a valid separator.
            let separator = substr(&self._text, self._cursor, 2);
            // separator[0] ('\0' when empty).
            let separator0 = ch(separator.as_bytes(), 0);
            if separator == "::" || separator == ":=" {
                self._cursor += 2;
            } else if separator0 == b':' as i32 || separator0 == b'=' as i32 {
                self._cursor += 1;
            } else {
                self._cursor = marker;
                return false;
            }

            // String, word or nothing are all valid.
            if Self::read_word_quoted(&self._text, "'\"", &mut self._cursor, &mut ignored_token)
                || Self::read_word(&self._text, &mut self._cursor, &mut ignored_token)
                || self.is_eos()
                || ws(self.at(self._cursor))
            {
                *token = substr(&self._text, marker, self._cursor - marker);
                *type_ = Type::pair;
                return true;
            }
        }

        self._cursor = marker;
        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::set
    /// ```text
    ///   a single number:      1
    ///   a list of numbers:    1,3,5
    ///   a range:              5-10
    ///   or a combination:     1,3,5-10
    ///
    ///   <id> [ - <id> ] [ , <id> [ - <id> ] ] ...
    /// ```
    pub fn is_set(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let marker = self._cursor;
        let mut count = 0;
        let mut dummy_token = String::new();
        let mut dummy_type = Type::word;

        loop {
            if self.is_integer(&mut dummy_token, &mut dummy_type) {
                count += 1;
                if self.is_literal("-", false, false) {
                    if self.is_integer(&mut dummy_token, &mut dummy_type) {
                        count += 1;
                    } else {
                        self._cursor = marker;
                        return false;
                    }
                }
            } else {
                self._cursor = marker;
                return false;
            }

            if !self.is_literal(",", false, false) {
                break;
            }
        }

        // Success is multiple numbers, matching the pattern.
        if count > 1
            && (self.is_eos()
                || ws(self.at(self._cursor))
                || Self::is_hard_boundary(self.at(self._cursor), self.at(self._cursor + 1)))
        {
            *token = substr(&self._text, marker, self._cursor - marker);
            *type_ = Type::set;
            return true;
        }

        self._cursor = marker;
        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::tag
    ///   ^ | '(' | ')' | <unicodeWhitespace>
    ///     [ +|- ] <isIdentifierStart> [ <word> ]
    pub fn is_tag(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;

        // Lookbehind: Assert ^ or preceded by whitespace, (, or ).
        if marker > 0
            && !ws(self.at(marker - 1))
            && self.at(marker - 1) != b'(' as i32
            && self.at(marker - 1) != b')' as i32
        {
            return false;
        }

        if self.at(marker) == b'+' as i32 || self.at(marker) == b'-' as i32 {
            marker += 1;

            if Self::is_identifier_start(self.at(marker)) && Self::read_word(&self._text, &mut marker, token) {
                *token = substr(&self._text, self._cursor, marker.wrapping_sub(self._cursor));
                *type_ = Type::tag;
                self._cursor = marker;
                return true;
            }
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::path
    ///   ( / <non-slash, non-whitespace> )+
    pub fn is_path(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;
        let mut slash_count = 0;
        let slash = b'/' as i32;

        loop {
            if self.at(marker) == slash {
                marker += 1;
                slash_count += 1;
            } else {
                break;
            }

            if self.at(marker) != 0 && !ws(self.at(marker)) && self.at(marker) != slash {
                utf8_next_char(&self._text, &mut marker);
                while self.at(marker) != 0 && !ws(self.at(marker)) && self.at(marker) != slash {
                    utf8_next_char(&self._text, &mut marker);
                }
            } else {
                break;
            }
        }

        if marker > self._cursor && slash_count > 3 {
            *type_ = Type::path;
            *token = substr(&self._text, self._cursor, marker - self._cursor);
            self._cursor = marker;
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::substitution
    ///   / <unquoted-string> / <unquoted-string> / [g]  <EOS> | <unicodeWhitespace>
    pub fn is_substitution(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let marker = self._cursor;

        let mut word = String::new();
        if Self::read_word_quoted(&self._text, "/", &mut self._cursor, &mut word) {
            self._cursor -= 1; // Step backwards over the '/'.

            if Self::read_word_quoted(&self._text, "/", &mut self._cursor, &mut word) {
                if self.at(self._cursor) == b'g' as i32 {
                    self._cursor += 1;
                }

                // Lookahread: <EOS> | <unicodeWhitespace>
                if self.at(self._cursor) == 0 || ws(self.at(self._cursor)) {
                    *token = substr(&self._text, marker, self._cursor - marker);
                    *type_ = Type::substitution;
                    return true;
                }
            }
        }

        self._cursor = marker;
        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::pattern
    ///   / <unquoted-string> /  <EOS> | <unicodeWhitespace>
    pub fn is_pattern(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let marker = self._cursor;

        let mut word = String::new();
        if Self::read_word_quoted(&self._text, "/", &mut self._cursor, &mut word)
            && (self.is_eos() || ws(self.at(self._cursor)))
        {
            *token = substr(&self._text, marker, self._cursor - marker);
            *type_ = Type::pattern;
            return true;
        }

        self._cursor = marker;
        false
    }

    /// `_eos - marker >= literal.length () && _text.substr (marker, n) == literal`.
    fn has_literal_at(&self, marker: usize, literal: &str) -> bool {
        self._eos.wrapping_sub(marker) >= literal.len() && substr(&self._text, marker, literal.len()) == literal
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::op
    ///   _hastag_ | _notag | _neg_ | _pos_ |
    ///   <isTripleCharOperator> |
    ///   <isDoubleCharOperator> |
    ///   <isSingleCharOperator> |
    pub fn is_operator(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;

        for literal in ["_hastag_", "_notag_", "_neg_", "_pos_"] {
            if self.has_literal_at(marker, literal) {
                marker += literal.len();
                *type_ = Type::op;
                *token = substr(&self._text, self._cursor, marker - self._cursor);
                self._cursor = marker;
                return true;
            }
        }

        if self._eos.wrapping_sub(marker) >= 3
            && Self::is_triple_char_operator(self.at(marker), self.at(marker + 1), self.at(marker + 2), self.at(marker + 3))
        {
            marker += 3;
            *type_ = Type::op;
            *token = substr(&self._text, self._cursor, marker - self._cursor);
            self._cursor = marker;
            true
        } else if self._eos.wrapping_sub(marker) >= 2
            && Self::is_double_char_operator(self.at(marker), self.at(marker + 1), self.at(marker + 2))
        {
            marker += 2;
            *type_ = Type::op;
            *token = substr(&self._text, self._cursor, marker - self._cursor);
            self._cursor = marker;
            true
        } else if Self::is_single_char_operator(self.at(marker)) {
            *token = (self.at(marker) as u8 as char).to_string();
            *type_ = Type::op;
            marker += 1;
            self._cursor = marker;
            true
        } else {
            false
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::dom
    ///   [ <isUUID> | <isDigit>+ . ] <isIdentifier> [ . <isIdentifier> ]*
    ///
    /// ```text
    /// Configuration:
    ///   rc.<name>
    ///
    /// System:
    ///   tw.syncneeded
    ///   tw.program
    ///   tw.args
    ///   tw.width
    ///   tw.height
    ///   context.program      // 2017-02-25 Deprecated in 2.6.0
    ///   context.args         // 2017-02-25 Deprecated in 2.6.0
    ///   context.width        // 2017-02-25 Deprecated in 2.6.0
    ///   context.height       // 2017-02-25 Deprecated in 2.6.0
    ///   system.version
    ///   system.os
    ///
    /// Relative or absolute attribute:
    ///   <attribute>
    ///   <id>.<attribute>
    ///   <uuid>.<attribute>
    ///
    /// Single tag:
    ///   tags.<word>
    ///
    /// Date type:
    ///   <date>.year
    ///   <date>.month
    ///   <date>.day
    ///   <date>.week
    ///   <date>.weekday
    ///   <date>.julian
    ///   <date>.hour
    ///   <date>.minute
    ///   <date>.second
    ///
    /// Annotations (entry is a date):
    ///   annotations.count
    ///   annotations.<N>.entry
    ///   annotations.<N>.description
    /// ```
    pub fn is_dom(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let marker = self._cursor;

        // rc. ...
        let mut partial_token = String::new();
        let mut partial_type = Type::word;
        if self.is_literal("rc.", false, false) && self.is_word(&mut partial_token, &mut partial_type) {
            // Configuration names may contain dashes, as in 'context.my-ctx',
            // so keep consuming as long as a dash is followed by more of the
            // name.
            while self.at(self._cursor) == b'-' as i32 {
                let dash = self._cursor;
                self._cursor += 1;
                if !self.is_word(&mut partial_token, &mut partial_type) {
                    self._cursor = dash;
                    break;
                }
            }

            *token = substr(&self._text, marker, self._cursor - marker);
            *type_ = Type::dom;
            return true;
        } else {
            self._cursor = marker;
        }

        // Literals
        if self.is_one_of(
            &[
                "tw.syncneeded",
                "tw.program",
                "tw.args",
                "tw.width",
                "tw.height",
                "tw.version",
                "context.program",
                "context.args",
                "context.width",
                "context.height",
                "system.version",
                "system.os",
            ],
            false,
            true,
        ) {
            *token = substr(&self._text, marker, self._cursor - marker);
            *type_ = Type::dom;
            return true;
        }

        // Optional:
        //   <uuid>.
        //   <id>.
        let mut extracted_token = String::new();
        let mut extracted_type = Type::word;
        if (self.is_uuid(&mut extracted_token, &mut extracted_type, false)
            || self.is_integer(&mut extracted_token, &mut extracted_type))
            && !self.is_literal(".", false, false)
        {
            self._cursor = marker;
            return false;
        }

        // Any failure after this line should rollback to the checkpoint.
        let checkpoint = self._cursor;

        // [prefix]tags.<word>
        if self.is_literal("tags", false, false)
            && self.is_literal(".", false, false)
            && self.is_word(&mut partial_token, &mut partial_type)
        {
            *token = substr(&self._text, marker, self._cursor - marker);
            *type_ = Type::dom;
            return true;
        } else {
            self._cursor = checkpoint;
        }

        // [prefix]attribute (bounded)
        if ATTRIBUTES.with(|a| self.is_one_of_map(&a.borrow(), false, true)) {
            *token = substr(&self._text, marker, self._cursor - marker);
            *type_ = Type::dom;
            return true;
        }

        // [prefix]attribute. (unbounded)
        if ATTRIBUTES.with(|a| self.is_one_of_map(&a.borrow(), false, false)) {
            if self.is_literal(".", false, false) {
                let attribute = substr(&self._text, checkpoint, self._cursor - checkpoint - 1);

                // if attribute type is 'date', then it has sub-elements.
                // (C++ `attributes[attribute]` inserts a missing key.)
                let is_date_attribute =
                    ATTRIBUTES.with(|a| a.borrow_mut().entry(attribute).or_default().as_str() == "date");
                if is_date_attribute
                    && self.is_one_of(
                        &["year", "month", "day", "week", "weekday", "julian", "hour", "minute", "second"],
                        false,
                        true,
                    )
                {
                    *token = substr(&self._text, marker, self._cursor - marker);
                    *type_ = Type::dom;
                    return true;
                }

                self._cursor = checkpoint;
            }
            // Lookahead: !<alpha>
            else if !unicode_latin_alpha(self.at(marker) as u32) {
                *token = substr(&self._text, marker, self._cursor - marker);
                *type_ = Type::dom;
                return true;
            }

            self._cursor = checkpoint;
        }

        // [prefix]annotations.
        if self.is_literal("annotations", true, false) && self.is_literal(".", false, false) {
            if self.is_literal("count", false, false) {
                *token = substr(&self._text, marker, self._cursor - marker);
                *type_ = Type::dom;
                return true;
            }

            let mut extracted_token = String::new();
            let mut extracted_type = Type::word;
            if self.is_integer(&mut extracted_token, &mut extracted_type) {
                if self.is_literal(".", false, false) {
                    // C++: if description (bounded) / else if entry (bounded) /
                    // else if entry.<date part>, each with the same body.
                    if self.is_literal("description", false, true)
                        || self.is_literal("entry", false, true)
                        || (self.is_literal("entry", false, false)
                            && self.is_literal(".", false, false)
                            && self.is_one_of(
                                &["year", "month", "day", "week", "weekday", "julian", "hour", "minute", "second"],
                                false,
                                true,
                            ))
                    {
                        *token = substr(&self._text, marker, self._cursor - marker);
                        *type_ = Type::dom;
                        return true;
                    }
                }
            } else {
                self._cursor = checkpoint;
            }
        }

        self._cursor = marker;
        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::identifier
    ///   <isIdentifierStart> [ <isIdentifierNext> ]*
    pub fn is_identifier(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;

        if Self::is_identifier_start(self.at(marker)) {
            utf8_next_char(&self._text, &mut marker);

            while Self::is_identifier_next(self.at(marker)) {
                utf8_next_char(&self._text, &mut marker);
            }

            *token = substr(&self._text, self._cursor, marker - self._cursor);
            *type_ = Type::identifier;
            self._cursor = marker;
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// Lexer::Type::word
    ///   [^\s]+
    pub fn is_word(&mut self, token: &mut String, type_: &mut Type) -> bool {
        let mut marker = self._cursor;

        while self.at(marker) != 0 && !ws(self.at(marker)) && !Self::is_single_char_operator(self.at(marker)) {
            utf8_next_char(&self._text, &mut marker);
        }

        if marker > self._cursor {
            *token = substr(&self._text, self._cursor, marker - self._cursor);
            *type_ = Type::word;
            self._cursor = marker;
            return true;
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isLiteral`.
    pub fn is_literal(&mut self, literal: &str, allow_abbreviations: bool, end_boundary: bool) -> bool {
        let common = Self::common_length_at(literal, 0, &self._text, self._cursor);

        // Without abbreviations, common must equal literal length.
        if !allow_abbreviations && common < literal.len() {
            return false;
        }

        // Abbreviations must meet the minimum size.
        if allow_abbreviations && common < Self::minimum_match_length() {
            return false;
        }

        // End boundary conditions must be met.
        let next = self.at(self._cursor + common);
        if end_boundary && next != 0 && !ws(next) && !Self::is_single_char_operator(next) {
            return false;
        }

        self._cursor += common;
        true
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isOneOf (const std::vector<std::string>&, bool, bool)`.
    pub fn is_one_of<S: AsRef<str>>(&mut self, options: &[S], allow_abbreviations: bool, end_boundary: bool) -> bool {
        for item in options {
            if self.is_literal(item.as_ref(), allow_abbreviations, end_boundary) {
                return true;
            }
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isOneOf (const std::map<std::string, std::string>&, bool,
    /// bool)`: tries each key, in map order.
    pub fn is_one_of_map(
        &mut self,
        options: &BTreeMap<String, String>,
        allow_abbreviations: bool,
        end_boundary: bool,
    ) -> bool {
        for item in options.keys() {
            if self.is_literal(item, allow_abbreviations, end_boundary) {
                return true;
            }
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ static `Lexer::typeToString`: the type name with ANSI colors.
    pub fn type_to_string(type_: Type) -> String {
        let (color, name) = match type_ {
            Type::string => ("\x1b[38;5;7m\x1b[48;5;3m", "string"),
            Type::uuid => ("\x1b[38;5;7m\x1b[48;5;10m", "uuid"),
            Type::hex => ("\x1b[38;5;7m\x1b[48;5;14m", "hex"),
            Type::number => ("\x1b[38;5;7m\x1b[48;5;6m", "number"),
            Type::separator => ("\x1b[38;5;7m\x1b[48;5;4m", "separator"),
            Type::url => ("\x1b[38;5;7m\x1b[48;5;4m", "url"),
            Type::pair => ("\x1b[38;5;7m\x1b[48;5;1m", "pair"),
            Type::set => ("\x1b[38;5;15m\x1b[48;5;208m", "set"),
            Type::tag => ("\x1b[37;45m", "tag"),
            Type::path => ("\x1b[37;102m", "path"),
            Type::substitution => ("\x1b[37;102m", "substitution"),
            Type::pattern => ("\x1b[37;42m", "pattern"),
            Type::op => ("\x1b[38;5;7m\x1b[48;5;203m", "op"),
            Type::dom => ("\x1b[38;5;15m\x1b[48;5;244m", "dom"),
            Type::identifier => ("\x1b[38;5;15m\x1b[48;5;244m", "identifier"),
            Type::word => ("\x1b[38;5;15m\x1b[48;5;236m", "word"),
            Type::date => ("\x1b[38;5;15m\x1b[48;5;34m", "date"),
            Type::duration => ("\x1b[38;5;15m\x1b[48;5;34m", "duration"),
        };
        format!("{color}{name}\x1b[0m")
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::isAllDigits`.
    pub fn is_all_digits(text: &str) -> bool {
        !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ static `Lexer::isDOM (const std::string&)`. This is intentionally
    /// looking for a single token.
    pub fn is_dom_str(text: &str) -> bool {
        let mut lex = Lexer::new(text);
        let mut count = 0;
        let mut token = String::new();
        let mut type_ = Type::word;
        while lex.token(&mut token, &mut type_) {
            count += 1;
        }

        count == 1 && type_ == Type::dom
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::readWord (text, quotes, cursor, word)`.
    ///
    /// Full implementation of a quoted word.  Includes:
    /// ```text
    ///   '\''
    ///   '"'
    ///   "'"
    ///   "\""
    ///   'one two'
    /// ```
    /// Result includes the quotes.
    pub fn read_word_quoted(text: &str, quotes: &str, cursor: &mut usize, word: &mut String) -> bool {
        let bytes = text.as_bytes();
        if !find_char(quotes, ch(bytes, *cursor)) {
            return false;
        }

        let eos = bytes.len();
        let quote = ch(bytes, *cursor);
        *cursor += 1;
        let mut w: Vec<u8> = vec![quote as u8];

        loop {
            let mut c = ch(bytes, *cursor);
            if c == 0 {
                break;
            }

            // Quoted word ends on a quote.
            if quote != 0 && quote == c {
                w.extend_from_slice(utf8_character(utf8_next_char(text, cursor)).as_bytes());
                break;
            }
            // Unicode U+XXXX or \uXXXX codepoint.
            else if is_unicode_escape(bytes, *cursor, eos) {
                let codepoint = Self::hex_to_int4(
                    ch(bytes, *cursor + 2),
                    ch(bytes, *cursor + 3),
                    ch(bytes, *cursor + 4),
                    ch(bytes, *cursor + 5),
                );
                w.extend_from_slice(utf8_character(codepoint as u32).as_bytes());
                *cursor += 6;
            }
            // An escaped thing.
            else if c == b'\\' as i32 {
                *cursor += 1;
                c = ch(bytes, *cursor);
                w.push(escaped_byte(c));
                *cursor += 1;
            }
            // Ordinary character.
            else {
                w.extend_from_slice(utf8_character(utf8_next_char(text, cursor)).as_bytes());
            }
        }

        *word = bytes_to_string(&w);

        // Verify termination.
        ch(&w, 0) == quote && ch(&w, w.len() - 1) == quote && w.len() >= 2
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::readWord (text, cursor, word)`.
    ///
    /// Full implementation of an unquoted word.  Includes:
    /// ```text
    ///   one\ two
    ///   abcU+0020def
    ///   abc def
    ///   a\tb
    /// ```
    ///
    /// Ends at:
    ///   Lexer::isEOS
    ///   unicodeWhitespace
    ///   Lexer::isHardBoundary
    pub fn read_word(text: &str, cursor: &mut usize, word: &mut String) -> bool {
        let bytes = text.as_bytes();
        let eos = bytes.len();

        let mut w: Vec<u8> = Vec::new();
        let mut prev: i32 = 0;
        loop {
            // Handles EOS.
            let mut c = ch(bytes, *cursor);
            if c == 0 {
                break;
            }

            // Unquoted word ends on white space.
            if ws(c) {
                break;
            }

            // Parentheses mostly.
            if prev != 0 && Self::is_hard_boundary(prev, c) {
                break;
            }
            // Unicode U+XXXX or \uXXXX codepoint.
            else if is_unicode_escape(bytes, *cursor, eos) {
                let codepoint = Self::hex_to_int4(
                    ch(bytes, *cursor + 2),
                    ch(bytes, *cursor + 3),
                    ch(bytes, *cursor + 4),
                    ch(bytes, *cursor + 5),
                );
                w.extend_from_slice(utf8_character(codepoint as u32).as_bytes());
                *cursor += 6;
            }
            // An escaped thing.
            else if c == b'\\' as i32 {
                *cursor += 1;
                c = ch(bytes, *cursor);
                w.push(escaped_byte(c));
                *cursor += 1;
            }
            // Ordinary character.
            else {
                w.extend_from_slice(utf8_character(utf8_next_char(text, cursor)).as_bytes());
            }

            prev = c;
        }

        *word = bytes_to_string(&w);
        !w.is_empty()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::decomposePair`.
    ///
    /// `<name> [. <modifier>] <: | = | :: | :=> [<value>]`
    pub fn decompose_pair(
        text: &str,
        name: &mut String,
        modifier: &mut String,
        separator: &mut String,
        value: &mut String,
    ) -> bool {
        // Look for the required elements.
        let dot = text.find('.');
        let sep_defer = text.find("::");
        let sep_eval = text.find(":=");
        let sep_colon = text.find(':');
        let sep_equal = text.find('=');

        // Determine which separator is dominant, which would be the first one
        // seen, taking into consideration the overlapping : characters.
        let mut sep: Option<usize> = None;
        let mut sep_end: usize = 0;
        if let Some(d) = sep_defer.filter(|&d| le_npos(d, sep_eval) && le_npos(d, sep_colon) && le_npos(d, sep_equal)) {
            sep = Some(d);
            sep_end = d + 2;
        } else if let Some(e) =
            sep_eval.filter(|&e| le_npos(e, sep_defer) && le_npos(e, sep_colon) && le_npos(e, sep_equal))
        {
            sep = Some(e);
            sep_end = e + 2;
        } else if let Some(c) =
            sep_colon.filter(|&c| le_npos(c, sep_defer) && le_npos(c, sep_eval) && le_npos(c, sep_equal))
        {
            sep = Some(c);
            sep_end = c + 1;
        } else if let Some(q) =
            sep_equal.filter(|&q| le_npos(q, sep_defer) && le_npos(q, sep_eval) && le_npos(q, sep_colon))
        {
            sep = Some(q);
            sep_end = q + 1;
        }

        // If sep is known, all is well.
        if let Some(sep) = sep {
            // Now the only unknown is whethere there is a modifier.
            match dot {
                Some(dot) if dot < sep => {
                    *name = substr(text, 0, dot);
                    *modifier = substr(text, dot + 1, sep - dot - 1);
                }
                _ => {
                    *name = substr(text, 0, sep);
                    *modifier = String::new();
                }
            }

            *separator = substr(text, sep, sep_end - sep);
            *value = substr_from(text, sep_end);

            // An empty name is an error.
            if !name.is_empty() {
                return true;
            }
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::decomposeSubstitution`.
    ///
    /// `/ <from> / <to> / [<flags>]`
    pub fn decompose_substitution(text: &str, from: &mut String, to: &mut String, flags: &mut String) -> bool {
        let mut parsed_from = String::new();
        let mut cursor: usize = 0;
        if Self::read_word_quoted(text, "/", &mut cursor, &mut parsed_from) && !parsed_from.is_empty() {
            cursor -= 1;
            let mut parsed_to = String::new();
            if Self::read_word_quoted(text, "/", &mut cursor, &mut parsed_to) {
                let parsed_flags = substr_from(text, cursor);
                if !parsed_flags.contains('/') {
                    Self::dequote_with(&mut parsed_from, "/");
                    Self::dequote_with(&mut parsed_to, "/");

                    *from = parsed_from;
                    *to = parsed_to;
                    *flags = parsed_flags;
                    return true;
                }
            }
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Lexer::decomposePattern`.
    ///
    /// `/ <pattern> / [<flags>]`
    ///
    /// The pattern length expression `cursor - 2 - flags.length ()` is kept
    /// verbatim from C++ (it also subtracts the flags length).
    pub fn decompose_pattern(text: &str, pattern: &mut String, flags: &mut String) -> bool {
        let mut ignored = String::new();
        let mut cursor: usize = 0;
        if Self::read_word_quoted(text, "/", &mut cursor, &mut ignored) && !ignored.is_empty() {
            let parsed_flags = substr_from(text, cursor);
            if !parsed_flags.contains('/') {
                *flags = parsed_flags;
                *pattern = substr(text, 1, cursor.wrapping_sub(2).wrapping_sub(flags.len()));
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str) -> Vec<(String, Type)> {
        let mut lexer = Lexer::new(text);
        let mut token = String::new();
        let mut type_ = Type::word;
        let mut all = Vec::new();
        while lexer.token(&mut token, &mut type_) {
            all.push((token.clone(), type_));
        }
        all
    }

    fn single(text: &str) -> (String, Type) {
        let all = tokens(text);
        assert_eq!(all.len(), 1, "{text:?} -> {all:?}");
        all[0].clone()
    }

    #[test]
    fn statics_defaults_and_setters() {
        assert_eq!(Lexer::date_format(), "");
        assert_eq!(Lexer::minimum_match_length(), 3);
        assert!(Lexer::attributes().is_empty());

        Lexer::set_date_format("Y-M-D");
        assert_eq!(Lexer::date_format(), "Y-M-D");
        Lexer::set_date_format("");

        Lexer::set_minimum_match_length(5);
        assert_eq!(Lexer::minimum_match_length(), 5);
        Lexer::set_minimum_match_length(3);

        Lexer::insert_attribute("due", "date");
        assert_eq!(Lexer::with_attributes(|a| a.get("due").cloned()), Some("date".to_string()));
        Lexer::set_attributes(BTreeMap::new());
        assert!(Lexer::attributes().is_empty());
    }

    #[test]
    fn empty_and_whitespace() {
        assert!(tokens("").is_empty(), "'' --> no tokens");
        assert!(tokens("       \t ").is_empty(), "'       \\t ' --> no tokens");
    }

    #[test]
    fn was_quoted_and_dequote() {
        assert!(!Lexer::was_quoted(""), "'' --> !wasQuoted");
        assert!(!Lexer::was_quoted("foo"), "'foo' --> !wasQuoted");
        assert!(Lexer::was_quoted("a b"), "'a b' --> wasQuoted");
        assert!(Lexer::was_quoted("(a)"), "'(a)' --> wasQuoted");

        let mut token = "foo".to_string();
        Lexer::dequote(&mut token);
        assert_eq!(token, "foo", "dequote foo --> foo");

        token = "'foo'".to_string();
        Lexer::dequote(&mut token);
        assert_eq!(token, "foo", "dequote 'foo' --> foo");

        token = "'o\\'clock'".to_string();
        Lexer::dequote(&mut token);
        assert_eq!(token, "o\\'clock", "dequote 'o\\'clock' --> o\\'clock");

        token = "abba".to_string();
        Lexer::dequote_with(&mut token, "a");
        assert_eq!(token, "bb", "dequote 'abba' (a) --> bb");

        token = "'".to_string();
        Lexer::dequote(&mut token);
        assert_eq!(token, "");
    }

    #[test]
    fn common_length() {
        assert_eq!(Lexer::common_length("", ""), 0);
        assert_eq!(Lexer::common_length("a", "a"), 1);
        assert_eq!(Lexer::common_length("abcde", "abcde"), 5);
        assert_eq!(Lexer::common_length("abc", ""), 0);
        assert_eq!(Lexer::common_length("abc", "def"), 0);
        assert_eq!(Lexer::common_length("foobar", "foo"), 3);
        assert_eq!(Lexer::common_length("foo", "foobar"), 3);
        assert_eq!(Lexer::common_length_at("wonder", 0, "prowonderbread", 3), 6);
        assert_eq!(Lexer::common_length_at("wonderful", 0, "wonderbread", 0), 6);
    }

    #[test]
    fn commify_trim_case() {
        for (input, expected) in [
            ("", ""),
            ("1", "1"),
            ("123", "123"),
            ("1234", "1,234"),
            ("1234567890", "1,234,567,890"),
            ("1234.0", "1,234.0"),
            ("1234567890.0", "1,234,567,890.0"),
            ("pre", "pre"),
            ("pre1234", "pre1,234"),
            ("1234post", "1,234post"),
            ("pre1234post", "pre1,234post"),
        ] {
            assert_eq!(Lexer::commify(input), expected, "Lexer::commify '{input}'");
        }

        assert_eq!(Lexer::trim_left(""), "");
        assert_eq!(Lexer::trim_left("   "), "");
        assert_eq!(Lexer::trim_left_with("", " \t"), "");
        assert_eq!(Lexer::trim_left("xxx"), "xxx");
        assert_eq!(Lexer::trim_left("  xxx "), "xxx ");
        assert_eq!(Lexer::trim_right("  xxx  "), "  xxx");
        assert_eq!(Lexer::trim_right("   "), "");
        assert_eq!(Lexer::trim_with(" \t xxx \t ", " \t"), "xxx");
        assert_eq!(Lexer::trim("  x y  "), "x y");

        assert_eq!(Lexer::lower_case("ABC def"), "abc def");
        assert_eq!(Lexer::uc_first("abc"), "Abc");
        assert_eq!(Lexer::uc_first(""), "");
        assert_eq!(Lexer::uc_first("éa"), "éa");
    }

    #[test]
    fn hex_digits_and_classes() {
        assert_eq!(Lexer::hex_to_int('0' as i32), 0);
        assert_eq!(Lexer::hex_to_int('9' as i32), 9);
        assert_eq!(Lexer::hex_to_int('a' as i32), 10);
        assert_eq!(Lexer::hex_to_int('F' as i32), 15);
        assert_eq!(Lexer::hex_to_int2('f' as i32, 'f' as i32), 255);
        assert_eq!(Lexer::hex_to_int4('2' as i32, '0' as i32, 'a' as i32, 'c' as i32), 0x20ac);

        assert!(Lexer::is_all_digits("0123"));
        assert!(!Lexer::is_all_digits(""));
        assert!(!Lexer::is_all_digits("1a"));

        assert!(Lexer::is_punctuation('.' as i32));
        assert!(!Lexer::is_punctuation('_' as i32));
        assert!(!Lexer::is_punctuation(-61)); // UTF-8 lead byte as signed char
        assert!(Lexer::is_identifier_start('a' as i32));
        assert!(!Lexer::is_identifier_start('1' as i32));
        assert!(!Lexer::is_identifier_next(':' as i32));
        assert!(Lexer::is_boundary('a' as i32, 0));
        assert!(Lexer::is_boundary('r' as i32, ' ' as i32));
        assert!(!Lexer::is_boundary('r' as i32, 'x' as i32));
        assert!(Lexer::is_double_char_operator('o' as i32, 'r' as i32, 0));
        assert!(!Lexer::is_double_char_operator('o' as i32, 'r' as i32, 'g' as i32));
        assert!(Lexer::is_triple_char_operator('a' as i32, 'n' as i32, 'd' as i32, 0));
        assert!(Lexer::is_hard_boundary('a' as i32, '(' as i32));
    }

    #[test]
    fn type_names() {
        assert_eq!(Lexer::type_name(&Type::uuid), "uuid");
        assert_eq!(Lexer::type_name(&Type::op), "op");
        assert_eq!(Lexer::type_name(&Type::duration), "duration");
        assert_eq!(Lexer::type_to_string(Type::tag), "\x1b[37;45mtag\x1b[0m");
    }

    #[test]
    fn read_word_variants() {
        let mut cursor = 0;
        let mut word = String::new();
        assert!(Lexer::read_word_quoted("'one two' three", "'\"", &mut cursor, &mut word));
        assert_eq!(word, "'one two'");
        assert_eq!(cursor, 9);

        cursor = 0;
        assert!(!Lexer::read_word_quoted("'unterminated", "'\"", &mut cursor, &mut word));

        cursor = 0;
        assert!(Lexer::read_word_quoted("\"a\\\"b\"", "'\"", &mut cursor, &mut word));
        assert_eq!(word, "\"a\"b\"");

        cursor = 0;
        assert!(Lexer::read_word("one\\ two three", &mut cursor, &mut word));
        assert_eq!(word, "one two");
        assert_eq!(cursor, 8);

        cursor = 0;
        assert!(Lexer::read_word("abcU+0020def", &mut cursor, &mut word));
        assert_eq!(word, "abc def");

        cursor = 0;
        assert!(Lexer::read_word("abc\\u20acdef", &mut cursor, &mut word));
        assert_eq!(word, "abc\u{20ac}def");

        cursor = 0;
        assert!(Lexer::read_word("a(b", &mut cursor, &mut word));
        assert_eq!(word, "a");
    }

    #[test]
    fn decompositions() {
        let (mut name, mut modifier, mut sep, mut value) = (String::new(), String::new(), String::new(), String::new());
        assert!(Lexer::decompose_pair("name.mod:=value", &mut name, &mut modifier, &mut sep, &mut value));
        assert_eq!((name.as_str(), modifier.as_str(), sep.as_str(), value.as_str()), ("name", "mod", ":=", "value"));
        assert!(Lexer::decompose_pair("rc.x::y", &mut name, &mut modifier, &mut sep, &mut value));
        assert_eq!((name.as_str(), modifier.as_str(), sep.as_str(), value.as_str()), ("rc", "x", "::", "y"));
        assert!(Lexer::decompose_pair("name=", &mut name, &mut modifier, &mut sep, &mut value));
        assert_eq!((name.as_str(), modifier.as_str(), sep.as_str(), value.as_str()), ("name", "", "=", ""));
        assert!(!Lexer::decompose_pair(":value", &mut name, &mut modifier, &mut sep, &mut value));
        assert!(!Lexer::decompose_pair("novalue", &mut name, &mut modifier, &mut sep, &mut value));

        let (mut from, mut to, mut flags) = (String::new(), String::new(), String::new());
        assert!(Lexer::decompose_substitution("/a/b/g", &mut from, &mut to, &mut flags));
        assert_eq!((from.as_str(), to.as_str(), flags.as_str()), ("a", "b", "g"));
        assert!(!Lexer::decompose_substitution("/a/b/c/", &mut from, &mut to, &mut flags));

        let mut pattern = String::new();
        assert!(Lexer::decompose_pattern("/foo/", &mut pattern, &mut flags));
        assert_eq!((pattern.as_str(), flags.as_str()), ("foo", ""));
        assert!(!Lexer::decompose_pattern("foo", &mut pattern, &mut flags));
    }

    #[test]
    fn simple_tokens() {
        assert_eq!(single("'one two'"), ("'one two'".to_string(), Type::string));
        assert_eq!(single("+tag"), ("+tag".to_string(), Type::tag));
        assert_eq!(single("-tag"), ("-tag".to_string(), Type::tag));
        assert_eq!(single("name:value"), ("name:value".to_string(), Type::pair));
        assert_eq!(single("pro:'P 1'"), ("pro:'P 1'".to_string(), Type::pair));
        assert_eq!(single("1,2"), ("1,2".to_string(), Type::set));
        assert_eq!(single("1-2,4,6-8"), ("1-2,4,6-8".to_string(), Type::set));
        assert_eq!(single("0x2f"), ("0x2f".to_string(), Type::hex));
        assert_eq!(single("http://example.com"), ("http://example.com".to_string(), Type::url));
        assert_eq!(single("/long/path/to/file.txt"), ("/long/path/to/file.txt".to_string(), Type::path));
        assert_eq!(single("/from/to/g"), ("/from/to/g".to_string(), Type::substitution));
        assert_eq!(single("/foo/"), ("/foo/".to_string(), Type::pattern));
        assert_eq!(single("_hastag_"), ("_hastag_".to_string(), Type::op));
        assert_eq!(single("<="), ("<=".to_string(), Type::op));
        assert_eq!(single("rc.foo"), ("rc.foo".to_string(), Type::dom));
        assert_eq!(single("rc.context.my-ctx"), ("rc.context.my-ctx".to_string(), Type::dom));
        assert_eq!(single("system.os"), ("system.os".to_string(), Type::dom));
        assert_eq!(single("foo.bar"), ("foo.bar".to_string(), Type::identifier));
        assert!(Lexer::is_dom_str("rc.foo"));
        assert!(!Lexer::is_dom_str("rc.foo bar"));
    }

    #[test]
    fn mixed_expression() {
        let all = tokens("--  ( +tag or -foo ) and 'x'");
        let expected = [
            ("--", Type::separator),
            ("(", Type::op),
            ("+tag", Type::tag),
            ("or", Type::op),
            ("-foo", Type::tag),
            (")", Type::op),
            ("and", Type::op),
            ("'x'", Type::string),
        ];
        assert_eq!(all.len(), expected.len(), "{all:?}");
        for ((token, type_), (etoken, etype)) in all.iter().zip(expected.iter()) {
            assert_eq!((token.as_str(), *type_), (*etoken, *etype));
        }

        assert_eq!(Lexer::split("  one   two "), vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn dom_with_attributes() {
        // The lexer_test.cpp setup (as Context does at startup): without it
        // a standalone "123" is a Duration (seconds), exactly as in C++.
        // These settings are thread-locals; restore them even on panic.
        struct Restore(BTreeMap<String, String>, bool, bool, bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                Lexer::set_attributes(std::mem::take(&mut self.0));
                Datetime::set_standalone_date_enabled(self.1);
                Datetime::set_standalone_time_enabled(self.2);
                Duration::set_standalone_seconds_enabled(self.3);
            }
        }
        let _restore = Restore(
            Lexer::attributes(),
            Datetime::standalone_date_enabled(),
            Datetime::standalone_time_enabled(),
            Duration::standalone_seconds_enabled(),
        );
        Datetime::set_standalone_date_enabled(false);
        Datetime::set_standalone_time_enabled(false);
        Duration::set_standalone_seconds_enabled(false);

        Lexer::insert_attribute("due", "date");
        Lexer::insert_attribute("description", "string");

        assert_eq!(single("due"), ("due".to_string(), Type::dom));
        assert_eq!(single("123.description"), ("123.description".to_string(), Type::dom));
        assert_eq!(single("due.year"), ("due.year".to_string(), Type::dom));
        assert_eq!(single("123.tags.PENDING"), ("123.tags.PENDING".to_string(), Type::dom));
        assert_eq!(single("123.annotations.1.entry"), ("123.annotations.1.entry".to_string(), Type::dom));
    }
}

#[cfg(test)]
#[path = "lexer_tests.rs"]
mod upstream_tests;
