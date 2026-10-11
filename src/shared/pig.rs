//! Rust translation of `libshared src/Pig.cpp` (everything declared in
//! `Pig.h`): a cursor-based text parser.
//!
//! `Pig` wraps an immutable text, shared between clones as in C++ (where the
//! text is a `std::shared_ptr<std::string>`), and a cursor that is a byte
//! offset into it. Every method has the same name (in snake case) and the
//! same semantics as its C++ counterpart:
//!
//! - The C++ out-parameters are kept as `&mut` out-parameters, and each
//!   method still returns the C++ `bool`. An out-parameter is written only
//!   where the C++ writes it, so a failed call leaves it unchanged (or, as in
//!   `get_hms`, partly written). This keeps C++ call chains such as
//!   `getDigit2 (h) || getDigit (h)` translatable line for line.
//! - The C++ `(*_text)[i]` reads a byte as a signed `char`; index `len`, and
//!   (in this port) any index past it, reads NUL. So an embedded NUL counts
//!   as end of string, as in C++.
//! - The UTF-8 walking methods use `utf8_next_char` and the `unicode*`
//!   predicates exactly as the C++ does. The byte-level methods (`skip`,
//!   `skip_partial`, `get_number`, ...) compare bytes as the C++ does.
//! - C++ overloads get distinct names: `getDigits (int&)` is `get_digits`,
//!   `getDigits (long long&)` is `get_digits_i64`, `getNumber/getDecimal
//!   (std::string&)` are `get_number`/`get_decimal`, the `double&` overloads
//!   are `get_number_f64`/`get_decimal_f64`, and `peek (int)` is `peek_n`.
//! - The C++ `int` character parameters of `skip`, `getUntil` and `getQuoted`
//!   are `char` here. For every non-negative C++ argument the result is the
//!   same; only ASCII can ever match the signed byte compared against in
//!   `skip` and `getQuoted`.
//!
//! Deviations, all limited to cases that are impossible or undefined in C++:
//! - A Rust `String` must be valid UTF-8. The byte-level methods can leave
//!   the cursor inside a multi-byte sequence (for example `skip_partial`
//!   with "è" against "é", which share their lead byte). A substring that
//!   splits a sequence has the partial sequence replaced by U+FFFD, where
//!   C++ would return the raw bytes.
//! - Where C++ would throw `std::out_of_range` (a `substr` starting past the
//!   end, reachable only after `skip ('\0')` at end of string or a bad
//!   `restore_to`), this port clamps the start to the end of the text.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.

use std::rc::Rc;

use crate::shared::shared::str_replace;
use crate::shared::unicode::{unicode_hex_digit, unicode_latin_digit, unicode_whitespace};
use crate::shared::utf8::{utf8_next_char, utf8_sequence};

/// C++ `Pig`: a text and a byte cursor into it.
#[derive(Debug, Clone)]
pub struct Pig {
    text: Rc<str>,
    cursor: usize,
    saved: usize,
}

impl Pig {
    /// C++ `Pig::Pig (const std::string&)`.
    pub fn new(text: &str) -> Self {
        Pig {
            text: Rc::from(text),
            cursor: 0,
            saved: 0,
        }
    }

    // ---------------------------------------------------------------------
    // Byte-access helpers mirroring `std::string` operations.

    fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }

    /// C++ `(*_text)[i]` as the raw byte: NUL at (and, here, past) the end.
    fn byte(&self, i: usize) -> u8 {
        self.bytes().get(i).copied().unwrap_or(0)
    }

    /// C++ `(*_text)[i]` promoted to `int` (signed `char`).
    fn signed(&self, i: usize) -> i32 {
        self.byte(i) as i8 as i32
    }

    /// C++ `(*_text)[i]` promoted to `unsigned int` (as passed to the
    /// `unicode*` predicates).
    fn unsigned(&self, i: usize) -> u32 {
        self.signed(i) as u32
    }

    /// C++ `_text->substr (start, count)`, with `count` clamped to the end
    /// of the text (`std::string::npos` is `usize::MAX`).
    fn sub(&self, start: usize, count: usize) -> String {
        let bytes = self.bytes();
        let start = start.min(bytes.len());
        let end = start + count.min(bytes.len() - start);
        bytes_to_string(&bytes[start..end])
    }

    // ---------------------------------------------------------------------

    /// C++ `Pig::skip (int)`: skips one byte if it equals `c`.
    pub fn skip(&mut self, c: char) -> bool {
        if self.signed(self.cursor) == c as i32 {
            self.cursor += 1;
            return true;
        }

        false
    }

    /// C++ `Pig::skipN (const int quantity = 1)`: skips `quantity` UTF-8
    /// characters, or nothing if there are fewer.
    pub fn skip_n(&mut self, quantity: i32) -> bool {
        let save = self.cursor;

        let mut count = 0;
        while {
            let more = count < quantity;
            count += 1;
            more
        } {
            if utf8_next_char(&self.text, &mut self.cursor) == 0 {
                self.cursor = save;
                return false;
            }
        }

        true
    }

    /// C++ `Pig::skipWS`: skips Unicode whitespace.
    pub fn skip_ws(&mut self) -> bool {
        let save = self.cursor;

        let mut prev = self.cursor;
        loop {
            let c = utf8_next_char(&self.text, &mut self.cursor);
            if c == 0 {
                break;
            }

            if !unicode_whitespace(c) {
                self.cursor = prev;
                break;
            }
            prev = self.cursor;
        }

        self.cursor > save
    }

    /// C++ `Pig::skipLiteral`.
    pub fn skip_literal(&mut self, literal: &str) -> bool {
        // C++ `_text->find (literal, _cursor) == _cursor`.
        if self.cursor <= self.bytes().len()
            && self.bytes()[self.cursor..].starts_with(literal.as_bytes())
        {
            self.cursor += literal.len();
            return true;
        }

        false
    }

    /// C++ `Pig::skipPartial (reference, result, ignore_case = false)`: skips
    /// the longest (byte) prefix of `reference` found at the cursor. With
    /// `ignore_case`, the text byte is lower-cased (C locale `tolower`) and
    /// compared with the unchanged reference byte.
    pub fn skip_partial(
        &mut self,
        reference: &str,
        result: &mut String,
        ignore_case: bool,
    ) -> bool {
        let reference = reference.as_bytes();
        let reference_at = |pos: usize| reference.get(pos).copied().unwrap_or(0);

        // Walk the common substring.
        let mut pos = 0;
        while reference_at(pos) != 0
            && self.byte(self.cursor + pos) != 0
            && ((reference_at(pos) == self.byte(self.cursor + pos) && !ignore_case)
                || (reference_at(pos) == self.byte(self.cursor + pos).to_ascii_lowercase()
                    && ignore_case))
        {
            pos += 1;
        }

        if pos > 0 {
            *result = self.sub(self.cursor, pos);
            self.cursor += pos;
            return true;
        }

        false
    }

    /// C++ `Pig::getUntilAscii (char, std::string&)`.
    ///
    /// ASCII-only version of `get_until`. Has better performance
    /// characteristics at the cost of not considering UTF-8 characters
    /// correctly. Use with caution.
    pub fn get_until_ascii(&mut self, end: u8, result: &mut String) -> bool {
        let save = self.cursor;
        let len = self.bytes().len();
        let from = self.cursor + 1;
        let found = if from <= len {
            self.bytes()[from..]
                .iter()
                .position(|&b| b == end)
                .map(|p| p + from)
        } else {
            None
        };

        let Some(found) = found else {
            *result = self.sub(self.cursor, len.wrapping_sub(self.cursor));
            self.cursor = len;
            return true;
        };

        *result = self.sub(self.cursor, found - self.cursor);
        self.cursor += found - self.cursor;

        self.cursor > save
    }

    /// C++ `Pig::getUntil (int, std::string&)`: gets UTF-8 characters up to,
    /// not including, `end`, or to the end of the text.
    pub fn get_until(&mut self, end: char, result: &mut String) -> bool {
        let save = self.cursor;

        let mut prev = self.cursor;
        loop {
            let c = utf8_next_char(&self.text, &mut self.cursor);
            if c == 0 {
                break;
            }

            if c == end as u32 {
                self.cursor = prev;
                *result = self.sub(save, self.cursor - save);
                return true;
            } else if self.eos() {
                *result = self.sub(save, self.cursor - save);
                return true;
            }

            prev = self.cursor;
        }

        self.cursor > save
    }

    /// C++ `Pig::getUntilWS`: gets UTF-8 characters up to, not including,
    /// Unicode whitespace, or to the end of the text.
    pub fn get_until_ws(&mut self, result: &mut String) -> bool {
        let save = self.cursor;

        let mut prev = self.cursor;
        loop {
            let c = utf8_next_char(&self.text, &mut self.cursor);
            if c == 0 {
                break;
            }

            if unicode_whitespace(c) {
                self.cursor = prev;
                *result = self.sub(save, self.cursor - save);
                return true;
            }
            // Note: This test must follow the above unicode_whitespace(c) test
            //       because it is testing the value of 'c', and eos() is
            //       testing the cursor, which has already been advanced.
            else if self.eos() {
                *result = self.sub(save, self.cursor - save);
                return true;
            }

            prev = self.cursor;
        }

        self.cursor > save
    }

    /// C++ `Pig::getCharacter`: gets one byte, as a signed `char` value.
    pub fn get_character(&mut self, result: &mut i32) -> bool {
        let c = self.signed(self.cursor);
        if c != 0 {
            *result = c;
            self.cursor += 1;
            return true;
        }

        false
    }

    /// C++ `Pig::getDigit`: gets one ASCII digit's value.
    pub fn get_digit(&mut self, result: &mut i32) -> bool {
        let c = self.signed(self.cursor);
        if c != 0 && unicode_latin_digit(c as u32) {
            *result = c - '0' as i32;
            self.cursor += 1;
            return true;
        }

        false
    }

    /// Shared body of `getDigit2/3/4`: exactly `count` ASCII digits.
    fn get_fixed_digits(&mut self, count: usize, result: &mut i32) -> bool {
        if (0..count).all(|k| unicode_latin_digit(self.unsigned(self.cursor + k))) {
            // C++ `strtoimax` of the digits; at most 4 digits, so no overflow.
            *result = self.bytes()[self.cursor..self.cursor + count]
                .iter()
                .fold(0, |acc, &b| acc * 10 + (b - b'0') as i32);
            self.cursor += count;
            return true;
        }

        false
    }

    /// C++ `Pig::getDigit2`: exactly two ASCII digits.
    pub fn get_digit2(&mut self, result: &mut i32) -> bool {
        self.get_fixed_digits(2, result)
    }

    /// C++ `Pig::getDigit3`: exactly three ASCII digits.
    pub fn get_digit3(&mut self, result: &mut i32) -> bool {
        self.get_fixed_digits(3, result)
    }

    /// C++ `Pig::getDigit4`: exactly four ASCII digits.
    pub fn get_digit4(&mut self, result: &mut i32) -> bool {
        self.get_fixed_digits(4, result)
    }

    /// C++ `Pig::getDigits (int&)`: the `long long` result truncated to `int`
    /// (two's complement wrap, as the C++ conversion does on the reference
    /// platforms).
    pub fn get_digits(&mut self, result: &mut i32) -> bool {
        let mut temp: i64 = 0;

        let success = self.get_digits_i64(&mut temp);
        if success {
            *result = temp as i32;
        }

        success
    }

    /// C++ `Pig::getDigits (long long&)`: one or more ASCII digits.
    pub fn get_digits_i64(&mut self, result: &mut i64) -> bool {
        let save = self.cursor;

        let mut prev = self.cursor;
        loop {
            let c = utf8_next_char(&self.text, &mut self.cursor);
            if c == 0 {
                break;
            }

            if !unicode_latin_digit(c) {
                self.cursor = prev;
                break;
            }

            prev = self.cursor;
        }

        if self.cursor > save {
            // C++ `strtoimax`, which saturates at INTMAX_MAX on overflow. The
            // text is digits only, so overflow can only be positive.
            *result = std::str::from_utf8(&self.bytes()[save..self.cursor])
                .ok()
                .and_then(|digits| digits.parse::<i64>().ok())
                .unwrap_or(i64::MAX);
            return true;
        }

        false
    }

    /// C++ `Pig::getHexDigit`: one hex digit's value.
    pub fn get_hex_digit(&mut self, result: &mut i32) -> bool {
        let c = self.signed(self.cursor);
        if c != 0 && unicode_hex_digit(c as u32) {
            let c = c as u8;
            if c.is_ascii_digit() {
                *result = (c - b'0') as i32;
                self.cursor += 1;
                return true;
            } else if (b'A'..=b'F').contains(&c) {
                *result = (c - b'A') as i32 + 10;
                self.cursor += 1;
                return true;
            } else if (b'a'..=b'f').contains(&c) {
                *result = (c - b'a') as i32 + 10;
                self.cursor += 1;
                return true;
            }
        }

        false
    }

    /// True if byte `i` is an ASCII digit (C++ `(*_text)[i] &&
    /// unicodeLatinDigit ((*_text)[i])`).
    fn digit_at(&self, i: usize) -> bool {
        self.byte(i) != 0 && unicode_latin_digit(self.unsigned(i))
    }

    /// True if byte `i` is one of `options`.
    fn byte_in(&self, i: usize, options: &[u8]) -> bool {
        self.byte(i) != 0 && options.contains(&self.byte(i))
    }

    /// C++ `Pig::getNumber (std::string&)`.
    ///
    /// ```text
    /// number:
    ///   int frac? exp?
    ///
    /// int:
    ///   (-|+)? digit+
    ///
    /// frac:
    ///   . digit+
    ///
    /// exp:
    ///   e digit+
    ///
    /// e:
    ///   e|E (+|-)?
    /// ```
    ///
    /// As in C++, the fraction may have no digits ("1." matches), and an
    /// exponent marker without digits fails the whole match ("1e" -> false,
    /// cursor unchanged).
    pub fn get_number(&mut self, result: &mut String) -> bool {
        let mut i = self.cursor;

        // [+-]?
        if self.byte_in(i, b"-+") {
            i += 1;
        }

        // digit+
        if self.digit_at(i) {
            i += 1;

            while self.digit_at(i) {
                i += 1;
            }

            // ( . digit+ )?
            if self.byte_in(i, b".") {
                i += 1;

                while self.digit_at(i) {
                    i += 1;
                }
            }

            // ( [eE] [+-]? digit+ )?
            if self.byte_in(i, b"eE") {
                i += 1;

                if self.byte_in(i, b"+-") {
                    i += 1;
                }

                if self.digit_at(i) {
                    i += 1;

                    while self.digit_at(i) {
                        i += 1;
                    }

                    *result = self.sub(self.cursor, i - self.cursor);
                    self.cursor = i;
                    return true;
                }

                return false;
            }

            *result = self.sub(self.cursor, i - self.cursor);
            self.cursor = i;
            return true;
        }

        false
    }

    /// C++ `Pig::getNumber (double&)`: `get_number`, converted by `strtod`.
    pub fn get_number_f64(&mut self, result: &mut f64) -> bool {
        let mut s = String::new();
        if self.get_number(&mut s) {
            *result = strtod(&s);
            return true;
        }

        false
    }

    /// C++ `Pig::getDecimal (std::string&)`: `[ + | - ] \d+ [ . [ \d+ ]]`.
    pub fn get_decimal(&mut self, result: &mut String) -> bool {
        let mut i = self.cursor;

        // [+-]?
        if self.byte_in(i, b"-+") {
            i += 1;
        }

        // digit+
        if self.digit_at(i) {
            i += 1;

            while self.digit_at(i) {
                i += 1;
            }

            // ( . digit+ )?
            if self.byte_in(i, b".") {
                i += 1;

                while self.digit_at(i) {
                    i += 1;
                }
            }

            *result = self.sub(self.cursor, i - self.cursor);
            self.cursor = i;
            return true;
        }

        false
    }

    /// C++ `Pig::getDecimal (double&)`: `get_decimal`, converted by `strtod`.
    pub fn get_decimal_f64(&mut self, result: &mut f64) -> bool {
        let mut s = String::new();
        if self.get_decimal(&mut s) {
            *result = strtod(&s);
            return true;
        }

        false
    }

    /// C++ `Pig::getQuoted (int, std::string&)`.
    ///
    /// Gets quote content:      `"foobar"` -> `foobar`      (for quote = '"')
    /// Handles escaped quotes:  `"foo\"bar"` -> `foo\"bar`  (for quote = '"')
    /// Returns false if first character is not `quote`, or if there is no
    /// closing `quote`. Does not modify content between quotes.
    pub fn get_quoted(&mut self, quote: char, result: &mut String) -> bool {
        if self.byte(self.cursor) == 0 || self.signed(self.cursor) != quote as i32 {
            return false;
        }

        // Past the check above, `quote` is ASCII: one byte.
        let quote_byte = quote as u8;
        let quote_len = utf8_sequence(quote as u32) as usize;
        let start = self.cursor + quote_len;
        let mut i = start;

        while self.byte(i) != 0 {
            // C++ `_text->find (quote, i)`.
            let Some(offset) = self.bytes()[i..].iter().position(|&b| b == quote_byte) else {
                return false; // Unclosed quote. Shortcut, not definitive.
            };
            i += offset;

            if i == start {
                // Empty quote
                self.cursor += 2 * quote_len; // Skip both quote chars
                *result = String::new();
                return true;
            }

            if self.byte(i - 1) == b'\\' {
                // Check for escaped backslashes. Backtracking like this is not
                // very efficient, but is only done in extreme corner cases.

                // Start one character further left. `i > start >= 1`, so this
                // cannot underflow, and the loop stops before `j` drops below
                // `start`.
                let mut j = i - 2;
                let mut is_escaped_quote = true;
                while j >= start && self.byte(j) == b'\\' {
                    // Toggle flag for each further backslash encountered.
                    is_escaped_quote = !is_escaped_quote;
                    j -= 1;
                }

                if is_escaped_quote {
                    // Keep searching
                    i += 1;
                    continue;
                }
            }

            // None of the above applied, we must have found the closing quote
            // char.
            *result = self.sub(start, i - start);
            self.cursor = i + quote_len; // Skip closing quote char
            return true;
        }

        // This should never be reached. We could throw here instead.
        false
    }

    /// C++ `Pig::getOneOf`: skips the first of `options` found at the cursor.
    ///
    /// Assumes that the options are sorted by decreasing length, so that if
    /// the options contain 'fourteen' and 'four', the stream is first matched
    /// against the longer entry.
    pub fn get_one_of<S: AsRef<str>>(&mut self, options: &[S], found: &mut String) -> bool {
        for option in options {
            if self.skip_literal(option.as_ref()) {
                *found = option.as_ref().to_string();
                return true;
            }
        }

        false
    }

    /// C++ `Pig::getHMS`: `h:mm` or `hh:mm`, optionally followed by `:ss`.
    ///
    /// As in C++, a trailing ':' not followed by two digits returns false
    /// without restoring the cursor (it is left after that ':').
    pub fn get_hms(&mut self, hours: &mut i32, minutes: &mut i32, seconds: &mut i32) -> bool {
        let save = self.cursor;

        if (self.get_digit2(hours) || self.get_digit(hours))
            && self.skip(':')
            && self.get_digit2(minutes)
        {
            *seconds = 0;
            if self.skip(':') && !self.get_digit2(seconds) {
                return false;
            }

            return true;
        }

        self.cursor = save;
        false
    }

    /// C++ `Pig::getRemainder`: everything from the cursor to the end.
    pub fn get_remainder(&mut self, result: &mut String) -> bool {
        if self.byte(self.cursor) != 0 {
            *result = self.sub(self.cursor, usize::MAX);
            self.cursor = self.bytes().len();
            return true;
        }

        false
    }

    /// C++ `Pig::eos`: true at the end of the text (or an embedded NUL).
    pub fn eos(&self) -> bool {
        self.byte(self.cursor) == 0
    }

    /// C++ `Pig::peek ()`: the byte at the cursor as a signed `char` value.
    /// Does not move the cursor.
    pub fn peek(&self) -> i32 {
        self.signed(self.cursor)
    }

    /// C++ `Pig::peek (const int)`: up to `quantity` bytes from the cursor.
    /// Does not move the cursor. As in C++, a negative `quantity` converts
    /// to a huge `size_t`, so it returns the rest of the text.
    pub fn peek_n(&self, quantity: i32) -> String {
        let adjusted = (quantity as usize).min(self.bytes().len().wrapping_sub(self.cursor));
        if self.byte(self.cursor) != 0 {
            return self.sub(self.cursor, adjusted);
        }

        String::new()
    }

    /// C++ `Pig::cursor`: the byte offset of the cursor.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// C++ `Pig::save`: remembers the cursor, and returns it.
    ///
    /// Note: never called internally, otherwise the client cannot rely on it.
    pub fn save(&mut self) -> usize {
        self.saved = self.cursor;
        self.saved
    }

    /// C++ `Pig::restore`: moves the cursor back to the saved position, and
    /// returns it.
    ///
    /// Note: never called internally, otherwise the client cannot rely on it.
    pub fn restore(&mut self) -> usize {
        self.cursor = self.saved;
        self.cursor
    }

    /// C++ `Pig::restoreTo`: moves the cursor to `previous`, and returns it.
    pub fn restore_to(&mut self, previous: usize) -> usize {
        self.cursor = previous;
        self.cursor
    }

    /// C++ `Pig::substr (start, end)`: the text in byte range `start..end`.
    /// As in C++ (`substr (start, end - start)` with unsigned arithmetic), an
    /// `end` before `start` yields the rest of the text from `start`.
    pub fn substr(&self, start: usize, end: usize) -> String {
        self.sub(start, end.wrapping_sub(start))
    }

    /// C++ `Pig::str`: the text from the cursor on.
    pub fn str(&self) -> String {
        self.sub(self.cursor, usize::MAX)
    }

    /// C++ `Pig::dump`: shows the text, with the matched part in white on
    /// green, and the unmatched part white on red, followed by the index
    /// equivalent (`<cursor>/<length>`). Newlines are shown as `\n`.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        if self.cursor != 0 {
            out.push_str("\x1b[37;42m");
            out.push_str(&self.sub(0, self.cursor));
            out.push_str("\x1b[0m");
        }

        out.push_str("\x1b[37;41m");
        out.push_str(&self.sub(self.cursor, usize::MAX));
        out.push_str("\x1b[0m ");
        out.push_str(&self.cursor.to_string());
        out.push('/');
        out.push_str(&self.bytes().len().to_string());

        str_replace(&out, "\n", "\\n")
    }
}

/// A byte range of the text as a `String`. A range that splits a UTF-8
/// sequence (see the module notes) has the partial sequence replaced by
/// U+FFFD.
fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// C++ `std::strtod (s.c_str (), nullptr)` for the strings `get_number` and
/// `get_decimal` match: `[+-]?\d+(\.\d*)?([eE][+-]?\d+)?`.
///
/// Every such string is accepted in full by Rust's `f64::from_str` (which
/// allows a leading '+', a fraction with no digits, and a signed exponent),
/// and both conversions are correctly rounded to nearest (glibc `strtod` in
/// the default rounding mode, Rust by specification), including overflow to
/// infinity (C++ `HUGE_VAL`), gradual underflow and "-0" -> -0.0. So the
/// results are bit-identical. The fallback 0.0 (C++ `strtod` with no
/// conversion) is unreachable for these inputs.
fn strtod(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    //! Port of libshared `test/pig.t.cpp` (180 assertions). Each C++
    //! `t.ok`/`t.notok`/`t.is` is one assertion here, carrying the C++ test
    //! description as its message. Rust-only edge-case tests for C++
    //! behaviour that pig.t.cpp does not cover follow.

    use super::*;

    /// C++ `p.dump ().find (needle) != std::string::npos`.
    fn dump_has(p: &Pig, needle: &str) -> bool {
        p.dump().contains(needle)
    }

    #[test]
    fn skip_and_skip_n() {
        // Pig::skip
        // Pig::skipN
        let mut p0 = Pig::new("12345");
        assert!(!p0.skip('0'), "skip='0', '12345' --> false");
        assert!(p0.skip('1'), "skip='1', '12345' --> true");
        assert!(dump_has(&p0, " 1/5"), "dump: {}", p0.dump());

        assert!(p0.skip_n(3), "skipN=3 '12345' --> true");
        assert!(dump_has(&p0, " 4/5"), "dump: {}", p0.dump());

        assert!(!p0.skip_n(2), "skipN=2 '5' --> false");
        assert!(dump_has(&p0, " 4/5"), "dump: {}", p0.dump());
    }

    #[test]
    fn skip_ws() {
        // Pig::skipWS
        let mut p1 = Pig::new("  one");
        assert!(p1.skip_ws(), "skipWS '  one' --> true");
        assert!(dump_has(&p1, " 2/5"), "dump: {}", p1.dump());

        assert!(!p1.skip_ws(), "skipWS 'one' --> false");
        assert!(dump_has(&p1, " 2/5"), "dump: {}", p1.dump());
    }

    #[test]
    fn skip_literal() {
        // Pig::skipLiteral
        let mut p2 = Pig::new("onetwo");
        assert!(
            !p2.skip_literal("two"),
            "skipLiteral=two 'onetwo' --> false"
        );
        assert!(p2.skip_literal("one"), "skipLiteral=one 'onetwo' --> true");
        assert!(dump_has(&p2, " 3/6"), "dump: {}", p2.dump());
    }

    #[test]
    fn skip_partial() {
        // Pig::skipPartial
        let mut p2a = Pig::new("wonderfully");
        let mut value = String::new();
        assert!(
            !p2a.skip_partial("foo", &mut value, false),
            "skipPartial=wonderfully 'foo' --> false"
        );
        assert!(
            p2a.skip_partial("wonder", &mut value, false),
            "skipPartial=wonderfully 'wonder' --> true"
        );
        assert_eq!(
            value, "wonder",
            "skipPartial=wonderfully 'wonder' --> 'wonder'"
        );

        value = String::new();
        assert!(
            p2a.skip_partial("fun", &mut value, false),
            "skipPartial=fully 'fun' --> true"
        );
        assert_eq!(value, "fu", "skipPartial=fully 'fun' --> 'fu'");
    }

    #[test]
    fn get_until_ws() {
        let mut value = String::new();

        // Pig::getUntilWS
        let mut p3 = Pig::new("one two three  ");
        assert!(
            p3.get_until_ws(&mut value),
            "getUntilWS 'one two three  ' --> true"
        );
        assert_eq!(value, "one", "getUntilWS 'one two three  ' --> 'one'");
        assert!(dump_has(&p3, " 3/15"), "dump: {}", p3.dump());

        assert!(p3.skip_ws(), "skipWS ' two three  ' --> true");

        assert!(
            p3.get_until_ws(&mut value),
            "getUntilWS 'two three  ' --> true"
        );
        assert_eq!(value, "two", "getUntilWS 'two three  ' --> 'two'");
        assert!(dump_has(&p3, " 7/15"), "dump: {}", p3.dump());

        assert!(p3.skip_ws(), "skipWS ' three  ' --> true");

        assert!(p3.get_until_ws(&mut value), "getUntilWS 'three  ' --> true");
        assert_eq!(value, "three", "getUntilWS 'three  ' --> 'three'");
        assert!(dump_has(&p3, " 13/15"), "dump: {}", p3.dump());

        let mut p3a = Pig::new("9th ");
        assert!(p3a.get_until_ws(&mut value), "getUntilWS '9th ' --> true");
        assert_eq!(value, "9th", "getUntilWS '9th ' --> '9th'");
        assert!(dump_has(&p3a, " 3/4"), "dump: {}", p3a.dump());
    }

    #[test]
    fn get_digit_and_digits() {
        // Pig::getDigit
        let mut p4 = Pig::new(" 123");
        let mut n = 0;
        assert!(!p4.get_digit(&mut n), "getDigit ' 123' --> false");
        assert!(p4.skip_ws(), "skipWS ' 123' --> true");
        assert!(dump_has(&p4, " 1/4"), "dump: {}", p4.dump());
        assert!(p4.get_digit(&mut n), "getDigit '123' --> true");
        assert_eq!(n, 1, "getDigit '123' --> '1'");
        assert!(dump_has(&p4, " 2/4"), "dump: {}", p4.dump());

        // Pig::getDigits
        let mut p5 = Pig::new("123 ");
        assert!(p5.get_digits(&mut n), "getDigits '123 ' --> true");
        assert_eq!(n, 123, "getDigits '123 ' --> 123");
        assert!(dump_has(&p5, " 3/4"), "dump: {}", p5.dump());

        let mut p6 = Pig::new("1");
        assert!(!p6.eos(), "eos '1' --> false");
        assert!(p6.get_digit(&mut n), "getDigit '1' --> true");
        assert!(!p6.get_digit(&mut n), "getDigit '' --> false");
        assert!(p6.eos(), "eos '' --> true");
        assert!(dump_has(&p6, " 1/1"), "dump: {}", p6.dump());
    }

    #[test]
    fn get_number() {
        let mut value = String::new();

        // Pig::getNumber
        let mut p7 = Pig::new("1 ");
        assert!(p7.get_number(&mut value), "getNumber '1 ' --> true");
        assert_eq!(value, "1", "getNumber '1 ' --> '1'");
        assert!(dump_has(&p7, " 1/2"), "dump: {}", p7.dump());

        let mut p8 = Pig::new("3.14");
        assert!(p8.get_number(&mut value), "getNumber '3.14' --> true");
        assert_eq!(value, "3.14", "getNumber '3.14' --> '3.14'");
        assert!(dump_has(&p8, " 4/4"), "dump: {}", p8.dump());

        let mut p9 = Pig::new("1.23e-4 ");
        assert!(p9.get_number(&mut value), "getNumber '1.23e-4 ' --> true");
        assert_eq!(value, "1.23e-4", "getNumber '1.23e-4 ' --> '1.23e-4'");
        assert!(dump_has(&p9, " 7/8"), "dump: {}", p9.dump());

        let mut p10 = Pig::new("2.34e-5");
        let mut dvalue = 0.0;
        assert!(
            p10.get_number_f64(&mut dvalue),
            "getNumber '2.34e-5' --> true"
        );
        assert!(
            (dvalue - 2.34e-5).abs() <= 1e-6,
            "getNumber '2.34e-5' --> 2.34e-5 +/- 1e-6"
        );
        assert!(dump_has(&p10, " 7/7"), "dump: {}", p10.dump());
    }

    #[test]
    fn get_remainder() {
        let mut value = String::new();

        // Pig::getRemainder
        let mut p11 = Pig::new("123");
        assert!(p11.skip_n(1), "skipN=1 '123' --> true");
        assert!(dump_has(&p11, " 1/3"), "dump: {}", p11.dump());

        assert!(p11.get_remainder(&mut value), "getRemainder '23' --> true");
        assert_eq!(value, "23", "getRemainder '23' --> '23'");
        assert!(dump_has(&p11, " 3/3"), "dump: {}", p11.dump());

        assert!(!p11.get_remainder(&mut value), "getRemainder '' --> false");
        assert!(dump_has(&p11, " 3/3"), "dump: {}", p11.dump());
    }

    #[test]
    fn peek() {
        // Pig::peek
        let p12 = Pig::new("123");
        assert_eq!(p12.peek(), '1' as i32, "peek '123' --> '1'");
        assert!(dump_has(&p12, " 0/3"), "dump: {}", p12.dump());
        assert_eq!(p12.peek_n(2), "12", "peek=2 '123' --> '12'");
        assert_eq!(p12.peek_n(3), "123", "peek=3 '123' --> '123'");
        assert_eq!(p12.peek_n(4), "123", "peek=4 '123' --> '123'");
        assert_eq!(p12.peek_n(20), "123", "peek=20 '123' --> '123'");
        assert!(dump_has(&p12, " 0/3"), "dump: {}", p12.dump());
    }

    #[test]
    fn save_and_restore() {
        // Pig::save, Pig::restore
        let mut p13 = Pig::new("123");
        assert_eq!(p13.save() as i32, 0, "save '123' --> 0");
        assert!(p13.skip_n(2), "skipN=2 '123' --> true");
        assert!(dump_has(&p13, " 2/3"), "dump: {}", p13.dump());
        assert_eq!(p13.cursor() as i32, 2, "cursor '123' --> 2");
        assert_eq!(p13.restore() as i32, 0, "restore '123' --> 0");
        assert!(dump_has(&p13, " 0/3"), "dump: {}", p13.dump());
    }

    #[test]
    fn get_one_of() {
        let mut value = String::new();

        // Pig::getOneOf
        let mut p14 = Pig::new("fourteenfour five");
        assert!(
            p14.get_one_of(&["fourteen", "four"], &mut value),
            "getOneOf={{fourteen,four}} 'fourteenfour five' --> true"
        );
        assert_eq!(
            value, "fourteen",
            "getOneOf={{fourteen,four}} 'fourteenfour five' --> 'fourteen'"
        );
        assert!(
            p14.get_one_of(&["fourteen", "four"], &mut value),
            "getOneOf={{fourteen,four}} 'fourteenfour five' --> true"
        );
        assert_eq!(
            value, "four",
            "getOneOf={{fourteen,four}} 'fourteenfour five' --> 'four'"
        );
        assert!(dump_has(&p14, " 12/17"), "dump: {}", p14.dump());
        assert!(
            !p14.get_one_of(&["five"], &mut value),
            "getOneOf={{five}} 'fourteenfour five' --> false"
        );
        assert!(dump_has(&p14, " 12/17"), "dump: {}", p14.dump());
    }

    #[test]
    fn get_hex_digit() {
        let mut n = 0;

        // Pig::getHexDigit
        let mut p15 = Pig::new(" 9aF");
        assert!(!p15.get_hex_digit(&mut n), "getHexDigit ' 9aF' --> false");
        assert!(p15.skip_ws(), "skipWS ' 9aF' --> true");
        assert!(dump_has(&p15, " 1/4"), "dump: {}", p15.dump());
        assert!(p15.get_hex_digit(&mut n), "getHexDigit '9aF' --> true");
        assert_eq!(n, 9, "getHexDigit '9aF' --> '9'");
        assert!(dump_has(&p15, " 2/4"), "dump: {}", p15.dump());

        assert!(p15.get_hex_digit(&mut n), "getHexDigit '9aF' --> true");
        assert_eq!(n, 10, "getHexDigit '9aF' --> '10'");
        assert!(dump_has(&p15, " 3/4"), "dump: {}", p15.dump());

        assert!(p15.get_hex_digit(&mut n), "getHexDigit '9aF' --> true");
        assert_eq!(n, 15, "getHexDigit '9aF' --> '15'");
        assert!(dump_has(&p15, " 4/4"), "dump: {}", p15.dump());
    }

    #[test]
    fn get_quoted() {
        let mut value = String::new();

        // Pig::getQuoted
        let mut p16 = Pig::new("");
        assert!(
            !p16.get_quoted('"', &mut value),
            "      \"\"   :      getQuoted ('\"')     --> false"
        );

        let mut p17 = Pig::new("''");
        assert!(
            p17.get_quoted('\'', &mut value),
            "      \"''\" :      getQuoted ('\\'')   --> true"
        );
        assert_eq!(value, "", "      \"''\" :      getQuoted ('\\'')   --> ''");

        let mut p18 = Pig::new("'\"'");
        assert!(
            p18.get_quoted('\'', &mut value),
            "     \"'\"'\" :      getQuoted ('\\'')   --> true"
        );
        assert_eq!(
            value, "\"",
            "     \"'\"'\" :      getQuoted ('\\'')   --> '\"'"
        );

        let mut p19 = Pig::new("'x'");
        assert!(
            p19.get_quoted('\'', &mut value),
            "     \"'x'\" :      getQuoted ('\\'')   --> true"
        );
        assert_eq!(
            value, "x",
            "     \"'x'\" :      getQuoted ('\\'')   --> \"x\""
        );

        let mut p20 = Pig::new("'x");
        assert!(
            !p20.get_quoted('\'', &mut value),
            "      \"'x\" :      getQuoted ('\\'')   --> false"
        );

        let mut p21 = Pig::new("x");
        assert!(
            !p21.get_quoted('\'', &mut value),
            "       'x' :      getQuoted ('\\'')   --> false"
        );

        let mut p22 = Pig::new("\"one\\\"two\"");
        assert!(
            !p22.get_quoted('\'', &mut value),
            " \"one\\\"two\" :     getQuoted ('\\'')   --> false"
        );
        assert!(
            p22.get_quoted('"', &mut value),
            " \"one\\\"two\" :     getQuoted ('\"')    --> true"
        );
        assert_eq!(
            value, "one\\\"two",
            " \"one\\\"two\" :     getQuoted ('\"')    --> \"one\\\"two\""
        );

        let mut p23 = Pig::new("\"one\\\\\"");
        assert!(
            p23.get_quoted('"', &mut value),
            "  \"one\\\\\"\" :     getQuoted ('\"')    --> true"
        );
        assert_eq!(
            value, "one\\\\",
            "  \"one\\\\\"\" :     getQuoted ('\"')    --> \"one\\\\\""
        );
    }

    #[test]
    fn get_digit_n() {
        let mut n = 0;

        // Pig::getDigit1
        // Pig::getDigit2
        // Pig::getDigit3
        // Pig::getDigit4
        // Pig::getDigits
        let mut p24 = Pig::new("122333444455555555");
        assert!(
            p24.get_digit(&mut n),
            "getDigit   '122333444455555555' --> true"
        );
        assert_eq!(n, 1, "getDigit   '122333444455555555' --> 1");
        assert!(dump_has(&p24, " 1/18"), "dump: {}", p24.dump());

        assert!(
            p24.get_digit2(&mut n),
            "getDigit2   '22333444455555555' --> true"
        );
        assert_eq!(n, 22, "getDigit2   '22333444455555555' --> 22");
        assert!(dump_has(&p24, " 3/18"), "dump: {}", p24.dump());

        assert!(
            p24.get_digit3(&mut n),
            "getDigit3   '22333444455555555' --> true"
        );
        assert_eq!(n, 333, "getDigit3   '22333444455555555' --> 333");
        assert!(dump_has(&p24, " 6/18"), "dump: {}", p24.dump());

        assert!(
            p24.get_digit4(&mut n),
            "getDigit4   '22333444455555555' --> true"
        );
        assert_eq!(n, 4444, "getDigit4   '22333444455555555' --> 4444");
        assert!(dump_has(&p24, " 10/18"), "dump: {}", p24.dump());

        assert!(
            p24.get_digits(&mut n),
            "getDigits   '22333444455555555' --> true"
        );
        assert_eq!(n, 55555555, "getDigits   '22333444455555555' --> 55555555");
        assert!(dump_has(&p24, " 18/18"), "dump: {}", p24.dump());

        assert!(p24.eos(), "eos --> true");
    }

    #[test]
    fn get_until() {
        let mut value = String::new();

        // Pig::getUntil
        let mut p25 = Pig::new("one two three");
        assert!(
            p25.get_until(' ', &mut value),
            "getUntil ' ', 'one two three' --> true"
        );
        assert_eq!(value, "one", "getUntil ' ', 'one two three' --> 'one'");
        assert!(dump_has(&p25, " 3/13"), "dump: {}", p25.dump());

        assert!(p25.skip_ws(), "skipWS ' two three' --> true");

        assert!(
            p25.get_until(' ', &mut value),
            "getUntil ' ', 'two three' --> true"
        );
        assert_eq!(value, "two", "getUntil ' ', 'two three' --> 'two'");
        assert!(dump_has(&p25, " 7/13"), "dump: {}", p25.dump());

        assert!(p25.skip_ws(), "skipWS ' three' --> true");

        assert!(
            p25.get_until(' ', &mut value),
            "getUntil ' ', 'three' --> true"
        );
        assert_eq!(value, "three", "getUntil ' ', 'three' --> 'three'");
        assert!(dump_has(&p25, " 13/13"), "dump: {}", p25.dump());

        let mut p25a = Pig::new("[foo]");
        assert!(p25a.skip('['), "Pig::skip [, [foo] --> foo] --> true");
        assert!(
            p25a.get_until(']', &mut value),
            "Pig::getUntil ], foo] --> true"
        );
        assert_eq!(value, "foo", "Pig::getUntil ], foo] --> foo");
        assert!(p25a.skip(']'), "Pig::skip ], ] --> true");
        assert!(p25a.eos(), "Pig::eos --> true");
    }

    #[test]
    fn str() {
        // Pig::str
        let mut p26 = Pig::new("abc");
        assert_eq!(p26.str(), "abc", "str 'abc'+0 --> 'abc'");
        assert!(p26.skip('a'), "skip 'a' in 'abc'");
        assert_eq!(p26.str(), "bc", "str 'abc'+1 --> 'bc'");
    }

    #[test]
    fn get_decimal() {
        let mut value = String::new();

        // Pig::getDecimal
        let mut p27 = Pig::new("1 ");
        assert!(p27.get_decimal(&mut value), "getDecimal '1 ' --> true");
        assert_eq!(value, "1", "getDecimal '1 ' --> '1'");
        assert!(dump_has(&p27, " 1/2"), "dump: {}", p27.dump());

        let mut p28 = Pig::new("3.14");
        assert!(p28.get_decimal(&mut value), "getDecimal '3.14' --> true");
        assert_eq!(value, "3.14", "getDecimal '3.14' --> '3.14'");
        assert!(dump_has(&p28, " 4/4"), "dump: {}", p28.dump());
    }

    #[test]
    fn get_hms() {
        let (mut h, mut m, mut s) = (0, 0, 0);

        let mut p29 = Pig::new("1:23:45");
        assert!(
            p29.get_hms(&mut h, &mut m, &mut s),
            "getHMS '1:23:45' --> true"
        );
        assert_eq!(h, 1, "getHMS '1:23:45' h --> 1");
        assert_eq!(m, 23, "getHMS '1:23:45' m --> 23");
        assert_eq!(s, 45, "getHMS '1:23:45' s --> 45");

        let mut p30 = Pig::new("12:34:56");
        assert!(
            p30.get_hms(&mut h, &mut m, &mut s),
            "getHMS '12:34:56' --> true"
        );
        assert_eq!(h, 12, "getHMS '12:34:56' h --> 12");
        assert_eq!(m, 34, "getHMS '12:34:56' m --> 34");
        assert_eq!(s, 56, "getHMS '12:34:56' s --> 56");
    }

    #[test]
    fn substr() {
        // Pig::substr
        let p31 = Pig::new("abc");
        assert_eq!(p31.substr(0, 0), "", "substr 0,0 --> ''");
        assert_eq!(p31.substr(0, 1), "a", "substr 0,1 --> 'a'");
        assert_eq!(p31.substr(0, 2), "ab", "substr 0,2 --> 'ab'");
        assert_eq!(p31.substr(0, 3), "abc", "substr 0,3 --> 'abc'");
        assert_eq!(p31.substr(1, 1), "", "substr 1,1 --> ''");
        assert_eq!(p31.substr(1, 2), "b", "substr 1,2 --> 'b'");
        assert_eq!(p31.substr(1, 3), "bc", "substr 1,3 --> 'bc'");
        assert_eq!(p31.substr(2, 2), "", "substr 2,2 --> ''");
        assert_eq!(p31.substr(2, 3), "c", "substr 2,3 --> 'c'");
    }

    #[test]
    fn get_character() {
        let mut n = 0;

        // Pig.getCharacter
        let mut p32 = Pig::new("abc");
        assert!(p32.get_character(&mut n), "getcharacter 'abc' --> true");
        assert_eq!(n, 97, "getcharacter 'abc ' --> 'a'");
        assert!(dump_has(&p32, " 1/3"), "dump: {}", p32.dump());
        assert!(p32.get_character(&mut n), "getcharacter 'abc' --> true");
        assert_eq!(n, 98, "getcharacter 'abc ' --> 'b'");
        assert!(dump_has(&p32, " 2/3"), "dump: {}", p32.dump());
        assert!(p32.get_character(&mut n), "getcharacter 'abc' --> true");
        assert_eq!(n, 99, "getcharacter 'abc ' --> 'c'");
        assert!(dump_has(&p32, " 3/3"), "dump: {}", p32.dump());
        assert!(p32.eos(), "eos --> true");
    }

    // ---------------------------------------------------------------------
    // Rust-only tests: C++ behaviour that pig.t.cpp does not cover.

    #[test]
    fn dump_format() {
        let mut p = Pig::new("ab\ncd");
        assert_eq!(
            p.dump(),
            "\x1b[37;41mab\\ncd\x1b[0m 0/5",
            "dump at 0 has no matched part"
        );
        assert!(p.skip_n(3));
        assert_eq!(
            p.dump(),
            "\x1b[37;42mab\\n\x1b[0m\x1b[37;41mcd\x1b[0m 3/5",
            "dump shows newlines as \\n"
        );
    }

    #[test]
    fn utf8_cursor_is_a_byte_offset() {
        let mut p = Pig::new("\u{e9}t\u{e9} x");
        assert!(p.skip_n(2), "skipN=2 'été x' --> true");
        assert_eq!(
            p.cursor(),
            3,
            "skipN counts characters, cursor counts bytes"
        );
        let mut value = String::new();
        p.restore_to(0);
        assert!(p.get_until_ws(&mut value));
        assert_eq!(value, "\u{e9}t\u{e9}");
        assert_eq!(p.cursor(), 5);
        assert!(p.skip_ws(), "skipWS over ' '");
        let mut p = Pig::new("\u{3000}x");
        assert!(p.skip_ws(), "skipWS over U+3000 IDEOGRAPHIC SPACE");
        assert_eq!(p.cursor(), 3);
        let mut p = Pig::new("a\u{e9}b");
        assert!(p.get_until('b', &mut value));
        assert_eq!(value, "a\u{e9}");
        assert_eq!(p.cursor(), 3);
        assert!(
            !Pig::new("\u{e9}").skip('\u{e9}'),
            "skip compares one byte, so never matches non-ASCII"
        );
        let mut c = 0;
        assert!(Pig::new("\u{e9}").get_character(&mut c));
        assert_eq!(
            c, 0xC3u8 as i8 as i32,
            "getCharacter yields the signed char"
        );
        assert_eq!(
            Pig::new("\u{e9}").peek(),
            0xC3u8 as i8 as i32,
            "peek yields the signed char"
        );
    }

    #[test]
    fn get_until_edges() {
        let mut value = String::from("unchanged");
        assert!(
            !Pig::new("").get_until('x', &mut value),
            "getUntil '' --> false"
        );
        assert_eq!(value, "unchanged");
        let mut p = Pig::new("xab");
        assert!(
            p.get_until('x', &mut value),
            "getUntil at the end char --> true, empty"
        );
        assert_eq!(value, "");
        assert_eq!(p.cursor(), 0);
    }

    #[test]
    fn get_until_ascii() {
        let mut value = String::new();
        let mut p = Pig::new("ab,cd,");
        assert!(p.get_until_ascii(b',', &mut value));
        assert_eq!(value, "ab");
        assert_eq!(p.cursor(), 2);
        // The search starts one past the cursor, so a leading end char is kept.
        assert!(p.get_until_ascii(b',', &mut value));
        assert_eq!(value, ",cd");
        assert_eq!(p.cursor(), 5);
        let mut p = Pig::new("abc");
        assert!(
            p.get_until_ascii(b',', &mut value),
            "no end char --> rest of text"
        );
        assert_eq!(value, "abc");
        assert!(p.eos());
        assert!(
            p.get_until_ascii(b',', &mut value),
            "at eos --> true, empty"
        );
        assert_eq!(value, "");
    }

    #[test]
    fn skip_partial_ignore_case() {
        let mut value = String::new();
        let mut p = Pig::new("MONday");
        assert!(p.skip_partial("monday", &mut value, true));
        assert_eq!(value, "MONday");
        let mut p = Pig::new("Mon");
        assert!(
            !p.skip_partial("MON", &mut value, true),
            "reference is not lower-cased"
        );
        let mut p = Pig::new("Mon");
        assert!(
            !p.skip_partial("mon", &mut value, false),
            "case-sensitive by default"
        );
    }

    #[test]
    fn skip_literal_edges() {
        let mut p = Pig::new("ab");
        assert!(p.skip_literal(""), "empty literal always matches");
        assert!(!p.skip_literal("abc"), "literal longer than the rest");
        assert!(p.skip_literal("ab"));
        assert!(p.skip_literal(""), "empty literal at end matches");
        assert_eq!(p.cursor(), 2);
    }

    #[test]
    fn number_edges() {
        let mut value = String::new();
        let mut d = 0.0;

        let mut p = Pig::new("1e");
        assert!(
            !p.get_number(&mut value),
            "exponent without digits --> false"
        );
        assert_eq!(p.cursor(), 0);
        let mut p = Pig::new("1.e+5x");
        assert!(p.get_number(&mut value));
        assert_eq!(value, "1.e+5");
        let mut p = Pig::new("+5");
        assert!(p.get_number_f64(&mut d));
        assert_eq!(d, 5.0);
        let mut p = Pig::new("-0");
        assert!(p.get_number_f64(&mut d));
        assert!(d == 0.0 && d.is_sign_negative(), "-0 --> -0.0");
        assert!(Pig::new("1e400").get_number_f64(&mut d));
        assert_eq!(d, f64::INFINITY, "overflow --> HUGE_VAL");
        assert!(Pig::new("4.9e-324").get_number_f64(&mut d));
        assert_eq!(d, f64::from_bits(1), "smallest denormal");
        assert!(Pig::new("0.1").get_number_f64(&mut d));
        assert_eq!(d.to_bits(), 0.1f64.to_bits());
        assert!(!Pig::new("-x").get_number(&mut value));
        assert!(
            !Pig::new(".5").get_number(&mut value),
            "leading digit required"
        );

        let mut p = Pig::new("-12.e3");
        assert!(p.get_decimal(&mut value));
        assert_eq!(value, "-12.", "getDecimal takes no exponent");
        let mut p = Pig::new("-2.5");
        assert!(p.get_decimal_f64(&mut d));
        assert_eq!(d, -2.5);
        assert!(!Pig::new("x").get_decimal_f64(&mut d));
    }

    #[test]
    fn digits_edges() {
        let mut n = 0;
        let mut big = 0i64;
        assert!(Pig::new("99999999999999999999").get_digits_i64(&mut big));
        assert_eq!(big, i64::MAX, "strtoimax saturates");
        assert!(Pig::new("4294967297").get_digits(&mut n));
        assert_eq!(n, 1, "long long -> int truncates");
        assert!(!Pig::new("1").get_digit2(&mut n));
        assert!(!Pig::new("12").get_digit3(&mut n));
        assert!(!Pig::new("123").get_digit4(&mut n));
        let mut p = Pig::new("12\u{e9}");
        assert!(p.get_digits(&mut n));
        assert_eq!((n, p.cursor()), (12, 2));
    }

    #[test]
    fn get_quoted_edges() {
        let mut value = String::new();
        let mut p = Pig::new("\"a\\\\\\\"b\" rest");
        assert!(
            p.get_quoted('"', &mut value),
            "three backslashes escape the quote"
        );
        assert_eq!(value, "a\\\\\\\"b");
        assert_eq!(p.str(), " rest");
        let mut p = Pig::new("'abc' x");
        assert!(p.get_quoted('\'', &mut value));
        assert_eq!(p.cursor(), 5);
        let mut p = Pig::new("''x");
        assert!(p.get_quoted('\'', &mut value));
        assert_eq!(p.cursor(), 2);
    }

    #[test]
    fn get_hms_edges() {
        let (mut h, mut m, mut s) = (7, 7, 7);
        let mut p = Pig::new("1:23");
        assert!(p.get_hms(&mut h, &mut m, &mut s));
        assert_eq!((h, m, s), (1, 23, 0));
        let mut p = Pig::new("1:23:4");
        assert!(
            !p.get_hms(&mut h, &mut m, &mut s),
            "':' without two digits --> false"
        );
        assert_eq!(p.cursor(), 5, "cursor is not restored after the second ':'");
        let mut p = Pig::new("123");
        assert!(!p.get_hms(&mut h, &mut m, &mut s));
        assert_eq!(p.cursor(), 0, "cursor restored");
    }

    #[test]
    fn peek_substr_restore_edges() {
        let mut p = Pig::new("abc");
        assert_eq!(p.peek_n(-1), "abc", "negative quantity --> rest of text");
        assert_eq!(p.peek_n(0), "");
        assert_eq!(p.substr(2, 1), "c", "end before start --> rest from start");
        p.restore_to(3);
        assert_eq!(p.peek_n(1), "", "peek at eos --> ''");
        assert_eq!(p.peek(), 0);
        assert_eq!(p.restore_to(1), 1);
        assert_eq!(p.save(), 1);
        p.skip_n(2);
        assert_eq!(p.restore(), 1);
        let clone = p.clone();
        assert_eq!(clone.cursor(), 1, "clones copy the cursor");
        assert!(!Pig::new("abc").skip_n(4), "skipN past end --> false");
        assert!(Pig::new("abc").skip_n(0), "skipN=0 --> true");
    }

    #[test]
    fn embedded_nul_is_eos() {
        let mut value = String::new();
        let mut p = Pig::new("a\0b");
        assert!(p.skip('a'));
        assert!(p.eos(), "embedded NUL reads as end of string");
        assert!(!p.get_remainder(&mut value));
        assert_eq!(p.str(), "\0b", "str still returns the bytes");
    }

    #[test]
    fn non_ascii_split_is_lossy() {
        let mut value = String::new();
        let mut p = Pig::new("\u{e9}");
        assert!(
            p.skip_partial("\u{e8}", &mut value, false),
            "shared lead byte"
        );
        assert_eq!(value, "\u{fffd}");
        assert_eq!(p.cursor(), 1);
    }
}
