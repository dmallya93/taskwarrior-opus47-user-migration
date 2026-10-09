//! Rust translation of `libshared src/Datetime.h` / `src/Datetime.cpp`.
//!
//! [`Datetime`] is a point in time stored as an `i64` epoch (`time_t`), plus
//! the intermediate fields a parse fills in before [`Datetime::resolve`]
//! turns them into the epoch. Every broken-down conversion (`month ()`,
//! `toString`, `mktime` in the constructors, ...) goes through
//! [`crate::shared::localtime`], i.e. the C library, so `TZ` and libfaketime
//! behave as in the C++ binary (milestone 2, Design Decision 1).
//!
//! # Mutable "static" settings (milestone 2, Design Decision 3)
//! C++ `Datetime` has process-wide statics (`weekstart`,
//! `minimumMatchLength`, `isoEnabled`, `standaloneDateEnabled`,
//! `standaloneTimeEnabled`, `timeRelative`) that `Context` assigns at
//! startup. They are ported as **thread-local `Cell`s with a getter/setter
//! pair on the type**:
//!
//! ```text
//! thread_local! { static WEEKSTART: Cell<i32> = const { Cell::new(1) }; }
//! impl Datetime {
//!     pub fn weekstart() -> i32 { WEEKSTART.with(Cell::get) }
//!     pub fn set_weekstart(value: i32) { WEEKSTART.with(|c| c.set(value)) }
//! }
//! ```
//!
//! C++ `Datetime::weekstart` reads become `Datetime::weekstart()` and
//! assignments `Datetime::set_weekstart(..)`; defaults equal the C++
//! initializers. Taskwarrior is single threaded, so a thread-local is
//! equivalent to a C++ static for the program, while every `cargo test`
//! thread starts from the defaults (a test that changes a setting must still
//! restore it). Other ported statics (Lexer, and Variant in milestone 3) follow
//! the same pattern: one `thread_local!` `Cell` (or `RefCell` for non-`Copy`
//! values) per C++ static, accessed only through `name()` / `set_name(..)`
//! associated functions on the owning type.
//!
//! # API mapping
//! * Constructors: `Datetime ()` -> [`Datetime::new`] (also `Default`),
//!   `Datetime (string, format)` -> [`Datetime::from_string`] (returns the
//!   C++ exception text as `TwError::Message`), `Datetime (time_t)` ->
//!   [`Datetime::from_epoch`], `Datetime (y, m, d)` -> [`Datetime::from_ymd`],
//!   `Datetime (y, m, d, h, n, s)` -> [`Datetime::from_ymdhms`].
//! * Overloads get distinct names: `valid` -> `valid_str` / `valid_ymdhms` /
//!   `valid_ymd` / `valid_yd`; static `dayOfWeek` -> `day_of_week_str` /
//!   `day_of_week_ymd` (the instance `dayOfWeek ()` is `day_of_week`).
//! * Operators: `==`, `<`, ... compare `_date` (`PartialEq`/`Ord`);
//!   `+ int64_t`, `- int64_t`, `+=`, `-=` are `Add`/`Sub`/`AddAssign`/
//!   `SubAssign<i64>`; `Datetime - Datetime` is `Sub<Datetime>` returning
//!   the `i64` difference; `++`/`--` (prefix and postfix are identical in
//!   C++) are [`Datetime::inc`] / [`Datetime::dec`].
//! * C++ `assert`s protecting argument order are `debug_assert!`s (C++ release
//!   builds define `NDEBUG`).
//!
//! Translated from GothenburgBitFactory/libshared at commit
//! d899eba6d1a863f78f15e5e82d45d857278e9471.

use std::cell::Cell;
use std::cmp::Ordering;
use std::ops::{Add, AddAssign, Sub, SubAssign};

use crate::error::{Result, TwError};
use crate::shared::format::format;
use crate::shared::localtime::{self, Tm};
use crate::shared::pig::Pig;
use crate::shared::shared::{close_enough, upper_case_first};
use crate::shared::unicode::{unicode_latin_alpha, unicode_latin_digit, unicode_whitespace};
use crate::shared::utf8::mk_wcwidth;

/// C++ `EPOCH_MIN_VALUE`: 1980-01-01T00:00:00Z.
pub const EPOCH_MIN_VALUE: i64 = 315_532_800;
/// C++ `EPOCH_MAX_VALUE`: 9999-12-31, 23:59:59 AoE.
pub const EPOCH_MAX_VALUE: i64 = 253_402_293_599;

/// C++ file-static `dayNames`.
const DAY_NAMES: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

/// C++ file-static `monthNames`.
const MONTH_NAMES: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

thread_local! {
    /// C++ `Datetime::weekstart`: 1 = Monday, per ISO-8601.
    static WEEKSTART: Cell<i32> = const { Cell::new(1) };
    /// C++ `Datetime::minimumMatchLength`.
    static MINIMUM_MATCH_LENGTH: Cell<i32> = const { Cell::new(3) };
    /// C++ `Datetime::isoEnabled`.
    static ISO_ENABLED: Cell<bool> = const { Cell::new(true) };
    /// C++ `Datetime::standaloneDateEnabled`.
    static STANDALONE_DATE_ENABLED: Cell<bool> = const { Cell::new(true) };
    /// C++ `Datetime::standaloneTimeEnabled`.
    static STANDALONE_TIME_ENABLED: Cell<bool> = const { Cell::new(true) };
    /// C++ `Datetime::timeRelative`.
    static TIME_RELATIVE: Cell<bool> = const { Cell::new(true) };
}

/// C++ `class Datetime`.
///
/// The public fields mirror the C++ public members (`_year` ... `_date`) so
/// the parser port and the upstream tests can be copied line for line.
#[derive(Debug, Clone, Copy)]
pub struct Datetime {
    pub _year: i32,
    pub _month: i32,
    pub _week: i32,
    pub _weekday: i32,
    pub _julian: i32,
    pub _day: i32,
    pub _seconds: i32,
    pub _offset: i32,
    pub _utc: bool,
    pub _date: i64,
}

/// `std::setw (width) << std::setfill ('0') << value`: right-aligned,
/// left-padded with '0' to `width` characters (the fill precedes a sign, as
/// iostream's default `right` adjustment does).
fn setw_zero(value: i32, width: usize) -> String {
    let text = value.to_string();
    if text.len() >= width {
        text
    } else {
        format!("{}{}", "0".repeat(width - text.len()), text)
    }
}

// -----------------------------------------------------------------------------
// Static settings (Design Decision 3).
impl Datetime {
    /// C++ `Datetime::weekstart` (0 = Sunday, 1 = Monday; default 1).
    pub fn weekstart() -> i32 {
        WEEKSTART.with(Cell::get)
    }

    /// C++ `Datetime::weekstart = value`.
    pub fn set_weekstart(value: i32) {
        WEEKSTART.with(|c| c.set(value));
    }

    /// C++ `Datetime::minimumMatchLength` (default 3).
    pub fn minimum_match_length() -> i32 {
        MINIMUM_MATCH_LENGTH.with(Cell::get)
    }

    /// C++ `Datetime::minimumMatchLength = value`.
    pub fn set_minimum_match_length(value: i32) {
        MINIMUM_MATCH_LENGTH.with(|c| c.set(value));
    }

    /// C++ `Datetime::isoEnabled` (default true).
    pub fn iso_enabled() -> bool {
        ISO_ENABLED.with(Cell::get)
    }

    /// C++ `Datetime::isoEnabled = value`.
    pub fn set_iso_enabled(value: bool) {
        ISO_ENABLED.with(|c| c.set(value));
    }

    /// C++ `Datetime::standaloneDateEnabled` (default true).
    pub fn standalone_date_enabled() -> bool {
        STANDALONE_DATE_ENABLED.with(Cell::get)
    }

    /// C++ `Datetime::standaloneDateEnabled = value`.
    pub fn set_standalone_date_enabled(value: bool) {
        STANDALONE_DATE_ENABLED.with(|c| c.set(value));
    }

    /// C++ `Datetime::standaloneTimeEnabled` (default true).
    pub fn standalone_time_enabled() -> bool {
        STANDALONE_TIME_ENABLED.with(Cell::get)
    }

    /// C++ `Datetime::standaloneTimeEnabled = value`.
    pub fn set_standalone_time_enabled(value: bool) {
        STANDALONE_TIME_ENABLED.with(|c| c.set(value));
    }

    /// C++ `Datetime::timeRelative` (default true).
    ///
    /// When true,
    /// - HH:MM:SS is assumed to be today if the time > now, otherwise it is
    ///   assumed to be tomorrow.
    /// - day name is converted to date relative to now
    /// - 1st, 2nd, 3rd, ... is converted to date relative to now
    ///
    /// When false,
    /// - HH:MM:SS is always today.
    /// - day name is always converted to date before now
    /// - 1st, 2nd, 3rd, ... is always converted to date before now
    pub fn time_relative() -> bool {
        TIME_RELATIVE.with(Cell::get)
    }

    /// C++ `Datetime::timeRelative = value`.
    pub fn set_time_relative(value: bool) {
        TIME_RELATIVE.with(|c| c.set(value));
    }
}

// -----------------------------------------------------------------------------
// Construction and the top-level parser.
impl Default for Datetime {
    /// C++ `Datetime ()`: now.
    fn default() -> Self {
        Datetime::new()
    }
}

impl Datetime {
    /// The cleared state (C++ `clear ()` applied to a fresh object).
    fn cleared() -> Self {
        let mut dt = Datetime {
            _year: 0,
            _month: 0,
            _week: 0,
            _weekday: 0,
            _julian: 0,
            _day: 0,
            _seconds: 0,
            _offset: 0,
            _utc: false,
            _date: 0,
        };
        dt.clear();
        dt
    }

    /// C++ `Datetime ()`: the current time.
    pub fn new() -> Self {
        let mut dt = Datetime::cleared();
        dt._date = localtime::time_now();
        dt
    }

    /// C++ `Datetime (const std::string& input, const std::string& format)`.
    ///
    /// # Errors
    /// `'{input}' is not a valid date in the '{format}' format.` when `input`
    /// does not parse.
    pub fn from_string(input: &str, format_: &str) -> Result<Self> {
        let mut dt = Datetime::cleared();
        let mut start = 0usize;
        if !dt.parse(input, &mut start, format_) {
            return Err(TwError::Message(format(
                "'{1}' is not a valid date in the '{2}' format.",
                &[&input, &format_],
            )));
        }
        Ok(dt)
    }

    /// C++ `Datetime (const time_t t)`.
    pub fn from_epoch(t: i64) -> Self {
        let mut dt = Datetime::cleared();
        dt._date = t;
        dt
    }

    /// C++ `Datetime (const int y, const int m, const int d)`: local midnight.
    pub fn from_ymd(y: i32, m: i32, d: i32) -> Self {
        // Protect against arguments being passed in the wrong order.
        debug_assert!((1969..=9999).contains(&y));
        debug_assert!((1..=12).contains(&m));
        debug_assert!((1..=31).contains(&d));

        let mut dt = Datetime::cleared();

        // Error if not valid.
        let mut t = Tm {
            tm_isdst: -1, // Requests that mktime determine summer time effect.
            tm_mday: d,
            tm_mon: m - 1,
            tm_year: y - 1900,
            ..Tm::default()
        };

        dt._date = localtime::mktime(&mut t);
        dt
    }

    /// C++ `Datetime (y, m, d, hr, mi, se)`: local time.
    pub fn from_ymdhms(y: i32, m: i32, d: i32, hr: i32, mi: i32, se: i32) -> Self {
        // Protect against arguments being passed in the wrong order.
        debug_assert!((1969..=9999).contains(&y));
        debug_assert!((1..=12).contains(&m));
        debug_assert!((1..=31).contains(&d));
        debug_assert!((0..=24).contains(&hr));
        debug_assert!((0..60).contains(&mi));
        debug_assert!((0..60).contains(&se));

        let mut dt = Datetime::cleared();

        // Error if not valid.
        let mut t = Tm {
            tm_isdst: -1, // Requests that mktime determine summer time effect.
            tm_mday: d,
            tm_mon: m - 1,
            tm_year: y - 1900,
            tm_hour: hr,
            tm_min: mi,
            tm_sec: se,
            ..Tm::default()
        };

        dt._date = localtime::mktime(&mut t);
        dt
    }

    /// C++ `Datetime::parse (input, start, format = "")`.
    ///
    /// Parses `input` from byte offset `start`; on success `start` is moved
    /// past the consumed text and `true` is returned. On failure `start` is
    /// unchanged (the intermediate fields may have been modified, as in C++).
    pub fn parse(&mut self, input: &str, start: &mut usize, format_: &str) -> bool {
        let i = *start;
        let mut pig = Pig::new(input);
        if i != 0 {
            pig.skip_n(i as i32);
        }

        let checkpoint = pig.cursor();

        // Parse epoch first, as it's the most common scenario.
        if self.parse_epoch(&mut pig) {
            // ::validate and ::resolve are not needed in this case.
            *start = pig.cursor();
            return true;
        }

        if self.parse_formatted(&mut pig, format_) {
            // Check the values and determine time_t.
            if self.validate() {
                *start = pig.cursor();
                self.resolve();
                return true;
            }
        }

        // Allow parse_date_time and parse_date_time_ext regardless of
        // Datetime::isoEnabled setting, because these formats are relied upon by
        // the 'import' command, JSON parser and hook system.
        if self.parse_date_time_ext(&mut pig) // Strictest first.
            || self.parse_date_time(&mut pig)
            || (Datetime::iso_enabled()
                && (self.parse_date_ext(&mut pig)
                    || (Datetime::standalone_date_enabled() && self.parse_date(&mut pig))
                    || self.parse_time_utc_ext(&mut pig)
                    || self.parse_time_utc(&mut pig)
                    || self.parse_time_off_ext(&mut pig)
                    || self.parse_time_ext(&mut pig, true)
                    // Time last, as it is the most permissive.
                    || (Datetime::standalone_time_enabled() && self.parse_time(&mut pig, true))
                    || (Datetime::standalone_time_enabled() && self.parse_time_off(&mut pig))))
        {
            // Check the values and determine time_t.
            if self.validate() {
                *start = pig.cursor();
                self.resolve();
                return true;
            }
        }

        pig.restore_to(checkpoint);

        if self.parse_named(&mut pig) {
            // ::validate and ::resolve are not needed in this case.
            *start = pig.cursor();
            return true;
        }

        false
    }

    /// C++ `Datetime::clear ()`.
    fn clear(&mut self) {
        self._year = 0;
        self._month = 0;
        self._week = 0;
        self._weekday = Datetime::weekstart();
        self._julian = 0;
        self._day = 0;
        self._seconds = 0;
        self._offset = 0;
        self._utc = false;
        self._date = 0;
    }
}

/// C++ `pig.str ().substr (0, 3)`: the first three bytes of `text` (fewer at
/// the end). A split UTF-8 sequence becomes U+FFFD, which no day or month
/// name matches, just as the raw bytes would not in C++.
fn substr3(text: &str) -> String {
    let bytes = text.as_bytes();
    String::from_utf8_lossy(&bytes[..bytes.len().min(3)]).into_owned()
}

/// C++ `format[i]` as the `int` terminator of `pig.getUntil`: `'\0'` at
/// `format.length ()` (std::string guarantees the NUL), and `'\0'` for a
/// non-ASCII byte too (a negative `char` in C++ that no code point equals).
/// `Pig::get_until` never matches `'\0'`, so both read to the end.
fn until_char(format: &[u8], i: usize) -> char {
    match format.get(i) {
        Some(&b) if b.is_ascii() => b as char,
        _ => '\0',
    }
}

/// C++ `pig.skip (format[f])` for the literal format byte `b`. ASCII goes
/// through `Pig::skip`; a non-ASCII byte (part of a UTF-8 literal such as
/// `é`) is compared with the input byte as signed `char`s and skipped one
/// byte at a time, as in C++ (`Pig::skip (char)` cannot express it).
fn skip_format_byte(pig: &mut Pig, b: u8) -> bool {
    if b.is_ascii() {
        return pig.skip(b as char);
    }

    if pig.peek() == b as i8 as i32 {
        let next = pig.cursor() + 1;
        pig.restore_to(next);
        return true;
    }

    false
}

// -----------------------------------------------------------------------------
// The grammar productions and named-date initializers (Datetime.cpp lines
// 235-2927), in C++ order. The range checks and nested ifs are written as in
// C++ for line-by-line review.
#[allow(
    clippy::manual_range_contains,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::nonminimal_bool,
    clippy::needless_range_loop
)]
impl Datetime {
    /// C++ `Datetime::parse_formatted` (Datetime.cpp line 235): parses
    /// `pig` according to an `rc.dateformat`-style `format`.
    ///
    /// Byte handling as in C++ (`format[f]` is a `char`):
    /// * A literal format byte is matched against the next input byte; a
    ///   non-ASCII byte (part of a UTF-8 literal) is matched byte by byte,
    ///   via [`skip_format_byte`].
    /// * `A`/`B` read up to `format[f + 1]`, which is `'\0'` past the end of
    ///   the format; [`until_char`] maps that, and a non-ASCII byte (a
    ///   negative `int` in C++, never equal to a code point), to `'\0'`,
    ///   which `Pig::get_until` likewise never matches.
    fn parse_formatted(&mut self, pig: &mut Pig, format: &str) -> bool {
        // Short-circuit on missing format.
        if format.is_empty() {
            return false;
        }

        let checkpoint = pig.cursor();

        let mut month: i32 = -1; // So we can check later.
        let mut day: i32 = -1;
        let mut year: i32 = -1;
        let mut hour: i32 = -1;
        let mut minute: i32 = -1;
        let mut second: i32 = -1;

        // For parsing, unused.
        let mut wday: i32 = -1;
        let mut week: i32 = -1;

        let fmt = format.as_bytes();
        for f in 0..fmt.len() {
            match fmt[f] {
                b'm' => {
                    if pig.get_digit(&mut month) {
                        if month == 0 {
                            pig.get_digit(&mut month);
                        }

                        if month == 1 {
                            if pig.get_digit(&mut month) {
                                month += 10;
                            }
                        }
                    } else {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'M' => {
                    if !pig.get_digit2(&mut month) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'd' => {
                    if pig.get_digit(&mut day) {
                        if day == 0 {
                            pig.get_digit(&mut day);
                        }

                        if day == 1 || day == 2 || day == 3 {
                            let tens = day;
                            if pig.get_digit(&mut day) {
                                day += 10 * tens;
                            }
                        }
                    } else {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'D' => {
                    if !pig.get_digit2(&mut day) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'y' => {
                    if !pig.get_digit2(&mut year) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                    year += 2000;
                }

                b'Y' => {
                    if !pig.get_digit4(&mut year) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'h' => {
                    if pig.get_digit(&mut hour) {
                        if hour == 0 {
                            pig.get_digit(&mut hour);
                        }

                        if hour == 1 || hour == 2 {
                            let tens = hour;
                            if pig.get_digit(&mut hour) {
                                hour += 10 * tens;
                            }
                        }
                    } else {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'H' => {
                    if !pig.get_digit2(&mut hour) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'n' => {
                    if pig.get_digit(&mut minute) {
                        if minute == 0 {
                            pig.get_digit(&mut minute);
                        }

                        if minute < 6 {
                            let tens = minute;
                            if pig.get_digit(&mut minute) {
                                minute += 10 * tens;
                            }
                        }
                    } else {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'N' => {
                    if !pig.get_digit2(&mut minute) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b's' => {
                    if pig.get_digit(&mut second) {
                        if second == 0 {
                            pig.get_digit(&mut second);
                        }

                        if second < 6 {
                            let tens = second;
                            if pig.get_digit(&mut second) {
                                second += 10 * tens;
                            }
                        }
                    } else {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'S' => {
                    if !pig.get_digit2(&mut second) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'v' => {
                    if pig.get_digit(&mut week) {
                        if week == 0 {
                            pig.get_digit(&mut week);
                        }

                        if week < 6 {
                            let tens = week;
                            if pig.get_digit(&mut week) {
                                week += 10 * tens;
                            }
                        }
                    } else {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'V' => {
                    if !pig.get_digit2(&mut week) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                b'a' => {
                    wday = Datetime::day_of_week_str(&substr3(&pig.str()));
                    if wday == -1 {
                        pig.restore_to(checkpoint);
                        return false;
                    }

                    pig.skip_n(3);
                }

                b'A' => {
                    let mut day_name = String::new();
                    if pig.get_until(until_char(fmt, f + 1), &mut day_name) {
                        wday = Datetime::day_of_week_str(&day_name);
                        if wday == -1 {
                            pig.restore_to(checkpoint);
                            return false;
                        }
                    }
                }

                b'b' => {
                    month = Datetime::month_of_year(&substr3(&pig.str()));
                    if month == -1 {
                        pig.restore_to(checkpoint);
                        return false;
                    }

                    pig.skip_n(3);
                }

                b'B' => {
                    let mut month_name = String::new();
                    if pig.get_until(until_char(fmt, f + 1), &mut month_name) {
                        month = Datetime::month_of_year(&month_name);
                        if month == -1 {
                            pig.restore_to(checkpoint);
                            return false;
                        }
                    }
                }

                literal => {
                    if !skip_format_byte(pig, literal) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }
            }
        }

        // `wday` and `week` are parsed but unused, as in C++.
        let _ = (wday, week);

        // It is possible that the format='Y-M-D', and the input is Y-M-DTH:N:SZ, and
        // this should not be considered a match.
        if !pig.eos() && !unicode_whitespace(pig.peek() as u32) {
            pig.restore_to(checkpoint);
            return false;
        }

        // Missing values are filled in from the current date.
        if year == -1 {
            let now = Datetime::new();
            year = now.year();
            if month == -1 {
                month = now.month();
                if day == -1 {
                    day = now.day();
                    if hour == -1 {
                        hour = now.hour();
                        if minute == -1 {
                            minute = now.minute();
                            if second == -1 {
                                second = now.second();
                            }
                        }
                    }
                }
            }
        }

        // Any remaining undefined values are assigned defaults.
        if month == -1 {
            month = 1;
        }
        if day == -1 {
            day = 1;
        }
        if hour == -1 {
            hour = 0;
        }
        if minute == -1 {
            minute = 0;
        }
        if second == -1 {
            second = 0;
        }

        self._year = year;
        self._month = month;
        self._day = day;
        self._seconds = (hour * 3600) + (minute * 60) + second;

        true
    }

    /// C++ `Datetime::parse_named` (Datetime.cpp line 611): the named dates
    /// (`now`, `today`, `sow`, `eoq`, `easter`, `8am`, ...), tried in the C++
    /// order.
    fn parse_named(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        // Experimental handling of date phrases, such as "first monday in march".
        // Note that this requires that phrases are delimited by EOS or WS.
        let mut token = String::new();
        let mut tokens: Vec<String> = Vec::new();
        while pig.get_until_ws(&mut token) {
            tokens.push(token.clone());
            if !pig.skip_ws() {
                break;
            }
        }

        // This group contains "1st monday ..." which must be processed before
        // initialize_ordinal below.
        // (Commented out upstream:)
        // if self.initialize_nth_day_in_month(&tokens) {
        //     return true;
        // }

        // Restoration necessary because of the tokenization.
        pig.restore_to(checkpoint);

        if self.initialize_now(pig)
            || self.initialize_yesterday(pig)
            || self.initialize_today(pig)
            || self.initialize_tomorrow(pig)
            || self.initialize_ordinal(pig)
            || self.initialize_day_name(pig)
            || self.initialize_month_name(pig)
            || self.initialize_later(pig)
            || self.initialize_sopd(pig)
            || self.initialize_sod(pig)
            || self.initialize_sond(pig)
            || self.initialize_eopd(pig)
            || self.initialize_eod(pig)
            || self.initialize_eond(pig)
            || self.initialize_sopw(pig)
            || self.initialize_sow(pig)
            || self.initialize_sonw(pig)
            || self.initialize_eopw(pig)
            || self.initialize_eow(pig)
            || self.initialize_eonw(pig)
            || self.initialize_sopww(pig) // Must appear after sopw
            || self.initialize_sonww(pig) // Must appear after sonw
            || self.initialize_soww(pig) // Must appear after sow
            || self.initialize_eopww(pig) // Must appear after eopw
            || self.initialize_eonww(pig) // Must appear after eonw
            || self.initialize_eoww(pig) // Must appear after eow
            || self.initialize_sopm(pig)
            || self.initialize_som(pig)
            || self.initialize_sonm(pig)
            || self.initialize_eopm(pig)
            || self.initialize_eom(pig)
            || self.initialize_eonm(pig)
            || self.initialize_sopq(pig)
            || self.initialize_soq(pig)
            || self.initialize_sonq(pig)
            || self.initialize_eopq(pig)
            || self.initialize_eoq(pig)
            || self.initialize_eonq(pig)
            || self.initialize_sopy(pig)
            || self.initialize_soy(pig)
            || self.initialize_sony(pig)
            || self.initialize_eopy(pig)
            || self.initialize_eoy(pig)
            || self.initialize_eony(pig)
            || self.initialize_easter(pig)
            || self.initialize_midsommar(pig)
            || self.initialize_midsommarafton(pig)
            || self.initialize_informal_time(pig)
        {
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_epoch` (Datetime.cpp line 697).
    ///
    /// Valid epoch values are unsigned integers after 1980-01-01T00:00:00Z.
    /// This restriction means that '12' will not be identified as an epoch
    /// date.
    fn parse_epoch(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut epoch: i64 = 0;
        if pig.get_digits_i64(&mut epoch)
            && !unicode_latin_alpha(pig.peek() as u32)
            && epoch >= EPOCH_MIN_VALUE
            && epoch < EPOCH_MAX_VALUE
        {
            self._date = epoch;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_date_time_ext` (Datetime.cpp line 719).
    ///
    /// ```text
    /// date_ext 'T' time_utc_ext 'Z'
    /// date_ext 'T' time_off_ext
    /// date_ext 'T' time_ext
    /// ```
    fn parse_date_time_ext(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if self.parse_date_ext(pig)
            && pig.skip('T')
            && (self.parse_time_utc_ext(pig) || self.parse_time_off_ext(pig) || self.parse_time_ext(pig, true))
        {
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_date_ext` (Datetime.cpp line 742).
    ///
    /// ```text
    /// YYYY-MM-DD
    /// YYYY-MM
    /// YYYY-DDD
    /// YYYY-Www-D
    /// YYYY-Www
    /// ```
    fn parse_date_ext(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut year: i32 = 0;
        if self.parse_year(pig, &mut year) && pig.skip('-') {
            let checkpoint_year = pig.cursor();

            let mut month: i32 = 0;
            let mut day: i32 = 0;
            let mut julian: i32 = 0;
            let mut week: i32 = 0;
            let mut weekday: i32 = 0;

            if pig.skip('W') && self.parse_week(pig, &mut week) {
                if !(pig.skip('-') && self.parse_weekday(pig, &mut weekday)) {
                    weekday = Datetime::weekstart();
                }

                if !unicode_latin_digit(pig.peek() as u32) {
                    self._weekday = weekday;
                    self._week = week;
                    self._year = year;
                    return true;
                }
            }

            pig.restore_to(checkpoint_year);

            if self.parse_month(pig, &mut month)
                && pig.skip('-')
                && self.parse_day(pig, &mut day)
                && !unicode_latin_digit(pig.peek() as u32)
            {
                self._year = year;
                self._month = month;
                self._day = day;
                return true;
            }

            pig.restore_to(checkpoint_year);

            if self.parse_julian(pig, &mut julian) && !unicode_latin_digit(pig.peek() as u32) {
                self._year = year;
                self._julian = julian;
                return true;
            }

            pig.restore_to(checkpoint_year);

            if self.parse_month(pig, &mut month)
                && pig.peek() != '-' as i32
                && !unicode_latin_digit(pig.peek() as u32)
            {
                self._year = year;
                self._month = month;
                self._day = 1;
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_off_ext` (Datetime.cpp line 818): `±hh[:mm]`.
    ///
    /// As in C++, `_offset` is assigned before the trailing-digit check, so a
    /// rejected `+01:002` leaves `_offset` modified.
    fn parse_off_ext(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let sign = pig.peek();
        if sign == '+' as i32 || sign == '-' as i32 {
            pig.skip_n(1);

            let mut hour: i32 = 0;
            let mut minute: i32 = 0;

            if self.parse_off_hour(pig, &mut hour) {
                if pig.skip(':') {
                    if !self.parse_off_minute(pig, &mut minute) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                self._offset = (hour * 3600) + (minute * 60);
                if sign == '-' as i32 {
                    self._offset = -self._offset;
                }

                if !unicode_latin_digit(pig.peek() as u32) {
                    return true;
                }
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_time_ext` (Datetime.cpp line 856): `hh:mm[:ss]`.
    fn parse_time_ext(&mut self, pig: &mut Pig, terminated: bool) -> bool {
        let checkpoint = pig.cursor();

        let mut hour: i32 = 0;
        let mut minute: i32 = 0;
        if self.parse_hour(pig, &mut hour) && pig.skip(':') && self.parse_minute(pig, &mut minute) {
            if pig.skip(':') {
                let mut second: i32 = 0;
                if self.parse_second(pig, &mut second)
                    && !unicode_latin_digit(pig.peek() as u32)
                    && (!terminated || (pig.peek() != '-' as i32 && pig.peek() != '+' as i32))
                {
                    self._seconds = (hour * 3600) + (minute * 60) + second;
                    return true;
                }

                pig.restore_to(checkpoint);
                return false;
            }

            let following = pig.peek();
            if !unicode_latin_digit(following as u32)
                && (!terminated || (following != '+' as i32 && following != '-' as i32))
                && following != 'A' as i32
                && following != 'a' as i32
                && following != 'P' as i32
                && following != 'p' as i32
            {
                self._seconds = (hour * 3600) + (minute * 60);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_time_utc_ext` (Datetime.cpp line 900):
    /// `time-ext 'Z'`.
    fn parse_time_utc_ext(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if self.parse_time_ext(pig, false) && pig.skip('Z') {
            if !unicode_latin_digit(pig.peek() as u32) {
                self._utc = true;
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_time_off_ext` (Datetime.cpp line 920):
    /// `time-ext off-ext`.
    fn parse_time_off_ext(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if self.parse_time_ext(pig, false) && self.parse_off_ext(pig) {
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_date_time` (Datetime.cpp line 937).
    ///
    /// ```text
    /// YYYYMMDDTHHMMSSZ
    /// YYYYMMDDTHHMMSS
    /// ```
    fn parse_date_time(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if self.parse_date(pig)
            && pig.skip('T')
            && (self.parse_time_utc(pig) || self.parse_time_off(pig) || self.parse_time(pig, true))
        {
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_date` (Datetime.cpp line 959).
    ///
    /// ```text
    /// YYYYWww
    /// YYYYDDD
    /// YYYYMMDD
    /// YYYYMM
    /// ```
    fn parse_date(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut year: i32 = 0;
        let mut month: i32 = 0;
        let mut julian: i32 = 0;
        let mut week: i32 = 0;
        let mut weekday: i32 = 0;
        let mut day: i32 = 0;
        if self.parse_year(pig, &mut year) {
            let checkpoint_year = pig.cursor();

            if pig.skip('W') && self.parse_week(pig, &mut week) {
                if !(self.parse_weekday(pig, &mut weekday)) {
                    weekday = Datetime::weekstart();
                }

                if !unicode_latin_digit(pig.peek() as u32) {
                    self._year = year;
                    self._week = week;
                    self._weekday = weekday;
                    return true;
                }
            }

            pig.restore_to(checkpoint_year);

            if self.parse_julian(pig, &mut julian) && !unicode_latin_digit(pig.peek() as u32) {
                self._year = year;
                self._julian = julian;
                return true;
            }

            pig.restore_to(checkpoint_year);

            if self.parse_month(pig, &mut month) {
                if self.parse_day(pig, &mut day) {
                    if !unicode_latin_digit(pig.peek() as u32) {
                        self._year = year;
                        self._month = month;
                        self._day = day;
                        return true;
                    }
                } else {
                    if !unicode_latin_digit(pig.peek() as u32) {
                        self._year = year;
                        self._month = month;
                        self._day = 1;
                        return true;
                    }
                }
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_time_utc` (Datetime.cpp line 1031): `<time> Z`.
    ///
    /// As in C++, `_utc` is set before the trailing-digit check.
    fn parse_time_utc(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if self.parse_time(pig, false) && pig.skip('Z') {
            self._utc = true;
            if !unicode_latin_digit(pig.peek() as u32) {
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_time_off` (Datetime.cpp line 1049):
    /// `<time><off>`.
    fn parse_time_off(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if self.parse_time(pig, false) && self.parse_off(pig) {
            let terminator = pig.peek();
            if terminator != '-' as i32 && !unicode_latin_digit(terminator as u32) {
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_time` (Datetime.cpp line 1070).
    ///
    /// ```text
    /// hhmmss
    /// hhmm
    /// ```
    fn parse_time(&mut self, pig: &mut Pig, terminated: bool) -> bool {
        let checkpoint = pig.cursor();

        let mut hour: i32 = 0;
        let mut minute: i32 = 0;
        if self.parse_hour(pig, &mut hour) && self.parse_minute(pig, &mut minute) {
            let mut second: i32 = 0;
            self.parse_second(pig, &mut second);

            let terminator = pig.peek();
            if !terminated
                || (!unicode_latin_digit(terminator as u32) && terminator != '-' as i32 && terminator != '+' as i32)
            {
                self._seconds = (hour * 3600) + (minute * 60) + second;
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_off` (Datetime.cpp line 1098).
    ///
    /// ```text
    /// ±hhmm
    /// ±hh
    /// ```
    fn parse_off(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let sign = pig.peek();
        if sign == '+' as i32 || sign == '-' as i32 {
            pig.skip_n(1);

            let mut hour: i32 = 0;
            if self.parse_off_hour(pig, &mut hour) {
                let mut minute: i32 = 0;
                self.parse_off_minute(pig, &mut minute);

                if !unicode_latin_digit(pig.peek() as u32) {
                    self._offset = (hour * 3600) + (minute * 60);
                    if sign == '-' as i32 {
                        self._offset = -self._offset;
                    }

                    return true;
                }
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_year` (Datetime.cpp line 1129): four digits,
    /// after 1969.
    fn parse_year(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut year: i32 = 0;
        if pig.get_digit4(&mut year) && year > 1969 {
            *value = year;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_month` (Datetime.cpp line 1146): `01`-`12`.
    fn parse_month(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut month: i32 = 0;
        if pig.get_digit2(&mut month) && month > 0 && month <= 12 {
            *value = month;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_week` (Datetime.cpp line 1164): `01`-`53`.
    fn parse_week(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut week: i32 = 0;
        if pig.get_digit2(&mut week) && week > 0 && week <= 53 {
            *value = week;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_julian` (Datetime.cpp line 1182): `001`-`366`.
    fn parse_julian(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut julian: i32 = 0;
        if pig.get_digit3(&mut julian) && julian > 0 && julian <= 366 {
            *value = julian;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_day` (Datetime.cpp line 1200): `01`-`31`.
    fn parse_day(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut day: i32 = 0;
        if pig.get_digit2(&mut day) && day > 0 && day <= 31 {
            *value = day;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_weekday` (Datetime.cpp line 1218): one digit,
    /// 1-7 when [`Datetime::weekstart`] is 1, else 0-6 (from `weekstart`).
    fn parse_weekday(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut weekday: i32 = 0;
        if pig.get_digit(&mut weekday)
            && weekday >= Datetime::weekstart()
            && weekday <= (if Datetime::weekstart() == 1 { 7 } else { 6 })
        {
            *value = weekday;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_hour` (Datetime.cpp line 1236): `00`-`23`.
    fn parse_hour(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut hour: i32 = 0;
        if pig.get_digit2(&mut hour) && hour >= 0 && hour < 24 {
            *value = hour;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_minute` (Datetime.cpp line 1254): `00`-`59`.
    fn parse_minute(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut minute: i32 = 0;
        if pig.get_digit2(&mut minute) && minute >= 0 && minute < 60 {
            *value = minute;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_second` (Datetime.cpp line 1272): `00`-`59`.
    fn parse_second(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut second: i32 = 0;
        if pig.get_digit2(&mut second) && second >= 0 && second < 60 {
            *value = second;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_off_hour` (Datetime.cpp line 1290): `00`-`12`.
    fn parse_off_hour(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut hour: i32 = 0;
        if pig.get_digit2(&mut hour) && hour >= 0 && hour <= 12 {
            *value = hour;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::parse_off_minute` (Datetime.cpp line 1308): `00`-`59`.
    fn parse_off_minute(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        let checkpoint = pig.cursor();

        let mut minute: i32 = 0;
        if pig.get_digit2(&mut minute) && minute >= 0 && minute < 60 {
            *value = minute;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeNow` (Datetime.cpp line 1327):
    /// `now [ !<alpha> && !<digit> ]`.
    fn initialize_now(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("now") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                self._date = localtime::time_now();
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeYesterday` (Datetime.cpp line 1348):
    /// `yesterday/abbrev [ !<alpha> && !<digit> ]`.
    ///
    /// `token.length () >= static_cast <size_t> (minimumMatchLength)`: the
    /// `as usize` cast wraps a negative setting exactly like the C++ cast.
    fn initialize_yesterday(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut token = String::new();
        if pig.skip_partial("yesterday", &mut token, false)
            && token.len() >= Datetime::minimum_match_length() as usize
        {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);

                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_isdst = -1;
                t.tm_mday -= 1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeToday` (Datetime.cpp line 1377):
    /// `today/abbrev [ !<alpha> && !<digit> ]`.
    fn initialize_today(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut token = String::new();
        if pig.skip_partial("today", &mut token, false) && token.len() >= Datetime::minimum_match_length() as usize {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);

                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);

                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeTomorrow` (Datetime.cpp line 1406):
    /// `tomorrow/abbrev [ !<alpha> && !<digit> ]`.
    fn initialize_tomorrow(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut token = String::new();
        if pig.skip_partial("tomorrow", &mut token, false)
            && token.len() >= Datetime::minimum_match_length() as usize
        {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);

                t.tm_mday += 1;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeOrdinal` (Datetime.cpp line 1435):
    /// `<digit>+ [ "st" | "nd" | "rd" | "th" ] [ !<alpha> && !<digit> ]`.
    ///
    /// `number` is the C++ `long long`; `remainder1/2` are `int`. As in C++,
    /// the two suffix characters are consumed before they are checked, and
    /// the cursor is only restored when the whole production fails.
    fn initialize_ordinal(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut number: i64 = 0;
        if pig.get_digits_i64(&mut number) && number > 0 && number <= 31 {
            let mut character1: i32 = 0;
            let mut character2: i32 = 0;
            if pig.get_character(&mut character1)
                && pig.get_character(&mut character2)
                && !unicode_latin_alpha(pig.peek() as u32)
                && !unicode_latin_digit(pig.peek() as u32)
            {
                let remainder1 = (number % 10) as i32;
                let remainder2 = (number % 100) as i32;
                if (remainder2 != 11 && remainder1 == 1 && character1 == 's' as i32 && character2 == 't' as i32)
                    || (remainder2 != 12 && remainder1 == 2 && character1 == 'n' as i32 && character2 == 'd' as i32)
                    || (remainder2 != 13 && remainder1 == 3 && character1 == 'r' as i32 && character2 == 'd' as i32)
                    || ((remainder2 == 11 || remainder2 == 12 || remainder2 == 13 || remainder1 == 0 || remainder1 > 3)
                        && character1 == 't' as i32
                        && character2 == 'h' as i32)
                {
                    let now = localtime::time_now();
                    let mut t = localtime::localtime(now);

                    let mut y = t.tm_year + 1900;
                    let mut m = t.tm_mon + 1;
                    let d = t.tm_mday;

                    if Datetime::time_relative() && (1 <= number && number <= d as i64) {
                        m += 1;
                        if m > 12 {
                            m = 1;
                            y += 1;
                        }
                    } else if !Datetime::time_relative()
                        && ((d as i64) < number && number <= Datetime::days_in_month(y, m) as i64)
                    {
                        m -= 1;
                        if m < 1 {
                            m = 12;
                            y -= 1;
                        }
                    }

                    t.tm_hour = 0;
                    t.tm_min = 0;
                    t.tm_sec = 0;
                    t.tm_mon = m - 1;
                    t.tm_mday = number as i32;
                    t.tm_year = y - 1900;
                    t.tm_isdst = -1;

                    self._date = localtime::mktime(&mut t);

                    return true;
                }
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeDayName` (Datetime.cpp line 1505):
    /// `sunday/abbrev [ !<alpha> && !<digit> && !: && != ]`.
    fn initialize_day_name(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut token = String::new();
        // Deliberate <= so that 'sunday' is either 0 or 7.
        for day in 0..=7 {
            if pig.skip_partial(DAY_NAMES[(day % 7) as usize], &mut token, true)
                && token.len() >= Datetime::minimum_match_length() as usize
            {
                let following = pig.peek();
                if !unicode_latin_alpha(following as u32)
                    && !unicode_latin_digit(following as u32)
                    && following != ':' as i32
                    && following != '=' as i32
                {
                    let now = localtime::time_now();
                    let mut t = localtime::localtime(now);

                    if t.tm_wday >= day {
                        t.tm_mday += day - t.tm_wday + if Datetime::time_relative() { 7 } else { 0 };
                    } else {
                        t.tm_mday += day - t.tm_wday - if Datetime::time_relative() { 0 } else { 7 };
                    }

                    t.tm_hour = 0;
                    t.tm_min = 0;
                    t.tm_sec = 0;
                    t.tm_isdst = -1;
                    self._date = localtime::mktime(&mut t);
                    return true;
                }
            }

            pig.restore_to(checkpoint);
        }

        false
    }

    /// C++ `Datetime::initializeMonthName` (Datetime.cpp line 1548):
    /// `january/abbrev [ !<alpha> && !<digit> && !: && != ]`.
    fn initialize_month_name(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut token = String::new();
        for month in 0..12 {
            if pig.skip_partial(MONTH_NAMES[month as usize], &mut token, true)
                && token.len() >= Datetime::minimum_match_length() as usize
            {
                let following = pig.peek();
                if !unicode_latin_alpha(following as u32)
                    && !unicode_latin_digit(following as u32)
                    && following != ':' as i32
                    && following != '=' as i32
                {
                    let now = localtime::time_now();
                    let mut t = localtime::localtime(now);

                    if t.tm_mon >= month && Datetime::time_relative() {
                        t.tm_year += 1;
                    }

                    t.tm_mon = month;
                    t.tm_mday = 1;
                    t.tm_hour = 0;
                    t.tm_min = 0;
                    t.tm_sec = 0;
                    t.tm_isdst = -1;
                    self._date = localtime::mktime(&mut t);
                    return true;
                }
            }

            pig.restore_to(checkpoint);
        }

        false
    }

    /// C++ `Datetime::initializeLater` (Datetime.cpp line 1590):
    /// `later/abbrev [ !<alpha> && !<digit> ]`,
    /// `someday/abbrev [ !<alpha> && !<digit> ]`.
    ///
    /// As in C++, a too-short `later` prefix is not undone before `someday`
    /// is tried (from the advanced cursor).
    fn initialize_later(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut token = String::new();
        if (pig.skip_partial("later", &mut token, false) && token.len() >= Datetime::minimum_match_length() as usize)
            || (pig.skip_partial("someday", &mut token, false)
                && token.len() >= std::cmp::max(Datetime::minimum_match_length(), 4) as usize)
        {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);

                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_year = 8099; // Year 9999
                t.tm_mon = 11;
                t.tm_mday = 30;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSopd` (Datetime.cpp line 1626):
    /// `sopd [ !<alpha> && !<digit> ]`.
    fn initialize_sopd(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sopd") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_isdst = -1;
                t.tm_mday -= 1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSod` (Datetime.cpp line 1653):
    /// `sod [ !<alpha> && !<digit> ]`.
    fn initialize_sod(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sod") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSond` (Datetime.cpp line 1679):
    /// `sond [ !<alpha> && !<digit> ]`.
    fn initialize_sond(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sond") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mday += 1;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEopd` (Datetime.cpp line 1706):
    /// `eopd [ !<alpha> && !<digit> ]`.
    fn initialize_eopd(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eopd") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEod` (Datetime.cpp line 1733):
    /// `eod [ !<alpha> && !<digit> ]`.
    fn initialize_eod(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eod") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mday += 1;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEond` (Datetime.cpp line 1761):
    /// `eond [ !<alpha> && !<digit> ]`.
    fn initialize_eond(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eond") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mday += 2;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSopw` (Datetime.cpp line 1789):
    /// `sopw [ !<alpha> && !<digit> ]`.
    fn initialize_sopw(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sopw") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;

                let extra = (t.tm_wday + 6) % 7;
                t.tm_mday -= extra;
                t.tm_mday -= 7;

                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSow` (Datetime.cpp line 1819):
    /// `sow [ !<alpha> && !<digit> ]`.
    fn initialize_sow(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sow") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;

                let extra = (t.tm_wday + 6) % 7;
                t.tm_mday -= extra;

                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSonw` (Datetime.cpp line 1848):
    /// `sonw [ !<alpha> && !<digit> ]`.
    fn initialize_sonw(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sonw") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;

                let extra = (t.tm_wday + 6) % 7;
                t.tm_mday -= extra;
                t.tm_mday += 7;

                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEopw` (Datetime.cpp line 1878):
    /// `eopw [ !<alpha> && !<digit> ]`.
    fn initialize_eopw(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eopw") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;

                let extra = (t.tm_wday + 6) % 7;
                t.tm_mday -= extra;

                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEow` (Datetime.cpp line 1908):
    /// `eow [ !<alpha> && !<digit> ]`.
    fn initialize_eow(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eow") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;

                let extra = (t.tm_wday + 6) % 7;
                t.tm_mday -= extra;
                t.tm_mday += 7;

                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEonw` (Datetime.cpp line 1939):
    /// `eonw [ !<alpha> && !<digit> ]`.
    fn initialize_eonw(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eonw") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;

                let extra = (t.tm_wday + 6) % 7;
                t.tm_mday += 14 - extra;

                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSopww` (Datetime.cpp line 1969):
    /// `sopww [ !<alpha> && !<digit> ]`.
    fn initialize_sopww(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sopww") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mday += -6 - t.tm_wday;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSoww` (Datetime.cpp line 1996):
    /// `soww [ !<alpha> && !<digit> ]`.
    fn initialize_soww(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("soww") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mday += 1 - t.tm_wday;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSonww` (Datetime.cpp line 2023):
    /// `sonww [ !<alpha> && !<digit> ]`.
    fn initialize_sonww(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sonww") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mday += 8 - t.tm_wday;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEopww` (Datetime.cpp line 2050):
    /// `eopww [ !<alpha> && !<digit> ]`.
    fn initialize_eopww(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eopww") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mday -= t.tm_wday + 1;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEoww` (Datetime.cpp line 2078):
    /// `eoww [ !<alpha> && !<digit> ]`.
    fn initialize_eoww(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eoww") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mday += 6 - t.tm_wday;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEonww` (Datetime.cpp line 2106):
    /// `eonww [ !<alpha> && !<digit> ]`.
    fn initialize_eonww(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eonww") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mday += 13 - t.tm_wday;
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSopm` (Datetime.cpp line 2134):
    /// `sopm [ !<alpha> && !<digit> ]`.
    fn initialize_sopm(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sopm") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;

                if t.tm_mon == 0 {
                    t.tm_year -= 1;
                    t.tm_mon = 11;
                } else {
                    t.tm_mon -= 1;
                }

                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSom` (Datetime.cpp line 2170):
    /// `som [ !<alpha> && !<digit> ]`.
    fn initialize_som(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("som") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSonm` (Datetime.cpp line 2197):
    /// `sonm [ !<alpha> && !<digit> ]`.
    fn initialize_sonm(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sonm") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;

                t.tm_mon += 1;
                if t.tm_mon > 11 {
                    t.tm_year += 1;
                    t.tm_mon = 0;
                }

                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEopm` (Datetime.cpp line 2232):
    /// `eopm [ !<alpha> && !<digit> ]`.
    fn initialize_eopm(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eopm") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEom` (Datetime.cpp line 2260):
    /// `eom [ !<alpha> && !<digit> ]`.
    fn initialize_eom(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eom") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;

                t.tm_mon += 1;
                if t.tm_mon > 11 {
                    t.tm_year += 1;
                    t.tm_mon = 0;
                }

                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEonm` (Datetime.cpp line 2296):
    /// `eonm [ !<alpha> && !<digit> ]`.
    fn initialize_eonm(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eonm") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_mday = 1;
                t.tm_mon += 2;
                if t.tm_mon > 11 {
                    t.tm_year += 1;
                    t.tm_mon -= 12;
                }

                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSopq` (Datetime.cpp line 2331):
    /// `sopq [ !<alpha> && !<digit> ]`.
    fn initialize_sopq(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sopq") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mon -= t.tm_mon % 3;
                t.tm_mon -= 3;
                if t.tm_mon < 0 {
                    t.tm_mon += 12;
                    t.tm_year -= 1;
                }

                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSoq` (Datetime.cpp line 2366):
    /// `soq [ !<alpha> && !<digit> ]`.
    fn initialize_soq(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("soq") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_mon -= t.tm_mon % 3;
                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSonq` (Datetime.cpp line 2394):
    /// `sonq [ !<alpha> && !<digit> ]`.
    fn initialize_sonq(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sonq") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mon += 3 - (t.tm_mon % 3);
                if t.tm_mon > 11 {
                    t.tm_mon -= 12;
                    t.tm_year += 1;
                }

                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEopq` (Datetime.cpp line 2428):
    /// `eopq [ !<alpha> && !<digit> ]`.
    fn initialize_eopq(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eopq") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_mon -= t.tm_mon % 3;
                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEoq` (Datetime.cpp line 2457):
    /// `eoq [ !<alpha> && !<digit> ]`.
    fn initialize_eoq(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eoq") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_mon += 3 - (t.tm_mon % 3);
                if t.tm_mon > 11 {
                    t.tm_mon -= 12;
                    t.tm_year += 1;
                }

                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEonq` (Datetime.cpp line 2492):
    /// `eonq [ !<alpha> && !<digit> ]`.
    fn initialize_eonq(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eonq") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_mon += 6 - (t.tm_mon % 3);
                if t.tm_mon > 11 {
                    t.tm_mon -= 12;
                    t.tm_year += 1;
                }

                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSopy` (Datetime.cpp line 2527):
    /// `sopy [ !<alpha> && !<digit> ]`.
    fn initialize_sopy(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sopy") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_mon = 0;
                t.tm_mday = 1;
                t.tm_year -= 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSoy` (Datetime.cpp line 2556):
    /// `soy [ !<alpha> && !<digit> ]`.
    fn initialize_soy(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("soy") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_mon = 0;
                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeSony` (Datetime.cpp line 2584):
    /// `sony [ !<alpha> && !<digit> ]`.
    fn initialize_sony(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("sony") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = 0;
                t.tm_mon = 0;
                t.tm_mday = 1;
                t.tm_year += 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEopy` (Datetime.cpp line 2613):
    /// `eopy [ !<alpha> && !<digit> ]`.
    fn initialize_eopy(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eopy") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_mon = 0;
                t.tm_mday = 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEoy` (Datetime.cpp line 2642):
    /// `eoy [ !<alpha> && !<digit> ]`.
    fn initialize_eoy(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eoy") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_mon = 0;
                t.tm_mday = 1;
                t.tm_year += 1;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEony` (Datetime.cpp line 2672):
    /// `eony [ !<alpha> && !<digit> ]`.
    fn initialize_eony(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("eony") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                t.tm_hour = 0;
                t.tm_min = 0;
                t.tm_sec = -1;
                t.tm_mon = 0;
                t.tm_mday = 1;
                t.tm_year += 2;
                t.tm_isdst = -1;
                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeEaster` (Datetime.cpp line 2706):
    /// `easter`, `eastermonday`, `ascension`, `pentecost`, `goodfriday`, each
    /// `[ !<alpha> && !<digit> ]`.
    ///
    /// As in C++, a holiday literal followed by a letter or digit is not
    /// undone inside the loop: the next holiday is tried from the advanced
    /// cursor, and the checkpoint is only restored after the loop.
    fn initialize_easter(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let holidays: [&str; 5] = ["eastermonday", "easter", "ascension", "pentecost", "goodfriday"];
        let offsets: [i32; 5] = [1, 0, 39, 49, -2];

        for holiday in 0..5 {
            if pig.skip_literal(holidays[holiday])
                && !unicode_latin_alpha(pig.peek() as u32)
                && !unicode_latin_digit(pig.peek() as u32)
            {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);

                self.easter(&mut t);
                self._date = localtime::mktime(&mut t);

                // If the result is earlier this year, then recalc for next year.
                if self._date < now {
                    t = localtime::localtime(now);
                    t.tm_year += 1;
                    self.easter(&mut t);
                }

                // Adjust according to holiday-specific offsets.
                t.tm_mday += offsets[holiday];

                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeMidsommar` (Datetime.cpp line 2748):
    /// `midsommar [ !<alpha> && !<digit> ]`.
    fn initialize_midsommar(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("midsommar") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                self.midsommar(&mut t);
                self._date = localtime::mktime(&mut t);

                // If the result is earlier this year, then recalc for next year.
                if self._date < now {
                    t = localtime::localtime(now);
                    t.tm_year += 1;
                    self.midsommar(&mut t);
                }

                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeMidsommarafton` (Datetime.cpp line 2783):
    /// `midsommarafton [ !<alpha> && !<digit> ]`,
    /// `juhannus [ !<alpha> && !<digit> ]`.
    fn initialize_midsommarafton(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip_literal("midsommarafton") || pig.skip_literal("juhannus") {
            let following = pig.peek();
            if !unicode_latin_alpha(following as u32) && !unicode_latin_digit(following as u32) {
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);
                self.midsommarafton(&mut t);
                self._date = localtime::mktime(&mut t);

                // If the result is earlier this year, then recalc for next year.
                if self._date < now {
                    t = localtime::localtime(now);
                    t.tm_year += 1;
                    self.midsommarafton(&mut t);
                }

                self._date = localtime::mktime(&mut t);
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Datetime::initializeInformalTime` (Datetime.cpp line 2826).
    ///
    /// ```text
    /// 8am
    /// 8a
    /// 8:30am
    /// 8:30a
    /// 8:30
    /// 8:30:00
    ///
    /// \d+ [ : \d{2} ] [ am | a | pm | p ] [ !<alpha> && !<digit> && !: && !+ && !- ]
    /// ```
    ///
    /// `seconds` is the C++ `long long`; `calc_seconds` is its `int`
    /// conversion (two's complement wrap, as on the reference platforms).
    fn initialize_informal_time(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut digit: i32 = 0;
        let mut need_designator = true; // Require am/pm.
        let mut have_designator = false; // Provided am/pm.
        if pig.get_digit(&mut digit) {
            let mut hours = digit;
            if pig.get_digit(&mut digit) {
                hours = 10 * hours + digit;
            }

            let mut minutes: i32 = 0;
            let mut seconds: i64 = 0;
            if pig.skip(':') {
                if !pig.get_digit2(&mut minutes) {
                    pig.restore_to(checkpoint);
                    return false;
                }

                if pig.skip(':') {
                    if !pig.get_digits_i64(&mut seconds) {
                        pig.restore_to(checkpoint);
                        return false;
                    }
                }

                need_designator = false;
            }

            if pig.skip_literal("am") || pig.skip_literal("a") {
                have_designator = true;
                if hours == 12 {
                    hours = 0;
                }
            } else if pig.skip_literal("pm") || pig.skip_literal("p") {
                // Note: '12pm is an exception:
                //  12am = 0h
                //  11am = 11h + 12h
                //  12pm = 12h
                //  1pm  = 1h + 12h
                if hours != 12 {
                    hours += 12;
                }

                have_designator = true;
            }

            // Informal time needs to be terminated.
            let following = pig.peek();
            if unicode_latin_alpha(following as u32)
                || unicode_latin_digit(following as u32)
                || following == ':' as i32
                || following == '-' as i32
                || following == '+' as i32
            {
                pig.restore_to(checkpoint);
                return false;
            }

            if have_designator || !need_designator {
                // Midnight today + hours:minutes:seconds.
                let now = localtime::time_now();
                let mut t = localtime::localtime(now);

                let now_seconds = (t.tm_hour * 3600) + (t.tm_min * 60) + t.tm_sec;
                let calc_seconds = ((hours as i64 * 3600) + (minutes as i64 * 60)).wrapping_add(seconds) as i32;

                if Datetime::time_relative() && calc_seconds < now_seconds {
                    t.tm_mday += 1;
                }

                // Basic validation.
                if hours >= 0 && hours < 24 && minutes >= 0 && minutes < 60 && seconds >= 0 && seconds < 60 {
                    t.tm_hour = hours;
                    t.tm_min = minutes;
                    t.tm_sec = seconds as i32;
                    t.tm_isdst = -1;
                    self._date = localtime::mktime(&mut t);

                    return true;
                }
            }
        }

        pig.restore_to(checkpoint);
        false
    }
}


// -----------------------------------------------------------------------------
// Datetime.cpp line 2929 onwards.
// The range checks are written as in C++ for line-by-line review.
#[allow(clippy::manual_range_contains)]
impl Datetime {
    /// C++ `Datetime::easter (struct tm*)`: sets `t` to local midnight on
    /// Easter Sunday of `t`'s year (anonymous Gregorian algorithm).
    fn easter(&self, t: &mut Tm) {
        let y = t.tm_year + 1900;
        let a = y % 19;
        let b = y / 100;
        let c = y % 100;
        let d = b / 4;
        let e = b % 4;
        let f = (b + 8) / 25;
        let g = (b - f + 1) / 3;
        let h = (19 * a + b - d - g + 15) % 30;
        let i = c / 4;
        let k = c % 4;
        let l = (32 + 2 * e + 2 * i - h - k) % 7;
        let m = (a + 11 * h + 22 * l) / 451;
        let month = (h + l - 7 * m + 114) / 31;
        let day = ((h + l - 7 * m + 114) % 31) + 1;

        t.tm_isdst = -1; // Requests that mktime determine summer time effect.
        t.tm_mday = day;
        t.tm_mon = month - 1;
        t.tm_year = y - 1900;
        t.tm_isdst = -1;
        t.tm_hour = 0;
        t.tm_min = 0;
        t.tm_sec = 0;
    }

    /// C++ `Datetime::midsommar (struct tm*)`: midnight, the first Saturday
    /// after June 20th of `t`'s year (`tm_mday` may be left > 20 unnormalized,
    /// as in C++; `mktime` normalizes it).
    fn midsommar(&self, t: &mut Tm) {
        t.tm_mon = 5; // June.
        t.tm_mday = 20; // Saturday after 20th.
        t.tm_hour = 0; // Midnight.
        t.tm_min = 0;
        t.tm_sec = 0;
        t.tm_isdst = -1; // Probably DST, but check.

        let then = localtime::mktime(t); // Obtain the weekday of June 20th.
        let mid = localtime::localtime(then);
        t.tm_mday += 6 - mid.tm_wday; // How many days after 20th.
    }

    /// C++ `Datetime::midsommarafton (struct tm*)`: midnight, the first
    /// Friday after June 19th of `t`'s year.
    fn midsommarafton(&self, t: &mut Tm) {
        t.tm_mon = 5; // June.
        t.tm_mday = 19; // Saturday after 20th.
        t.tm_hour = 0; // Midnight.
        t.tm_min = 0;
        t.tm_sec = 0;
        t.tm_isdst = -1; // Probably DST, but check.

        let then = localtime::mktime(t); // Obtain the weekday of June 19th.
        let mid = localtime::localtime(then);
        t.tm_mday += 5 - mid.tm_wday; // How many days after 19th.
    }

    // Suggested date expressions (C++ comment block, Datetime.cpp 2982-3040):
    //   {ordinal} {day} in|of {month}
    //   last|past|next|this {day}
    //   last|past|next|this {month}
    //   last|past|next|this week
    //   last|past|next|this month
    //   last|past|next|this weekend
    //   last|past|next|this year
    //   {day} last|past|next|this week
    //   {day} [at] {time}
    //   {time} {day}

    /// C++ `Datetime::initializeNthDayInMonth`: experimental
    /// `<ordinal> <weekday> in|of <month>` recognition. Upstream's only call
    /// (in `parse_named`) is commented out, and the body only prints a
    /// diagnostic.
    #[allow(dead_code)] // Unused upstream too (call commented out in parse_named).
    fn initialize_nth_day_in_month(&mut self, tokens: &[String]) -> bool {
        if tokens.len() == 4 {
            let mut ordinal = 0;
            if self.is_ordinal(&tokens[0], &mut ordinal) {
                let day = Datetime::day_of_week_str(&tokens[1]);
                if day != -1 && (tokens[2] == "in" || tokens[2] == "of") {
                    let month = Datetime::month_of_year(&tokens[3]);
                    if month != -1 {
                        println!("# ordinal={ordinal} day={day} in month={month}");

                        // TODO Assume 1st of the month
                        // TODO Assume current year
                        // TODO Determine the day
                        // TODO Project forwards/backwards, to the desired day
                        // TODO Add ((ordinal - 1) * 7) days

                        return true;
                    }
                }
            }
        }

        false
    }

    /// C++ `Datetime::isOrdinal`: `1st`, `2nd`, `3rd`, `11th`, `22nd`, ...
    /// (note the C++ rule accepts any number with `th`).
    #[allow(dead_code)] // Used by initialize_nth_day_in_month.
    fn is_ordinal(&self, token: &str, ordinal: &mut i32) -> bool {
        let mut p = Pig::new(token);
        let mut number: i64 = 0;
        let mut suffix = String::new();
        if p.get_digits_i64(&mut number)
            && p.get_remainder(&mut suffix)
            && (((11..=13).contains(&number) && suffix == "th")
                || (number % 10 == 1 && suffix == "st")
                || (number % 10 == 2 && suffix == "nd")
                || (number % 10 == 3 && suffix == "rd")
                || (suffix == "th"))
        {
            *ordinal = number as i32;
            return true;
        }

        false
    }

    /// C++ `Datetime::validate`: validation via simple range checking.
    fn validate(&self) -> bool {
        // for ISO 8601 week specification: YYYY-Www-D where D is 1-7 (Mon-Sun)
        let wks = if Datetime::weekstart() == 1 { 1 } else { 0 };
        let wke = if Datetime::weekstart() == 1 { 7 } else { 6 };

        #[allow(clippy::nonminimal_bool, clippy::overly_complex_bool_expr)]
        let invalid = (self._year != 0 && (self._year < 1900 || self._year > 9999))
            || (self._month != 0 && (self._month < 1 || self._month > 12))
            || (self._week != 0 && (self._week < 1 || self._week > 53))
            || (self._weekday < wks || self._weekday > wke)
            || (self._julian != 0 && (self._julian < 1 || self._julian > Datetime::days_in_year(self._year)))
            || (self._day != 0
                && (self._day < 1 || self._day > Datetime::days_in_month(self._year, self._month)))
            || (self._seconds != 0 && (self._seconds < 1 || self._seconds > 86400))
            || (self._offset != 0 && (self._offset < -86400 || self._offset > 86400));

        !invalid
    }

    /// C++ `Datetime::timegm (struct tm*)`. The C++ fallback (temporarily
    /// setting `TZ=UTC` around `mktime`) is only compiled when the platform
    /// lacks `timegm` (`!HAVE_TIMEGM`); every libc target this crate builds
    /// for provides it, so this is the `HAVE_TIMEGM` path: the libc call.
    fn timegm(&self, t: &mut Tm) -> i64 {
        localtime::timegm(t)
    }

    /// C++ `Datetime::resolve`: turns the parsed fields into `_date`.
    ///
    /// ```text
    /// int tm_sec;       seconds (0 - 60)
    /// int tm_min;       minutes (0 - 59)
    /// int tm_hour;      hours (0 - 23)
    /// int tm_mday;      day of month (1 - 31)
    /// int tm_mon;       month of year (0 - 11)
    /// int tm_year;      year - 1900
    /// int tm_wday;      day of week (0-6 or 1-7 depending on Datetime::weekstart)
    /// int tm_yday;      day of year (0 - 365)
    /// int tm_isdst;     is daylight saving time in effect?
    /// long tm_gmtoff;   offset from UTC in seconds
    /// ```
    fn resolve(&mut self) {
        // Don't touch the original values.
        let mut year = self._year;
        let mut month = self._month;
        let week = self._week;
        let weekday = self._weekday;
        let mut julian = self._julian;
        let mut day = self._day;
        let mut seconds = self._seconds;
        let offset = self._offset;
        let mut utc = self._utc;

        // Get current time.
        let mut now = localtime::time_now();

        // A UTC offset needs to be accommodated.  Once the offset is subtracted,
        // only local and UTC times remain.
        if offset != 0 {
            seconds -= offset;
            now -= offset as i64;
            utc = true;
        }

        // Get 'now' in the relevant location.
        let t_now = if utc { localtime::gmtime(now) } else { localtime::localtime(now) };

        let seconds_now = (t_now.tm_hour * 3600) + (t_now.tm_min * 60) + t_now.tm_sec;

        // Project forward one day if the specified seconds are earlier in the day
        // than the current seconds. Overridden by the ::timeRelative setting.
        if Datetime::time_relative()
            && year == 0
            && month == 0
            && day == 0
            && week == 0
            && weekday == Datetime::weekstart()
            && seconds < seconds_now
        {
            seconds += 86400;
        }

        // Convert week + weekday --> julian, possibly in the previous or next
        // calendar year (for ISO8601 weeks).  Weekday will be 1-7 if weekstart = 1,
        // as per ISO week spec.  This is incompatible with tm.tm_wday (which is
        // 0-6, Sunday based), but that field is not assigned anywhere in this
        // function, and note that mktime() uses tm_wday only as an output field.
        // We use Datetime::weekstart as a hint about whether the user is
        // giving/wants ISO weeks, although the very use of 2024-W30 and 2024-W30-1
        // format is from ISO8601 in the first place.
        if week != 0 {
            // Get 0-based (Sunday) and 1-based (Monday, ie ISO) dow for 4th Jan
            // 0 Sun, 1 Mon, 2 Tue, 3 Wed, 4 Thu, 5 Fri, 6 Sat
            //        1 Mon, 2 Tue, 3 Wed, 4 Thu, 5 Fri, 6 Sat, 7 Sun
            let jan4dow0 = Datetime::day_of_week_ymd(year, 1, 4);
            let jan4dow1 = if jan4dow0 == 0 { 7 } else { jan4dow0 };

            if Datetime::weekstart() == 1 {
                // https://en.wikipedia.org/wiki/ISO_week_date
                julian = week * 7 + weekday - (jan4dow1 + 3);
                if julian < 1 {
                    year -= 1;
                    julian += Datetime::days_in_year(year);
                } else {
                    let yeardays = Datetime::days_in_year(year);
                    if julian > yeardays {
                        year += 1;
                        julian -= yeardays;
                    }
                }
            } else {
                // Prior algorithm that was used for all weekstarts before, now only
                // used for Sunday weekstarts
                julian = week * 7 + weekday - jan4dow0 - 3;
            }
        }
        // Provide default values for year, month, day.
        else {
            // Default values for year, month, day:
            //
            // y   m   d  -->  y   m   d
            // y   m   -  -->  y   m   1
            // y   -   -  -->  y   1   1
            // -   -   -  -->  now now now
            //
            if year == 0 {
                year = t_now.tm_year + 1900;
                month = t_now.tm_mon + 1;
                day = t_now.tm_mday;
            } else if month == 0 {
                month = 1;
                day = 1;
            } else if day == 0 {
                day = 1;
            }
        }

        if julian != 0 {
            month = 1;
            day = julian;
        }

        let mut t = Tm {
            tm_isdst: -1, // Requests that mktime/gmtime determine summer time effect.
            tm_year: year - 1900,
            tm_mon: month - 1,
            tm_mday: day,
            ..Tm::default()
        };

        if seconds > 86400 {
            let days = seconds / 86400;
            t.tm_mday += days;
            seconds %= 86400;
        }

        t.tm_hour = seconds / 3600;
        t.tm_min = (seconds % 3600) / 60;
        t.tm_sec = seconds % 60;

        self._date = if utc { self.timegm(&mut t) } else { localtime::mktime(&mut t) };
    }

    /// C++ `Datetime::toEpoch`.
    pub fn to_epoch(&self) -> i64 {
        self._date
    }

    /// C++ `Datetime::toEpochString`.
    pub fn to_epoch_string(&self) -> String {
        format("{1}", &[&self._date])
    }

    /// C++ `Datetime::toISO`: `19980119T070000Z` = `YYYYMMDDThhmmssZ` (UTC).
    pub fn to_iso(&self) -> String {
        let t = localtime::gmtime(self._date);

        let mut iso = String::new();
        iso.push_str(&setw_zero(t.tm_year + 1900, 4));
        iso.push_str(&setw_zero(t.tm_mon + 1, 2));
        iso.push_str(&setw_zero(t.tm_mday, 2));
        iso.push('T');
        iso.push_str(&setw_zero(t.tm_hour, 2));
        iso.push_str(&setw_zero(t.tm_min, 2));
        iso.push_str(&setw_zero(t.tm_sec, 2));
        iso.push('Z');
        iso
    }

    /// C++ `Datetime::toISOLocalExtended`: `1998-01-19T07:00:00` =
    /// `YYYY-MM-DDThh:mm:ss` (local time).
    pub fn to_iso_local_extended(&self) -> String {
        let t = localtime::localtime(self._date);

        let mut iso = String::new();
        iso.push_str(&setw_zero(t.tm_year + 1900, 4));
        iso.push('-');
        iso.push_str(&setw_zero(t.tm_mon + 1, 2));
        iso.push('-');
        iso.push_str(&setw_zero(t.tm_mday, 2));
        iso.push('T');
        iso.push_str(&setw_zero(t.tm_hour, 2));
        iso.push(':');
        iso.push_str(&setw_zero(t.tm_min, 2));
        iso.push(':');
        iso.push_str(&setw_zero(t.tm_sec, 2));
        iso
    }

    /// C++ `Datetime::toJulian`.
    pub fn to_julian(&self) -> f64 {
        (self._date as f64 / 86400.0) + 2440587.5
    }

    /// C++ `Datetime::toYMD (int& y, int& m, int& d)` (local time).
    pub fn to_ymd(&self, y: &mut i32, m: &mut i32, d: &mut i32) {
        let t = localtime::localtime(self._date);

        *m = t.tm_mon + 1;
        *d = t.tm_mday;
        *y = t.tm_year + 1900;
    }

    /// C++ `Datetime::toString (format = "Y-M-D")`.
    ///
    /// Each format byte is one of `m M d D y Y a A b B v V h H n N s S j J w`
    /// or is copied through verbatim (multi-byte UTF-8 is copied byte by byte,
    /// so it survives intact).
    ///
    /// # Panics
    /// For `v`/`V` when [`Datetime::weekstart`] is neither 0 nor 1 (see
    /// [`Datetime::week`]).
    pub fn to_string(&self, format_: &str) -> String {
        let mut formatted: Vec<u8> = Vec::with_capacity(format_.len() * 2);
        for &c in format_.as_bytes() {
            let piece = match c {
                b'm' => self.month().to_string(),
                b'M' => setw_zero(self.month(), 2),
                b'd' => self.day().to_string(),
                b'D' => setw_zero(self.day(), 2),
                b'y' => setw_zero(self.year() % 100, 2),
                b'Y' => self.year().to_string(),
                b'a' => Datetime::day_name_short(self.day_of_week()),
                b'A' => Datetime::day_name(self.day_of_week()),
                b'b' => Datetime::month_name_short(self.month()),
                b'B' => Datetime::month_name(self.month()),
                b'v' => self.week().to_string(),
                b'V' => setw_zero(self.week(), 2),
                b'h' => self.hour().to_string(),
                b'H' => setw_zero(self.hour(), 2),
                b'n' => self.minute().to_string(),
                b'N' => setw_zero(self.minute(), 2),
                b's' => self.second().to_string(),
                b'S' => setw_zero(self.second(), 2),
                b'j' => self.day_of_year().to_string(),
                b'J' => setw_zero(self.day_of_year(), 3),
                b'w' => self.day_of_week().to_string(),
                _ => {
                    formatted.push(c);
                    continue;
                }
            };
            formatted.extend_from_slice(piece.as_bytes());
        }

        String::from_utf8(formatted).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
    }

    /// C++ `Datetime::startOfDay`: local midnight of the same day.
    pub fn start_of_day(&self) -> Datetime {
        Datetime::from_ymd(self.year(), self.month(), self.day())
    }

    /// C++ `Datetime::startOfWeek`: local midnight of the preceding (or same)
    /// Sunday.
    pub fn start_of_week(&self) -> Datetime {
        let mut sow = Datetime::from_epoch(self._date);
        sow -= (self.day_of_week() * 86400) as i64;
        Datetime::from_ymd(sow.year(), sow.month(), sow.day())
    }

    /// C++ `Datetime::startOfMonth`.
    pub fn start_of_month(&self) -> Datetime {
        Datetime::from_ymd(self.year(), self.month(), 1)
    }

    /// C++ `Datetime::startOfYear`.
    pub fn start_of_year(&self) -> Datetime {
        Datetime::from_ymd(self.year(), 1, 1)
    }

    /// C++ `Datetime::valid (const std::string&, const std::string& format)`.
    pub fn valid_str(input: &str, format_: &str) -> bool {
        Datetime::from_string(input, format_).is_ok()
    }

    /// C++ `Datetime::valid (y, m, d, hr, mi, se)`.
    pub fn valid_ymdhms(y: i32, m: i32, d: i32, hr: i32, mi: i32, se: i32) -> bool {
        if !(0..=24).contains(&hr) {
            return false;
        }

        if !(0..=59).contains(&mi) {
            return false;
        }

        if !(0..=59).contains(&se) {
            return false;
        }

        if hr == 24 && (mi != 0 || se != 0) {
            return false;
        }

        Datetime::valid_ymd(y, m, d)
    }

    /// C++ `Datetime::valid (y, m, d)`.
    pub fn valid_ymd(y: i32, m: i32, d: i32) -> bool {
        // Check that the year is valid.
        if y < 0 {
            return false;
        }

        // Check that the month is valid.
        if !(1..=12).contains(&m) {
            return false;
        }

        // Finally check that the days fall within the acceptable range for this
        // month, and whether this is a leap year.
        if d < 1 || d > Datetime::days_in_month(y, m) {
            return false;
        }

        true
    }

    /// C++ `Datetime::valid (y, d)`: Julian (day-of-year) date.
    pub fn valid_yd(y: i32, d: i32) -> bool {
        // Check that the year is valid.
        if y < 0 {
            return false;
        }

        if d < 1 || d > Datetime::days_in_year(y) {
            return false;
        }

        true
    }

    /// C++ static `Datetime::leapYear`.
    pub fn leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    /// C++ static `Datetime::daysInMonth`.
    pub fn days_in_month(year: i32, month: i32) -> i32 {
        // Protect against arguments being passed in the wrong order.
        debug_assert!((1969..=9999).contains(&year));
        debug_assert!((1..=31).contains(&month));

        const DAYS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

        if month == 2 && Datetime::leap_year(year) {
            return 29;
        }

        DAYS[(month - 1) as usize]
    }

    /// C++ static `Datetime::daysInYear`.
    pub fn days_in_year(year: i32) -> i32 {
        if Datetime::leap_year(year) {
            366
        } else {
            365
        }
    }

    /// C++ static `Datetime::monthName`: `1` -> `January`.
    pub fn month_name(month: i32) -> String {
        debug_assert!(month > 0);
        debug_assert!(month <= 12);
        upper_case_first(MONTH_NAMES[(month - 1) as usize])
    }

    /// C++ static `Datetime::monthNameShort`: `1` -> `Jan`.
    pub fn month_name_short(month: i32) -> String {
        debug_assert!(month > 0);
        debug_assert!(month <= 12);
        upper_case_first(MONTH_NAMES[(month - 1) as usize])[..3].to_string()
    }

    /// C++ static `Datetime::dayName`: `0` -> `Sunday`.
    pub fn day_name(dow: i32) -> String {
        debug_assert!(dow >= 0);
        debug_assert!(dow <= 6);
        upper_case_first(DAY_NAMES[dow as usize])
    }

    /// C++ static `Datetime::dayNameShort`: `0` -> `Sun`.
    pub fn day_name_short(dow: i32) -> String {
        debug_assert!(dow >= 0);
        debug_assert!(dow <= 6);
        upper_case_first(DAY_NAMES[dow as usize])[..3].to_string()
    }

    /// C++ static `Datetime::dayOfWeek (const std::string&)`: 0-6 for a
    /// (case-insensitive, abbreviable to [`Datetime::minimum_match_length`])
    /// day name, else -1. Resets a zero minimum match length to 3, as in C++.
    pub fn day_of_week_str(input: &str) -> i32 {
        if Datetime::minimum_match_length() == 0 {
            Datetime::set_minimum_match_length(3);
        }

        let min = Datetime::minimum_match_length() as u32;
        for (i, name) in DAY_NAMES.iter().enumerate() {
            if close_enough(name, input, min) {
                return i as i32;
            }
        }

        -1
    }

    /// C++ static `Datetime::dayOfWeek (int year, int month, int day)`: 0-6
    /// (Sunday based), using the variant of Zeller's Congruence from RFC3339
    /// appendix B.
    pub fn day_of_week_ymd(year: i32, month: i32, day: i32) -> i32 {
        let mut year = year;
        let mut month = month;

        // adjust months so February is the last one
        month -= 2;
        if month < 1 {
            month += 12;
            year -= 1;
        }

        // split by century
        let cent = year / 100;
        year %= 100;

        ((26 * month - 2) / 10 + day + year + year / 4 + cent / 4 + 5 * cent) % 7
    }

    /// C++ static `Datetime::monthOfYear`: 1-12 for a (case-insensitive,
    /// abbreviable) month name, else -1. Resets a zero minimum match length
    /// to 3, as in C++.
    pub fn month_of_year(input: &str) -> i32 {
        if Datetime::minimum_match_length() == 0 {
            Datetime::set_minimum_match_length(3);
        }

        let min = Datetime::minimum_match_length() as u32;
        for (i, name) in MONTH_NAMES.iter().enumerate() {
            if close_enough(name, input, min) {
                return i as i32 + 1;
            }
        }

        -1
    }

    /// C++ static `Datetime::length`: the maximum rendered width of a
    /// `toString` format. Other bytes add `mk_wcwidth` of the (signed `char`)
    /// byte, so each byte of a multi-byte UTF-8 character adds -1, as in C++.
    pub fn length(format_: &str) -> i32 {
        let mut len = 0;
        for &i in format_.as_bytes() {
            match i {
                b'm' | b'M' | b'd' | b'D' | b'y' | b'v' | b'V' | b'h' | b'H' | b'n' | b'N' | b's'
                | b'S' => len += 2,
                b'b' | b'j' | b'J' | b'a' => len += 3,
                b'Y' => len += 4,
                b'A' | b'B' => len += 10,

                // Calculate the width, don't assume a single character width.
                _ => len += mk_wcwidth(i as i8 as i32),
            }
        }

        len
    }

    /// C++ `Datetime::month`: 1-12, local time.
    pub fn month(&self) -> i32 {
        localtime::localtime(self._date).tm_mon + 1
    }

    /// C++ `Datetime::week`: `strftime` `%U` (weekstart 0) or `%V` (weekstart
    /// 1) of the local time.
    ///
    /// # Errors
    /// `The week may only start on a Sunday or Monday.` for any other
    /// [`Datetime::weekstart`].
    pub fn try_week(&self) -> Result<i32> {
        let t = localtime::localtime(self._date);

        let week_str = match Datetime::weekstart() {
            0 => localtime::strftime("%U", &t),
            1 => localtime::strftime("%V", &t),
            _ => {
                return Err(TwError::Message("The week may only start on a Sunday or Monday.".to_string()));
            }
        };

        // strtol (weekStr, nullptr, 10)
        Ok(week_str.trim().parse::<i32>().unwrap_or(0))
    }

    /// C++ `Datetime::week`, for callers that (like Taskwarrior's `Context`)
    /// guarantee a valid weekstart.
    ///
    /// # Panics
    /// With the C++ exception text when [`Datetime::weekstart`] is neither 0
    /// nor 1 (the C++ `throw` escaping a non-`Result` call chain); use
    /// [`Datetime::try_week`] to get the error instead.
    pub fn week(&self) -> i32 {
        match self.try_week() {
            Ok(week) => week,
            Err(e) => panic!("{e}"),
        }
    }

    /// C++ `Datetime::day`: day of month, local time.
    pub fn day(&self) -> i32 {
        localtime::localtime(self._date).tm_mday
    }

    /// C++ `Datetime::year`: local time.
    pub fn year(&self) -> i32 {
        localtime::localtime(self._date).tm_year + 1900
    }

    /// C++ `Datetime::dayOfWeek ()`: 0 = Sunday, local time.
    pub fn day_of_week(&self) -> i32 {
        localtime::localtime(self._date).tm_wday
    }

    /// C++ `Datetime::dayOfYear`: 1-366, local time.
    pub fn day_of_year(&self) -> i32 {
        localtime::localtime(self._date).tm_yday + 1
    }

    /// C++ `Datetime::hour`: local time.
    pub fn hour(&self) -> i32 {
        localtime::localtime(self._date).tm_hour
    }

    /// C++ `Datetime::minute`: local time.
    pub fn minute(&self) -> i32 {
        localtime::localtime(self._date).tm_min
    }

    /// C++ `Datetime::second`: local time.
    pub fn second(&self) -> i32 {
        localtime::localtime(self._date).tm_sec
    }

    /// C++ `Datetime::sameHour`.
    pub fn same_hour(&self, rhs: &Datetime) -> bool {
        self.year() == rhs.year()
            && self.month() == rhs.month()
            && self.day() == rhs.day()
            && self.hour() == rhs.hour()
    }

    /// C++ `Datetime::sameDay`.
    pub fn same_day(&self, rhs: &Datetime) -> bool {
        self.year() == rhs.year() && self.month() == rhs.month() && self.day() == rhs.day()
    }

    /// C++ `Datetime::sameWeek`.
    pub fn same_week(&self, rhs: &Datetime) -> bool {
        self.year() == rhs.year() && self.week() == rhs.week()
    }

    /// C++ `Datetime::sameMonth`.
    pub fn same_month(&self, rhs: &Datetime) -> bool {
        self.year() == rhs.year() && self.month() == rhs.month()
    }

    /// C++ `Datetime::sameQuarter`.
    pub fn same_quarter(&self, rhs: &Datetime) -> bool {
        self.year() == rhs.year() && ((self.month() - 1) / 3) == ((rhs.month() - 1) / 3)
    }

    /// C++ `Datetime::sameYear`.
    pub fn same_year(&self, rhs: &Datetime) -> bool {
        self.year() == rhs.year()
    }

    /// C++ `Datetime::operator-- ()` / `operator-- (int)`: the same local
    /// time-of-day on the previous day (DST-safe).
    pub fn dec(&mut self) {
        let mut yesterday = self.start_of_day() - 1;
        yesterday = Datetime::from_ymdhms(
            yesterday.year(),
            yesterday.month(),
            yesterday.day(),
            self.hour(),
            self.minute(),
            self.second(),
        );
        self._date = yesterday._date;
    }

    /// C++ `Datetime::operator++ ()` / `operator++ (int)`: the same local
    /// time-of-day on the next day (DST-safe).
    pub fn inc(&mut self) {
        let mut tomorrow = (self.start_of_day() + 90001).start_of_day();
        tomorrow = Datetime::from_ymdhms(
            tomorrow.year(),
            tomorrow.month(),
            tomorrow.day(),
            self.hour(),
            self.minute(),
            self.second(),
        );
        self._date = tomorrow._date;
    }
}

/// C++ `operator==` / `operator!=`: compares `_date` only.
impl PartialEq for Datetime {
    fn eq(&self, rhs: &Self) -> bool {
        rhs._date == self._date
    }
}

impl Eq for Datetime {}

/// C++ `operator<`, `>`, `<=`, `>=`: compares `_date` only.
impl PartialOrd for Datetime {
    fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
        Some(self.cmp(rhs))
    }
}

impl Ord for Datetime {
    fn cmp(&self, rhs: &Self) -> Ordering {
        self._date.cmp(&rhs._date)
    }
}

/// C++ `Datetime operator+ (const int64_t)`: a new `Datetime (_date + delta)`.
impl Add<i64> for Datetime {
    type Output = Datetime;

    fn add(self, delta: i64) -> Datetime {
        Datetime::from_epoch(self._date.wrapping_add(delta))
    }
}

/// C++ `Datetime operator- (const int64_t)`: a new `Datetime (_date - delta)`.
impl Sub<i64> for Datetime {
    type Output = Datetime;

    fn sub(self, delta: i64) -> Datetime {
        Datetime::from_epoch(self._date.wrapping_sub(delta))
    }
}

/// C++ `Datetime& operator+= (const int64_t)`: only `_date` changes.
impl AddAssign<i64> for Datetime {
    fn add_assign(&mut self, delta: i64) {
        self._date = self._date.wrapping_add(delta);
    }
}

/// C++ `Datetime& operator-= (const int64_t)`: only `_date` changes.
impl SubAssign<i64> for Datetime {
    fn sub_assign(&mut self, delta: i64) {
        self._date = self._date.wrapping_sub(delta);
    }
}

/// C++ `time_t operator- (const Datetime&)`: the difference in seconds.
impl Sub<Datetime> for Datetime {
    type Output = i64;

    fn sub(self, rhs: Datetime) -> i64 {
        self._date.wrapping_sub(rhs._date)
    }
}

/// C++ `time_t operator- (const Datetime&)` on references.
impl Sub<&Datetime> for &Datetime {
    type Output = i64;

    fn sub(self, rhs: &Datetime) -> i64 {
        self._date.wrapping_sub(rhs._date)
    }
}

#[cfg(test)]
#[path = "datetime_tests.rs"]
mod upstream_tests;

#[cfg(test)]
mod tests {
    //! Direct tests of the ported private helpers, grammar productions and
    //! every named date (`parse_named` / `initialize*`), complementing the
    //! line-by-line port of libshared `test/datetime.t.cpp` and
    //! `test/dates.t.cpp` in `datetime_tests.rs`. Tests that set `TZ` hold
    //! [`crate::test_util::env_lock`] and restore `TZ` (via
    //! [`crate::test_util::EnvGuard`]) before the lock is released.
    use super::*;
    use crate::test_util::{env_lock, EnvGuard};

    /// Runs `f` with `TZ=tz`, holding the env lock; restores `TZ` afterwards.
    fn with_tz<F: FnOnce()>(tz: &str, f: F) {
        let _lock = env_lock();
        let env = EnvGuard::save(&["TZ"]);
        std::env::set_var("TZ", tz);
        localtime::tzset();
        f();
        drop(env);
        localtime::tzset();
    }

    /// Restores the thread-local settings changed by a test.
    struct SettingsGuard {
        weekstart: i32,
        minimum_match_length: i32,
        time_relative: bool,
    }

    impl SettingsGuard {
        fn save() -> Self {
            SettingsGuard {
                weekstart: Datetime::weekstart(),
                minimum_match_length: Datetime::minimum_match_length(),
                time_relative: Datetime::time_relative(),
            }
        }
    }

    impl Drop for SettingsGuard {
        fn drop(&mut self) {
            Datetime::set_weekstart(self.weekstart);
            Datetime::set_minimum_match_length(self.minimum_match_length);
            Datetime::set_time_relative(self.time_relative);
        }
    }

    #[test]
    fn static_setting_defaults_and_setters() {
        let _settings = SettingsGuard::save();
        assert_eq!(Datetime::weekstart(), 1, "weekstart defaults to Monday");
        assert_eq!(Datetime::minimum_match_length(), 3);
        assert!(Datetime::iso_enabled());
        assert!(Datetime::standalone_date_enabled());
        assert!(Datetime::standalone_time_enabled());
        assert!(Datetime::time_relative());

        Datetime::set_weekstart(0);
        assert_eq!(Datetime::weekstart(), 0);
        assert_eq!(Datetime::from_epoch(0)._weekday, 0, "clear () uses weekstart");
        Datetime::set_iso_enabled(false);
        assert!(!Datetime::iso_enabled());
        Datetime::set_iso_enabled(true);
        Datetime::set_standalone_date_enabled(false);
        assert!(!Datetime::standalone_date_enabled());
        Datetime::set_standalone_date_enabled(true);
        Datetime::set_standalone_time_enabled(false);
        assert!(!Datetime::standalone_time_enabled());
        Datetime::set_standalone_time_enabled(true);
    }

    #[test]
    fn epoch_constants() {
        assert_eq!(EPOCH_MIN_VALUE, 315532800);
        assert_eq!(EPOCH_MAX_VALUE, 253402293599);
        with_tz("UTC", || {
            assert_eq!(Datetime::from_epoch(EPOCH_MIN_VALUE).to_iso(), "19800101T000000Z");
        });
    }

    #[test]
    fn constructors_and_fields() {
        let dt = Datetime::from_epoch(1234567890);
        assert_eq!(dt._date, 1234567890);
        assert_eq!(
            (dt._year, dt._month, dt._week, dt._weekday, dt._julian, dt._day, dt._seconds, dt._offset, dt._utc),
            (0, 0, 0, 1, 0, 0, 0, 0, false)
        );

        let before = localtime::time_now();
        let now = Datetime::new();
        let after = localtime::time_now();
        assert!(now._date >= before && now._date <= after, "Datetime () is now");
        assert!(Datetime::default()._date >= before);
        with_tz("UTC", || {
            assert!(!Datetime::new().to_iso().contains("1969"), "'now' != 1969");
        });
    }

    #[test]
    fn from_string_error_message() {
        // 'foo' matches no grammar production (nor a named date).
        match Datetime::from_string("foo", "m/d/Y") {
            Err(TwError::Message(m)) => assert_eq!(m, "'foo' is not a valid date in the 'm/d/Y' format."),
            other => panic!("unexpected {other:?}"),
        }
        assert!(!Datetime::valid_str("foo", ""));

        let mut iso = Datetime::from_epoch(0);
        let mut start = 0;
        assert!(!iso.parse("foo", &mut start, ""), "foo --> false");
        assert_eq!(start, 0, "foo[0]");
    }

    #[test]
    fn valid_overloads() {
        assert!(Datetime::valid_ymd(2008, 2, 29), "valid: 2/29/2008");
        assert!(!Datetime::valid_ymd(2007, 2, 29), "invalid: 2/29/2007");
        assert!(!Datetime::valid_ymd(-1, 2, 1));
        assert!(!Datetime::valid_ymd(2007, 13, 1));
        assert!(!Datetime::valid_ymd(2007, 4, 31));

        assert!(Datetime::valid_yd(2008, 366), "valid: 366 days in 2008");
        assert!(!Datetime::valid_yd(2007, 366), "invalid: 366 days in 2007");
        assert!(!Datetime::valid_yd(2007, 0));
        assert!(!Datetime::valid_yd(-1, 1));

        assert!(Datetime::valid_ymdhms(2010, 2, 28, 0, 0, 0), "valid 2/28/2010 0:00:00");
        assert!(Datetime::valid_ymdhms(2010, 2, 28, 23, 59, 59), "valid 2/28/2010 23:59:59");
        assert!(!Datetime::valid_ymdhms(2010, 2, 28, 24, 59, 59), "valid 2/28/2010 24:59:59");
        assert!(!Datetime::valid_ymdhms(2010, 2, 28, -1, 0, 0), "valid 2/28/2010 -1:00:00");
        assert!(Datetime::valid_ymdhms(2010, 2, 28, 24, 0, 0));
        assert!(!Datetime::valid_ymdhms(2010, 2, 28, 1, 60, 0));
        assert!(!Datetime::valid_ymdhms(2010, 2, 28, 1, 0, 60));
        assert!(!Datetime::valid_ymdhms(2010, 2, 29, 1, 0, 0));
    }

    #[test]
    fn leap_year_and_month_lengths() {
        assert!(Datetime::leap_year(2008), "2008 is a leap year");
        assert!(!Datetime::leap_year(2007), "2007 is not a leap year");
        assert!(Datetime::leap_year(2000), "2000 is a leap year");
        assert!(!Datetime::leap_year(1900), "1900 is not a leap year");

        assert_eq!(Datetime::days_in_year(2016), 366, "366 days in 2016");
        assert_eq!(Datetime::days_in_year(2015), 365, "365 days in 2015");

        assert_eq!(Datetime::days_in_month(2008, 2), 29, "29 days in February 2008");
        assert_eq!(Datetime::days_in_month(2007, 2), 28, "28 days in February 2007");
        let expected = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for (m, days) in expected.iter().enumerate() {
            assert_eq!(Datetime::days_in_month(2015, m as i32 + 1), *days);
        }
    }

    #[test]
    fn month_and_day_names() {
        let months = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        for (i, name) in months.iter().enumerate() {
            let m = i as i32 + 1;
            assert_eq!(Datetime::month_name(m), *name, "{m} = {name}");
            assert_eq!(Datetime::month_name_short(m), name[..3], "{m} = short {name}");
            assert_eq!(Datetime::month_of_year(name), m, "{name} = {m}");
        }

        let days = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        for (i, name) in days.iter().enumerate() {
            let d = i as i32;
            assert_eq!(Datetime::day_name(d), *name, "{d} == {name}");
            assert_eq!(Datetime::day_name_short(d), name[..3], "{d} == short {name}");
            assert_eq!(Datetime::day_of_week_str(name), d, "{name} == {d}");
        }

        assert_eq!(Datetime::day_of_week_str("SUNDAY"), 0, "SUNDAY == 0");
        assert_eq!(Datetime::day_of_week_str("sunday"), 0, "sunday == 0");
        assert_eq!(Datetime::day_of_week_str("Sunday"), 0, "Sunday == 0");
        assert_eq!(Datetime::day_of_week_str("sun"), 0);
        assert_eq!(Datetime::day_of_week_str("su"), -1, "shorter than minimumMatchLength");
        assert_eq!(Datetime::day_of_week_str("sundayx"), -1);
        assert_eq!(Datetime::day_of_week_str("foo"), -1);
        assert_eq!(Datetime::month_of_year("dec"), 12);
        assert_eq!(Datetime::month_of_year("de"), -1);
        assert_eq!(Datetime::month_of_year("foo"), -1);
    }

    #[test]
    fn minimum_match_length_zero_resets_to_three() {
        let _settings = SettingsGuard::save();
        Datetime::set_minimum_match_length(0);
        assert_eq!(Datetime::day_of_week_str("mo"), -1);
        assert_eq!(Datetime::minimum_match_length(), 3);
        Datetime::set_minimum_match_length(0);
        assert_eq!(Datetime::month_of_year("ja"), -1);
        assert_eq!(Datetime::minimum_match_length(), 3);
        Datetime::set_minimum_match_length(2);
        assert_eq!(Datetime::day_of_week_str("mo"), 1);
        assert_eq!(Datetime::month_of_year("ja"), 1);
    }

    #[test]
    fn day_of_week_zeller() {
        assert_eq!(Datetime::day_of_week_ymd(2008, 1, 1), 2, "1/1/2008 == Tuesday");
        assert_eq!(Datetime::day_of_week_ymd(2015, 10, 28), 3);
        assert_eq!(Datetime::day_of_week_ymd(9015, 10, 28), 6);
        assert_eq!(Datetime::day_of_week_ymd(2000, 2, 29), 2);
        assert_eq!(Datetime::day_of_week_ymd(1981, 1, 4), 0);
        assert_eq!(Datetime::day_of_week_ymd(2024, 3, 1), 5);
    }

    #[test]
    fn length_of_formats() {
        let cases = [
            ("m", 2),
            ("M", 2),
            ("d", 2),
            ("D", 2),
            ("y", 2),
            ("Y", 4),
            ("a", 3),
            ("A", 10),
            ("b", 3),
            ("B", 10),
            ("v", 2),
            ("V", 2),
            ("h", 2),
            ("H", 2),
            ("n", 2),
            ("N", 2),
            ("s", 2),
            ("S", 2),
            ("j", 3),
            ("J", 3),
            (" ", 1),
            ("Y-M-D", 10),
            ("w", 1),
            ("", 0),
        ];
        for (format_, len) in cases {
            assert_eq!(Datetime::length(format_), len, "length '{format_}' --> {len}");
        }
    }

    #[test]
    fn ymd_construction_and_getters() {
        for tz in ["UTC", "America/New_York"] {
            with_tz(tz, || {
                let happy_new_year = Datetime::from_ymd(2008, 1, 1);
                assert_eq!(happy_new_year.day_of_week(), 2, "1/1/2008 == Tuesday");
                assert_eq!(happy_new_year.month(), 1, "1/1/2008 == January");
                assert_eq!(happy_new_year.day(), 1, "1/1/2008 == 1");
                assert_eq!(happy_new_year.year(), 2008, "1/1/2008 == 2008");
                assert_eq!(happy_new_year.to_string("Y-M-D"), "2008-01-01", "toString 2008-01-01");
                assert_eq!((happy_new_year.hour(), happy_new_year.minute(), happy_new_year.second()), (0, 0, 0));
                assert_eq!(happy_new_year.day_of_year(), 1);

                let (mut y, mut m, mut d) = (0, 0, 0);
                happy_new_year.to_ymd(&mut y, &mut m, &mut d);
                assert_eq!((y, m, d), (2008, 1, 1));

                let mut epoch = Datetime::from_ymd(2001, 9, 8);
                assert!(epoch.to_epoch() < 1000000000, "9/8/2001 < 1,000,000,000");
                epoch += 172800;
                assert!(epoch.to_epoch() > 1000000000, "9/10/2001 > 1,000,000,000");

                let from_epoch = Datetime::from_epoch(epoch.to_epoch());
                assert_eq!(from_epoch.to_string("Y-M-D"), epoch.to_string("Y-M-D"), "ctor (time_t)");

                let iso = Datetime::from_epoch(1000000000);
                assert_eq!(iso.to_iso(), "20010909T014640Z", "1,000,000,000 -> 20010909T014640Z");

                // Test for Y2038 problem
                let f_epoch = Datetime::from_ymd(2039, 1, 1);
                assert!(f_epoch.to_epoch() > 2147483647, "9/01/2039 > 2,147,483,647");

                let f_iso = Datetime::from_epoch(2147483650);
                assert_eq!(f_iso.to_iso(), "20380119T031410Z", "2147483650 -> 20380119T031410Z");
            });
        }
    }

    #[test]
    fn local_epochs_by_zone() {
        with_tz("UTC", || {
            assert_eq!(Datetime::from_ymd(2013, 12, 6).to_epoch(), 1386288000);
            assert_eq!(Datetime::from_ymdhms(2015, 10, 28, 12, 55, 0).to_epoch(), 1446036900);
            assert_eq!(Datetime::from_epoch(1000000000).to_iso_local_extended(), "2001-09-09T01:46:40");
        });
        with_tz("America/New_York", || {
            assert_eq!(Datetime::from_ymd(2013, 12, 6).to_epoch(), 1386288000 + 5 * 3600);
            assert_eq!(Datetime::from_ymdhms(2015, 10, 28, 12, 55, 0).to_epoch(), 1446036900 + 4 * 3600);
            assert_eq!(Datetime::from_epoch(1000000000).to_iso_local_extended(), "2001-09-08T21:46:40");
            assert_eq!(Datetime::from_epoch(1000000000).to_iso(), "20010909T014640Z");
        });
    }

    #[test]
    fn epoch_string_and_julian() {
        let dt = Datetime::from_epoch(1234567890);
        assert_eq!(dt.to_epoch_string(), "1234567890");
        assert_eq!(Datetime::from_epoch(0).to_julian(), 2440587.5);
        assert_eq!(Datetime::from_epoch(86400).to_julian(), 2440588.5);
        assert_eq!(Datetime::from_epoch(43200).to_julian(), 2440588.0);
    }

    #[test]
    fn quantization() {
        for tz in ["UTC", "America/New_York"] {
            with_tz(tz, || {
                let quant = Datetime::from_epoch(1234526400);
                assert_eq!(quant.start_of_day().to_string("YMDHNS"), "20090213000000", "startOfDay");
                assert_eq!(quant.start_of_week().to_string("YMDHNS"), "20090208000000", "startOfWeek");
                assert_eq!(quant.start_of_month().to_string("YMDHNS"), "20090201000000", "startOfMonth");
                assert_eq!(quant.start_of_year().to_string("YMDHNS"), "20090101000000", "startOfYear");
            });
        }
    }

    #[test]
    fn to_string_all_format_options() {
        for tz in ["UTC", "America/New_York"] {
            with_tz(tz, || {
                let _settings = SettingsGuard::save();
                Datetime::set_weekstart(1);

                let r32 = Datetime::from_ymdhms(2015, 10, 28, 12, 55, 0);
                let cases = [
                    ("Y", "2015"),
                    ("y", "15"),
                    ("M", "10"),
                    ("m", "10"),
                    ("D", "28"),
                    ("d", "28"),
                    ("H", "12"),
                    ("h", "12"),
                    ("N", "55"),
                    ("n", "55"),
                    ("S", "00"),
                    ("s", "0"),
                    ("A", "Wednesday"),
                    ("a", "Wed"),
                    ("B", "October"),
                    ("b", "Oct"),
                    ("V", "44"),
                    ("v", "44"),
                    ("J", "301"),
                    ("j", "301"),
                    ("w", "3"),
                ];
                for (f, expected) in cases {
                    assert_eq!(r32.to_string(f), expected, "2015-10-28T12:55:00 -> {f} -> {expected}");
                }

                // Test all format options for a future date
                let r38 = Datetime::from_ymdhms(9015, 10, 28, 12, 55, 0);
                let cases = [
                    ("Y", "9015"),
                    ("y", "15"),
                    ("M", "10"),
                    ("m", "10"),
                    ("D", "28"),
                    ("d", "28"),
                    ("H", "12"),
                    ("h", "12"),
                    ("N", "55"),
                    ("n", "55"),
                    ("S", "00"),
                    ("s", "0"),
                    ("A", "Saturday"),
                    ("a", "Sat"),
                    ("B", "October"),
                    ("b", "Oct"),
                    ("V", "43"),
                    ("v", "43"),
                    ("J", "301"),
                    ("j", "301"),
                    ("w", "6"),
                ];
                for (f, expected) in cases {
                    assert_eq!(r38.to_string(f), expected, "9015-10-28T12:55:00 -> {f} -> {expected}");
                }
            });
        }
    }

    #[test]
    fn to_string_padding_and_passthrough() {
        with_tz("UTC", || {
            let dt = Datetime::from_ymdhms(2009, 2, 3, 4, 5, 6);
            assert_eq!(dt.to_string("Y-M-D H:N:S"), "2009-02-03 04:05:06");
            assert_eq!(dt.to_string("m/d/y h:n:s"), "2/3/09 4:5:6");
            assert_eq!(dt.to_string("J j"), "034 34");
            assert_eq!(dt.to_string("[Y] é → x"), "[2009] é → x", "other bytes pass through");
            assert_eq!(dt.to_string(""), "");
            assert_eq!(Datetime::from_ymd(2000, 1, 2).to_string("y V v"), "00 52 52");
        });
    }

    #[test]
    fn week_numbers_by_weekstart() {
        with_tz("UTC", || {
            let _settings = SettingsGuard::save();
            // 2015-10-28: %U 43, %V 44.
            let dt = Datetime::from_ymd(2015, 10, 28);
            Datetime::set_weekstart(1);
            assert_eq!(dt.week(), 44);
            Datetime::set_weekstart(0);
            assert_eq!(dt.week(), 43);
            assert_eq!(dt.to_string("V"), "43");
            // 2017-01-01 (Sunday): %U 01, %V 52.
            let jan1 = Datetime::from_ymd(2017, 1, 1);
            assert_eq!(jan1.to_string("V v"), "01 1");
            Datetime::set_weekstart(1);
            assert_eq!(jan1.to_string("V v"), "52 52");

            Datetime::set_weekstart(2);
            match dt.try_week() {
                Err(TwError::Message(m)) => assert_eq!(m, "The week may only start on a Sunday or Monday."),
                other => panic!("unexpected {other:?}"),
            }
        });
    }

    #[test]
    fn comparisons_and_same_periods() {
        with_tz("America/New_York", || {
            let _settings = SettingsGuard::save();
            Datetime::set_weekstart(1);

            let left = Datetime::from_ymd(2008, 7, 4);
            let comp1 = Datetime::from_ymd(2008, 7, 4);
            assert!(left.same_day(&comp1), "7/4/2008 is on the same day as 7/4/2008");
            assert!(left.same_week(&comp1), "7/4/2008 is on the same week as 7/4/2008");
            assert!(left.same_month(&comp1), "7/4/2008 is in the same month as 7/4/2008");
            assert!(left.same_quarter(&comp1), "7/4/2008 is in the same quarter as 7/4/2008");
            assert!(left.same_year(&comp1), "7/4/2008 is in the same year as 7/4/2008");

            let comp2 = Datetime::from_ymd(2008, 7, 5);
            assert!(!left.same_day(&comp2), "7/4/2008 is not on the same day as 7/5/2008");
            assert!(left.same_month(&comp2), "7/4/2008 is in the same month as 7/5/2008");
            assert!(left.same_quarter(&comp2), "7/4/2008 is in the same quarter as 7/5/2008");
            assert!(left.same_year(&comp2), "7/4/2008 is in the same year as 7/5/2008");

            let comp3 = Datetime::from_ymd(2008, 8, 4);
            assert!(!left.same_day(&comp3), "7/4/2008 is not on the same day as 8/4/2008");
            assert!(!left.same_week(&comp3), "7/4/2008 is not on the same week as 8/4/2008");
            assert!(!left.same_month(&comp3), "7/4/2008 is not in the same month as 8/4/2008");
            assert!(left.same_quarter(&comp3), "7/4/2008 is in the same quarter as 8/4/2008");
            assert!(left.same_year(&comp3), "7/4/2008 is in the same year as 8/4/2008");

            let comp4 = Datetime::from_ymd(2009, 7, 4);
            assert!(!left.same_day(&comp4), "7/4/2008 is not on the same day as 7/4/2009");
            assert!(!left.same_week(&comp4), "7/4/2008 is not on the same week as 7/4/2009");
            assert!(!left.same_month(&comp4), "7/4/2008 is not in the same month as 7/4/2009");
            assert!(!left.same_quarter(&comp4), "7/4/2008 is not in the same quarter as 7/4/2009");
            assert!(!left.same_year(&comp4), "7/4/2008 is not in the same year as 7/4/2009");

            assert!(!left.same_quarter(&Datetime::from_ymd(2008, 10, 1)));
            assert!(left.same_quarter(&Datetime::from_ymd(2008, 9, 30)));

            // Datetime::sameHour
            let r20 = Datetime::from_ymdhms(2010, 6, 7, 1, 0, 0);
            let r21 = Datetime::from_ymdhms(2010, 6, 7, 1, 59, 59);
            assert!(r20.same_hour(&r21), "two dates within the same hour");
            let r22 = Datetime::from_ymdhms(2010, 6, 7, 0, 59, 59);
            assert!(!r20.same_hour(&r22), "two dates not within the same hour");

            // Relational operators compare _date only.
            let mut other_fields = Datetime::from_epoch(left._date);
            other_fields._year = 1999;
            assert!(left == other_fields);
            assert!(left != comp2);
            assert!(left < comp2);
            assert!(comp2 > left);
            assert!(left <= comp1);
            assert!(left >= comp1);
            assert!(left <= comp2);
            assert!(comp4 >= comp3);
        });
    }

    #[test]
    fn arithmetic() {
        // Datetime::operator-
        let r25 = Datetime::from_epoch(1234567890);
        assert_eq!((r25 - 1).to_epoch(), 1234567889, "1234567890 - 1 = 1234567889");
        assert_eq!((r25 + 10).to_epoch(), 1234567900);
        let mut r = r25;
        r += 100;
        assert_eq!(r.to_epoch(), 1234567990);
        r -= 50;
        assert_eq!(r.to_epoch(), 1234567940);
        assert_eq!(r - r25, 50);
        #[allow(clippy::op_ref)] // Exercises the by-reference impl.
        let by_ref = &r25 - &r;
        assert_eq!(by_ref, -50);
        assert_eq!(r25.to_epoch(), 1234567890, "operator+/- do not modify the operand");
    }

    #[test]
    fn increment_decrement_across_dst() {
        with_tz("America/New_York", || {
            // Datetime::operator--
            let mut r26 = Datetime::from_ymdhms(2010, 11, 7, 23, 59, 59);
            r26.dec();
            assert_eq!(r26.to_string("YMDHNS"), "20101106235959", "decrement across fall DST boundary");

            let mut r27 = Datetime::from_ymdhms(2010, 3, 14, 23, 59, 59);
            r27.dec();
            assert_eq!(r27.to_string("YMDHNS"), "20100313235959", "decrement across spring DST boundary");

            // Datetime::operator++
            let mut r28 = Datetime::from_ymdhms(2010, 11, 6, 23, 59, 59);
            r28.inc();
            assert_eq!(r28.to_string("YMDHNS"), "20101107235959", "increment across fall DST boundary");

            let mut r29 = Datetime::from_ymdhms(2010, 3, 13, 23, 59, 59);
            r29.inc();
            assert_eq!(r29.to_string("YMDHNS"), "20100314235959", "increment across spring DST boundary");

            // Fall-back day is 25 hours long.
            assert_eq!(
                Datetime::from_ymd(2010, 11, 8) - Datetime::from_ymd(2010, 11, 7),
                25 * 3600
            );
        });
    }

    fn tm_for_year(year: i32) -> Tm {
        Tm { tm_year: year - 1900, tm_mon: 0, tm_mday: 1, tm_isdst: -1, ..Tm::default() }
    }

    #[test]
    fn easter_dates() {
        let dt = Datetime::from_epoch(0);
        for (year, month, day) in [(2017, 4, 16), (2024, 3, 31), (2025, 4, 20), (2000, 4, 23), (2038, 4, 25)] {
            let mut t = tm_for_year(year);
            t.tm_hour = 5;
            dt.easter(&mut t);
            assert_eq!(
                (t.tm_year + 1900, t.tm_mon + 1, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec, t.tm_isdst),
                (year, month, day, 0, 0, 0, -1),
                "easter {year}"
            );
        }
    }

    #[test]
    fn midsommar_dates() {
        for tz in ["UTC", "America/New_York"] {
            with_tz(tz, || {
                let dt = Datetime::from_epoch(0);
                for (year, midsommar, afton) in [(2017, 24, 23), (2024, 22, 21), (2015, 20, 19), (2021, 26, 18)] {
                    let mut t = tm_for_year(year);
                    dt.midsommar(&mut t);
                    let epoch = localtime::mktime(&mut t);
                    assert_eq!(Datetime::from_epoch(epoch).to_string("Y-M-D H:N:S w"), format!("{year}-06-{midsommar} 00:00:00 6"));

                    let mut t = tm_for_year(year);
                    dt.midsommarafton(&mut t);
                    let epoch = localtime::mktime(&mut t);
                    assert_eq!(Datetime::from_epoch(epoch).to_string("Y-M-D H:N:S w"), format!("{year}-06-{afton} 00:00:00 5"));
                }
            });
        }
    }

    #[test]
    fn ordinals() {
        let dt = Datetime::from_epoch(0);
        let mut ordinal = 0;
        for (token, expected) in [("1st", 1), ("2nd", 2), ("3rd", 3), ("4th", 4), ("11th", 11), ("12th", 12), ("13th", 13), ("21st", 21), ("22nd", 22), ("23rd", 23), ("1th", 1)] {
            assert!(dt.is_ordinal(token, &mut ordinal), "{token}");
            assert_eq!(ordinal, expected, "{token}");
        }
        // C++ accepts these too: the `% 10` rules ignore the 11-13 exception.
        for token in ["11st", "12nd", "13rd"] {
            assert!(dt.is_ordinal(token, &mut ordinal), "{token}");
        }
        ordinal = -7;
        for token in ["1nd", "2st", "3nd", "1", "st", "", "4st", "1st ", "x1st"] {
            assert!(!dt.is_ordinal(token, &mut ordinal), "{token}");
        }
        assert_eq!(ordinal, -7, "failure leaves the output untouched");

        let mut dt = Datetime::from_epoch(0);
        let tokens = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        assert!(dt.initialize_nth_day_in_month(&tokens("2nd monday in march")));
        assert!(dt.initialize_nth_day_in_month(&tokens("1st fri of jan")));
        assert!(!dt.initialize_nth_day_in_month(&tokens("2nd monday at march")));
        assert!(!dt.initialize_nth_day_in_month(&tokens("2nd monday in")));
        assert!(!dt.initialize_nth_day_in_month(&tokens("second monday in march")));
    }

    #[allow(clippy::too_many_arguments)]
    fn fields(year: i32, month: i32, week: i32, weekday: i32, julian: i32, day: i32, seconds: i32, offset: i32, utc: bool) -> Datetime {
        let mut dt = Datetime::from_epoch(0);
        dt._year = year;
        dt._month = month;
        dt._week = week;
        dt._weekday = weekday;
        dt._julian = julian;
        dt._day = day;
        dt._seconds = seconds;
        dt._offset = offset;
        dt._utc = utc;
        dt
    }

    #[test]
    fn validate_ranges() {
        let _settings = SettingsGuard::save();
        Datetime::set_weekstart(1);
        assert!(fields(2013, 12, 0, 1, 0, 6, 0, 0, false).validate());
        assert!(fields(0, 0, 0, 1, 0, 0, 86400, 0, false).validate());
        assert!(!fields(1899, 0, 0, 1, 0, 0, 0, 0, false).validate());
        assert!(!fields(10000, 0, 0, 1, 0, 0, 0, 0, false).validate());
        assert!(!fields(2013, 13, 0, 1, 0, 0, 0, 0, false).validate());
        assert!(!fields(2013, 0, 54, 1, 0, 0, 0, 0, false).validate());
        assert!(fields(2013, 0, 53, 7, 0, 0, 0, 0, false).validate());
        assert!(!fields(2013, 0, 1, 0, 0, 0, 0, 0, false).validate(), "weekday 0 invalid for ISO weeks");
        assert!(!fields(2013, 0, 1, 8, 0, 0, 0, 0, false).validate());
        assert!(fields(2012, 0, 0, 1, 366, 0, 0, 0, false).validate());
        assert!(!fields(2013, 0, 0, 1, 366, 0, 0, 0, false).validate());
        assert!(!fields(2013, 2, 0, 1, 0, 29, 0, 0, false).validate());
        assert!(fields(2012, 2, 0, 1, 0, 29, 0, 0, false).validate());
        assert!(!fields(2013, 1, 0, 1, 0, 1, 86401, 0, false).validate());
        assert!(!fields(2013, 1, 0, 1, 0, 1, -1, 0, false).validate());
        assert!(fields(2013, 1, 0, 1, 0, 1, 0, -86400, false).validate());
        assert!(!fields(2013, 1, 0, 1, 0, 1, 0, 86401, false).validate());

        Datetime::set_weekstart(0);
        assert!(fields(2013, 0, 1, 0, 0, 0, 0, 0, false).validate());
        assert!(!fields(2013, 0, 1, 7, 0, 0, 0, 0, false).validate());
    }

    fn resolved(mut dt: Datetime) -> i64 {
        dt.resolve();
        dt._date
    }

    #[test]
    fn resolve_dates() {
        for tz in ["UTC", "America/New_York"] {
            with_tz(tz, || {
                let _settings = SettingsGuard::save();
                Datetime::set_weekstart(1);

                // y m d
                assert_eq!(resolved(fields(2013, 12, 0, 1, 0, 6, 0, 0, false)), Datetime::from_ymd(2013, 12, 6).to_epoch());
                // y m -  -->  y m 1
                assert_eq!(resolved(fields(2013, 12, 0, 1, 0, 0, 0, 0, false)), Datetime::from_ymd(2013, 12, 1).to_epoch());
                // y - -  -->  y 1 1
                assert_eq!(resolved(fields(2013, 0, 0, 1, 0, 0, 0, 0, false)), Datetime::from_ymd(2013, 1, 1).to_epoch());
                // Julian.
                assert_eq!(resolved(fields(2013, 0, 0, 1, 340, 0, 0, 0, false)), Datetime::from_ymd(2013, 12, 6).to_epoch());
                // Time of day.
                assert_eq!(
                    resolved(fields(2013, 12, 0, 1, 0, 6, 12 * 3600 + 34 * 60 + 56, 0, false)),
                    Datetime::from_ymdhms(2013, 12, 6, 12, 34, 56).to_epoch()
                );
                // Seconds past a day roll over.
                assert_eq!(
                    resolved(fields(2013, 12, 0, 1, 0, 6, 86400 + 60, 0, false)),
                    Datetime::from_ymdhms(2013, 12, 7, 0, 1, 0).to_epoch()
                );
                // UTC and offsets.
                assert_eq!(resolved(fields(2013, 12, 0, 1, 0, 6, 0, 0, true)), 1386288000);
                assert_eq!(resolved(fields(2013, 12, 0, 1, 0, 6, 12 * 3600, 3600, false)), 1386288000 + 11 * 3600);
                assert_eq!(resolved(fields(2013, 12, 0, 1, 0, 6, 12 * 3600, -18000, false)), 1386288000 + 17 * 3600);

                // ISO weeks (from the upstream boundary table).
                for (y, w, wd, ey, em, ed) in [
                    (1976, 53, 6, 1977, 1, 1),
                    (1976, 53, 7, 1977, 1, 2),
                    (1977, 52, 6, 1977, 12, 31),
                    (1977, 52, 7, 1978, 1, 1),
                    (1978, 1, 1, 1978, 1, 2),
                    (1978, 52, 7, 1978, 12, 31),
                    (1979, 1, 1, 1979, 1, 1),
                    (1979, 52, 7, 1979, 12, 30),
                    (1980, 1, 1, 1979, 12, 31),
                    (1980, 1, 2, 1980, 1, 1),
                    (1980, 52, 7, 1980, 12, 28),
                    (1981, 1, 1, 1980, 12, 29),
                    (1981, 1, 2, 1980, 12, 30),
                    (1981, 1, 3, 1980, 12, 31),
                    (1981, 1, 4, 1981, 1, 1),
                    (1981, 53, 4, 1981, 12, 31),
                    (1981, 53, 5, 1982, 1, 1),
                    (1981, 53, 6, 1982, 1, 2),
                    (1981, 53, 7, 1982, 1, 3),
                ] {
                    assert_eq!(
                        resolved(fields(y, 0, w, wd, 0, 0, 0, 0, false)),
                        Datetime::from_ymd(ey, em, ed).to_epoch(),
                        "{y}-W{w}-{wd}"
                    );
                }
            });
        }
    }

    #[test]
    fn resolve_time_only_respects_time_relative() {
        with_tz("UTC", || {
            let _settings = SettingsGuard::save();
            Datetime::set_weekstart(1);
            let today = Datetime::new().start_of_day();

            // 00:00:01 is (almost always) earlier than now: tomorrow when
            // relative, today otherwise. Skip the one-second race at midnight.
            if Datetime::new().to_epoch() - today.to_epoch() > 1 {
                Datetime::set_time_relative(true);
                let t = Datetime::from_epoch(resolved(fields(0, 0, 0, 1, 0, 0, 1, 0, false)));
                assert!(!today.same_day(&t), "Datetime::timeRelative=true 0:00:01 --> tomorrow");

                Datetime::set_time_relative(false);
                let t = Datetime::from_epoch(resolved(fields(0, 0, 0, 1, 0, 0, 1, 0, false)));
                assert!(today.same_day(&t), "Datetime::timeRelative=false 0:00:01 --> today");
            }
        });
    }

    // -------------------------------------------------------------------------
    // Grammar productions: ISO 8601 forms, epoch and rc.dateformat parsing.
    // (The upstream testParse table, negative cases and fromString cases are
    // ported line by line in datetime_tests.rs.)

    /// C++ `testParse (t, input)` (datetime.t.cpp line 68).
    fn test_parse(input: &str) {
        let mut positive = Datetime::new();
        let mut pos = 0usize;
        assert!(
            positive.parse(input, &mut pos, "") && pos != 0,
            "Datetime::parse positive '{input}' --> success"
        );
    }

    /// C++ `testParseError` (datetime.t.cpp line 80).
    fn test_parse_error(input: &str) {
        let mut neg = Datetime::new();
        let mut pos = 0usize;
        assert!(!neg.parse(input, &mut pos, ""), "Datetime::parse negative '{input}' --> fail");
    }

    #[test]
    fn parse_iso_disabled_allows_only_date_time_forms() {
        with_tz("Europe/Berlin", || {
            let _settings = SettingsGuard::save();
            Datetime::set_weekstart(1);
            // isoEnabled=false still allows the date-time forms (import, JSON,
            // hooks), but not the date-only or time-only forms.
            Datetime::set_iso_enabled(false);
            test_parse("2013-12-06T12:34:56Z");
            test_parse("20131206T123456Z");
            test_parse_error("2013-12-06");
            // Not as ISO time-ext, but (as in C++) as an informal time via
            // parse_named, which leaves the ISO fields untouched.
            let mut informal = Datetime::new();
            let mut start = 0usize;
            assert!(informal.parse("12:34:56", &mut start, ""), "isoEnabled=false '12:34:56' --> informal time");
            assert_eq!(start, 8);
            assert_eq!(informal._seconds, 0, "isoEnabled=false '12:34:56' is not parse_time_ext");
            Datetime::set_iso_enabled(true);
        });
    }

    #[test]
    fn parse_epoch_bounds() {
        with_tz("Europe/Berlin", || {
            let mut dt = Datetime::new();
            let mut start = 0;
            assert!(dt.parse("1234567890", &mut start, ""));
            assert_eq!(start, 10);
            assert_eq!(dt._date, 1234567890);

            // EPOCH_MIN_VALUE is accepted, one second before is not an epoch
            // (and not any ISO form either).
            let mut dt = Datetime::new();
            let mut start = 0;
            assert!(dt.parse("315532800", &mut start, ""));
            assert_eq!(dt._date, EPOCH_MIN_VALUE);
            test_parse_error("315532799");

            // '12' is not an epoch (but HHMM-free '12' fails everything).
            test_parse_error("12");

            // An epoch directly followed by a Latin letter is not an epoch.
            let mut dt = Datetime::new();
            let mut pig = Pig::new("1234567890d");
            assert!(!dt.parse_epoch(&mut pig));
            assert_eq!(pig.cursor(), 0, "cursor restored");

            // EPOCH_MAX_VALUE itself is excluded.
            let mut pig = Pig::new("253402293599");
            assert!(!dt.parse_epoch(&mut pig));
            let mut pig = Pig::new("253402293598");
            assert!(dt.parse_epoch(&mut pig));
            assert_eq!(dt._date, EPOCH_MAX_VALUE - 1);

            // Parsing from an offset leaves the cursor past the match.
            let mut dt = Datetime::new();
            let mut start = 4;
            assert!(dt.parse("due:1234567890 x", &mut start, ""));
            assert_eq!(start, 14);
        });
    }

    #[test]
    fn parse_iso_from_offset_and_restore() {
        with_tz("UTC", || {
            let _settings = SettingsGuard::save();
            Datetime::set_weekstart(1);

            let mut dt = Datetime::new();
            let mut start = 4;
            assert!(dt.parse("due:2013-12-06T12:34:56Z,", &mut start, ""));
            assert_eq!(start, 24);
            assert_eq!(dt._date, 1386333296);

            // A failed production restores the cursor.
            let mut dt = Datetime::new();
            let mut pig = Pig::new("2013-12-06Tfoo");
            assert!(!dt.parse_date_time_ext(&mut pig));
            assert_eq!(pig.cursor(), 0);
            let mut pig = Pig::new("12:34:5");
            assert!(!dt.parse_time_ext(&mut pig, true));
            assert_eq!(pig.cursor(), 0);
            let mut pig = Pig::new("+01:");
            assert!(!dt.parse_off_ext(&mut pig));
            assert_eq!(pig.cursor(), 0);

            // Out-params are only written on success.
            let mut value = 42;
            let mut pig = Pig::new("13");
            assert!(!dt.parse_month(&mut pig, &mut value));
            assert_eq!(value, 42);
            assert_eq!(pig.cursor(), 0);
            assert!(dt.parse_hour(&mut pig, &mut value));
            assert_eq!(value, 13);
            assert_eq!(pig.cursor(), 2);

            let mut pig = Pig::new("1969");
            assert!(!dt.parse_year(&mut pig, &mut value));
            let mut pig = Pig::new("1970");
            assert!(dt.parse_year(&mut pig, &mut value));
            assert_eq!(value, 1970);

            let mut pig = Pig::new("13");
            assert!(!dt.parse_off_hour(&mut pig, &mut value));
            let mut pig = Pig::new("12");
            assert!(dt.parse_off_hour(&mut pig, &mut value));

            let mut pig = Pig::new("367");
            assert!(!dt.parse_julian(&mut pig, &mut value));
            let mut pig = Pig::new("366");
            assert!(dt.parse_julian(&mut pig, &mut value));
            assert_eq!(value, 366);

            let mut pig = Pig::new("00");
            assert!(!dt.parse_week(&mut pig, &mut value));
            assert!(!dt.parse_day(&mut pig, &mut value));
            assert!(dt.parse_minute(&mut pig, &mut value));
            assert_eq!(value, 0);

            let mut pig = Pig::new("0");
            assert!(!dt.parse_weekday(&mut pig, &mut value), "0 invalid when weekstart=1");
            Datetime::set_weekstart(0);
            assert!(dt.parse_weekday(&mut pig, &mut value));
            assert_eq!(value, 0);
            let mut pig = Pig::new("7");
            assert!(!dt.parse_weekday(&mut pig, &mut value), "7 invalid when weekstart=0");
            Datetime::set_weekstart(1);

            // Offset forms set _offset; '-' sign negates.
            let mut dt = Datetime::new();
            let mut pig = Pig::new("-0530");
            assert!(dt.parse_off(&mut pig));
            assert_eq!(dt._offset, -(5 * 3600 + 30 * 60));
            let mut pig = Pig::new("+05:30");
            assert!(dt.parse_off_ext(&mut pig));
            assert_eq!(dt._offset, 5 * 3600 + 30 * 60);

            // Time-ext followed by am/pm is left for the informal-time parser.
            let mut pig = Pig::new("10:30am");
            assert!(!dt.parse_time_ext(&mut pig, true));
            assert_eq!(pig.cursor(), 0);
        });
    }

    #[test]
    fn parse_formatted_from_string_cases() {
        for tz in ["UTC", "Europe/Berlin"] {
            with_tz(tz, || {
                let _settings = SettingsGuard::save();
                Datetime::set_weekstart(1);
                let ymd = |dt: &Datetime| (dt.year(), dt.month(), dt.day());
                let hns = |dt: &Datetime| (dt.hour(), dt.minute(), dt.second());

                // Format parsing (datetime.t.cpp 730-795).
                assert_eq!(ymd(&Datetime::from_string("1/1/2008", "m/d/Y").unwrap()), (2008, 1, 1), "fromString1");
                assert_eq!(ymd(&Datetime::from_string("20080101", "YMD").unwrap()), (2008, 1, 1), "fromString2");
                assert_eq!(ymd(&Datetime::from_string("12/31/2007", "m/d/Y").unwrap()), (2007, 12, 31), "fromString3");
                assert_eq!(ymd(&Datetime::from_string("01/01/2008", "m/d/Y").unwrap()), (2008, 1, 1), "fromString4");
                assert_eq!(
                    ymd(&Datetime::from_string("Tue 05 Feb 2008 (06)", "a D b Y (V)").unwrap()),
                    (2008, 2, 5),
                    "fromString5"
                );
                assert_eq!(
                    ymd(&Datetime::from_string("Tuesday, February 5, 2008", "A, B d, Y").unwrap()),
                    (2008, 2, 5),
                    "fromString6"
                );
                assert_eq!(
                    ymd(&Datetime::from_string("w01 Tue 2008-01-01", "wV a Y-M-D").unwrap()),
                    (2008, 1, 1),
                    "fromString7"
                );

                let f8 = Datetime::from_string("6/7/2010 1:23:45", "m/d/Y h:N:S").unwrap();
                assert_eq!((ymd(&f8), hns(&f8)), ((2010, 6, 7), (1, 23, 45)), "fromString8");
                let f9 = Datetime::from_string("6/7/2010 01:23:45", "m/d/Y H:N:S").unwrap();
                assert_eq!((ymd(&f9), hns(&f9)), ((2010, 6, 7), (1, 23, 45)), "fromString9");
                let f10 = Datetime::from_string("6/7/2010 12:34:56", "m/d/Y H:N:S").unwrap();
                assert_eq!((ymd(&f10), hns(&f10)), ((2010, 6, 7), (12, 34, 56)), "fromString10");
                let f11 = Datetime::from_string("6/7/3010 12:34:56", "m/d/Y H:N:S").unwrap();
                assert_eq!((ymd(&f11), hns(&f11)), ((3010, 6, 7), (12, 34, 56)), "fromString11");

                // Day of year.
                assert_eq!(Datetime::from_string("1/1/2011", "m/d/Y").unwrap().day_of_year(), 1);
                assert_eq!(Datetime::from_string("5/1/2011", "m/d/Y").unwrap().day_of_year(), 121);
                assert_eq!(Datetime::from_string("12/31/2011", "m/d/Y").unwrap().day_of_year(), 365);

                // Datetime::valid with a format (datetime.t.cpp 570-571).
                assert!(Datetime::valid_str("2/29/2008", "m/d/Y"), "valid: 2/29/2008");
                assert!(!Datetime::valid_str("2/29/2007", "m/d/Y"), "invalid: 2/29/2007");

                // Comparisons on formatted dates (datetime.t.cpp 538-565).
                let left = Datetime::from_string("7/4/2008", "m/d/Y").unwrap();
                let comp1 = Datetime::from_string("7/4/2008", "m/d/Y").unwrap();
                assert!(left == comp1, "7/4/2008 == 7/4/2008");
                let comp2 = Datetime::from_string("7/5/2008", "m/d/Y").unwrap();
                assert!(left < comp2, "7/4/2008 < 7/5/2008");
                let comp3 = Datetime::from_string("8/4/2008", "m/d/Y").unwrap();
                assert!(left < comp3, "7/4/2008 < 8/4/2008");
                let comp4 = Datetime::from_string("7/4/2009", "m/d/Y").unwrap();
                assert!(left < comp4, "7/4/2008 < 7/4/2009");

                // Depletion requirement.
                let r30 = Datetime::from_string("Mon Jun 30 2014", "a b D Y").unwrap();
                assert_eq!(r30.to_string("YMDHNS"), "20140630000000", "Depletion required on complex format with spaces");
                let r31 = Datetime::from_string("Mon Jun 30 2014 xxx", "a b D Y").unwrap();
                assert_eq!(r31.to_string("YMDHNS"), "20140630000000", "Depletion not required on complex format with spaces");

                // Test all format options (ISO input, no format).
                let r32 = Datetime::from_string("2015-10-28T12:55:00", "").unwrap();
                for (f, e) in [
                    ("Y", "2015"),
                    ("y", "15"),
                    ("M", "10"),
                    ("m", "10"),
                    ("D", "28"),
                    ("d", "28"),
                    ("H", "12"),
                    ("h", "12"),
                    ("N", "55"),
                    ("n", "55"),
                    ("S", "00"),
                    ("s", "0"),
                    ("A", "Wednesday"),
                    ("a", "Wed"),
                    ("B", "October"),
                    ("b", "Oct"),
                    ("V", "44"),
                    ("v", "44"),
                    ("J", "301"),
                    ("j", "301"),
                    ("w", "3"),
                ] {
                    assert_eq!(r32.to_string(f), e, "2015-10-28T12:55:01 -> {f}");
                }

                // Test all parse options.
                let r33 = Datetime::from_string("2015 10 28 19 28 01", "Y M D H N S").unwrap();
                assert_eq!((ymd(&r33), hns(&r33)), ((2015, 10, 28), (19, 28, 1)), "Y M D H N S works");
                let r34 = Datetime::from_string("15 5 4 3 2 1", "y m d h n s").unwrap();
                assert_eq!((ymd(&r34), hns(&r34)), ((2015, 5, 4), (3, 2, 1)), "y m d h n s works");
                let r35 = Datetime::from_string("Wednesday October 28 2015", "A B D Y").unwrap();
                assert_eq!(ymd(&r35), (2015, 10, 28), "A B D Y works");
                assert_eq!(r35.day_of_week(), 3, "A works");
                let r36 = Datetime::from_string("Wed Oct 28 15", "a b d y").unwrap();
                assert_eq!(ymd(&r36), (2015, 10, 28), "a b d y works");
                assert_eq!(r36.day_of_week(), 3, "a works");

                // Test all format options for a future date.
                let r38 = Datetime::from_string("9015-10-28T12:55:00", "").unwrap();
                for (f, e) in [
                    ("Y", "9015"),
                    ("y", "15"),
                    ("M", "10"),
                    ("D", "28"),
                    ("H", "12"),
                    ("N", "55"),
                    ("S", "00"),
                    ("A", "Saturday"),
                    ("a", "Sat"),
                    ("B", "October"),
                    ("V", "43"),
                    ("J", "301"),
                    ("w", "6"),
                ] {
                    assert_eq!(r38.to_string(f), e, "9015-10-28T12:55:01 -> {f}");
                }
            });
        }
    }

    #[test]
    fn parse_formatted_tokens_and_literals() {
        with_tz("Europe/Berlin", || {
            let _settings = SettingsGuard::save();
            Datetime::set_weekstart(1);
            let fields_of = |input: &str, format_: &str| {
                let mut dt = Datetime::new();
                let mut pig = Pig::new(input);
                let ok = dt.parse_formatted(&mut pig, format_);
                (ok, pig.cursor(), dt._year, dt._month, dt._day, dt._seconds)
            };

            // Single-digit tokens take one or two digits.
            assert_eq!(fields_of("2015-1-2", "Y-m-d"), (true, 8, 2015, 1, 2, 0));
            assert_eq!(fields_of("2015-12-31", "Y-m-d"), (true, 10, 2015, 12, 31, 0));
            assert_eq!(fields_of("2015-01-09", "Y-m-d"), (true, 10, 2015, 1, 9, 0));
            // 'm' only reads a second digit after a leading 1: "2" then "5" is a literal mismatch.
            assert!(!fields_of("25/1/2015", "m/d/Y").0);
            // 'd' reads a second digit after 1, 2 or 3 only.
            assert!(!fields_of("2015-1-45", "Y-m-d").0);
            // 'h' reads a second digit after 1 or 2; 'n'/'s' after 0-5.
            assert_eq!(fields_of("2015 23:59:59", "Y h:n:s"), (true, 13, 2015, 1, 1, 86399));
            assert_eq!(fields_of("2015 3:7:9", "Y h:n:s"), (true, 10, 2015, 1, 1, 3 * 3600 + 7 * 60 + 9));
            // Week tokens are consumed (and unused).
            assert_eq!(fields_of("2015 W5", "Y Wv"), (true, 7, 2015, 1, 1, 0));
            assert_eq!(fields_of("2015 W05", "Y WV"), (true, 8, 2015, 1, 1, 0));

            // Missing values: a year with no month defaults to Jan 1, 00:00:00.
            assert_eq!(fields_of("2015", "Y"), (true, 4, 2015, 1, 1, 0));

            // No year: the current year/month/day (and time, if missing) are used.
            let now = Datetime::new();
            let (ok, _, y, m, d, s) = fields_of("12:00", "H:N");
            assert!(ok);
            assert_eq!((y, m, d, s), (now.year(), now.month(), now.day(), 12 * 3600));

            // Trailing text must be whitespace (or end of input).
            assert!(!fields_of("2015-10-28T12:00", "Y-M-D").0);
            assert_eq!(fields_of("2015-10-28 12:00", "Y-M-D"), (true, 10, 2015, 10, 28, 0));

            // A failed token restores the cursor.
            let mut dt = Datetime::new();
            let mut pig = Pig::new("2015-xx-28");
            assert!(!dt.parse_formatted(&mut pig, "Y-M-D"));
            assert_eq!(pig.cursor(), 0);

            // Empty format short-circuits.
            assert!(!fields_of("2015", "").0);

            // Name tokens: invalid names fail; 'A'/'B' at the end of the format read to the end.
            assert!(!fields_of("Foo 2015", "a Y").0);
            assert!(!fields_of("Xyz 2015", "b Y").0);
            assert_eq!(fields_of("2015 February", "Y B"), (true, 13, 2015, 2, 1, 0));
            assert_eq!(fields_of("2015 Thursday", "Y A"), (true, 13, 2015, 1, 1, 0));
            assert!(fields_of("2015 Thurs", "Y A").0, "abbreviated day names are accepted");
            assert!(!fields_of("2015 Frogday", "Y A").0);

            // Literal bytes, including multi-byte UTF-8 literals, match byte by byte.
            assert_eq!(fields_of("28.10.2015", "D.M.Y"), (true, 10, 2015, 10, 28, 0));
            assert_eq!(fields_of("2015年10月28日", "Y年M月D日"), (true, 17, 2015, 10, 28, 0));
            assert!(!fields_of("2015年10月28日", "Y月M月D日").0);

            // Through Datetime::parse: a formatted date that fails validation
            // falls through to the ISO forms.
            let mut dt = Datetime::new();
            let mut start = 0;
            assert!(dt.parse("2015-10-28T12:00", &mut start, "Y-M-D"));
            assert_eq!(start, 16);
            assert_eq!(dt._seconds, 12 * 3600);
            assert!(!Datetime::valid_str("2/30/2015", "m/d/Y"));
        });
    }

    // -------------------------------------------------------------------------
    // Named dates: `parse_named` and every `initialize*` production
    // (Datetime.cpp lines 611-696 and 1327-2927). The named-date parts of
    // datetime.t.cpp and dates.t.cpp are ported in datetime_tests.rs.
    //
    // Expectations are computed from `Datetime::new ()` (the current time)
    // with an independent proleptic-Gregorian day count and converted to an
    // epoch through `Datetime::from_ymd` / `from_ymdhms` (local `mktime`).
    // Each table is evaluated inside `same_second`, so the parse and the
    // reference see the same "now".

    /// Runs `f` until `time (nullptr)` is identical before and after it, so a
    /// parse and its reference computation observe the same current second.
    fn same_second<R, F: FnMut() -> R>(mut f: F) -> R {
        loop {
            let before = localtime::time_now();
            let result = f();
            if localtime::time_now() == before {
                return result;
            }
        }
    }

    /// Days since 1970-01-01 of the proleptic Gregorian date `y-m-d`.
    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }

    /// Inverse of [`days_from_civil`].
    fn civil_from_days(z: i64) -> (i32, i32, i32) {
        let z = z + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        ((if m <= 2 { y + 1 } else { y }) as i32, m as i32, d as i32)
    }

    /// The calendar date `days` days after `y-m-d` (`d` may be out of range).
    fn add_days(y: i32, m: i32, d: i32, days: i32) -> (i32, i32, i32) {
        civil_from_days(days_from_civil(y as i64, m as i64, 1) + (d - 1 + days) as i64)
    }

    /// `y` / `m` moved by `delta` months.
    fn add_months(y: i32, m: i32, delta: i32) -> (i32, i32) {
        let index = y * 12 + (m - 1) + delta;
        (index.div_euclid(12), index.rem_euclid(12) + 1)
    }

    /// Local midnight of `y-m-d + days`.
    fn midnight(y: i32, m: i32, d: i32, days: i32) -> i64 {
        let (y, m, d) = add_days(y, m, d, days);
        Datetime::from_ymd(y, m, d)._date
    }

    /// Local `hh:mm:ss` on `y-m-d + days`.
    fn local_at(y: i32, m: i32, d: i32, days: i32, hh: i32, mm: i32, ss: i32) -> i64 {
        let (y, m, d) = add_days(y, m, d, days);
        Datetime::from_ymdhms(y, m, d, hh, mm, ss)._date
    }

    /// One second before local midnight of `y-m-d + days` (C++ `tm_sec = -1`).
    /// Neither UTC nor America/New_York has a transition at midnight.
    fn end_before(y: i32, m: i32, d: i32, days: i32) -> i64 {
        midnight(y, m, d, days) - 1
    }

    /// The current local date and time, from `Datetime ()`.
    struct Today {
        now: i64,
        y: i32,
        m: i32,
        d: i32,
        /// 0 = Sunday.
        w: i32,
        secs: i32,
    }

    fn today() -> Today {
        let n = Datetime::new();
        Today {
            now: n.to_epoch(),
            y: n.year(),
            m: n.month(),
            d: n.day(),
            w: n.day_of_week(),
            secs: n.hour() * 3600 + n.minute() * 60 + n.second(),
        }
    }

    /// `Datetime::parse (input, start)` from 0: (success, cursor, `_date`).
    fn parse_at(input: &str) -> (bool, usize, i64) {
        let mut dt = Datetime::new();
        let mut start = 0usize;
        let ok = dt.parse(input, &mut start, "");
        (ok, start, dt._date)
    }

    /// One evaluated row: input, parse result, expected epoch.
    type Row = (String, (bool, usize, i64), i64);

    /// Asserts each row parsed completely to its expected epoch.
    fn check_rows(label: &str, rows: &[Row]) {
        for (input, (ok, start, date), expected) in rows {
            assert!(*ok, "{label}: Datetime::parse (\"{input}\") --> true");
            assert_eq!(*start, input.len(), "{label}: Datetime::parse (\"{input}\") --> []");
            assert_eq!(
                *date,
                *expected,
                "{label}: Datetime::parse (\"{input}\") --> {} expected {}",
                Datetime::from_epoch(*date).to_iso_local_extended(),
                Datetime::from_epoch(*expected).to_iso_local_extended()
            );
        }
    }

    /// Runs `f` under each zone with both `timeRelative` settings and both
    /// weekstarts (none of the named dates read `weekstart`).
    fn for_each_setting<F: Fn(&str)>(f: F) {
        for tz in ["UTC", "America/New_York"] {
            with_tz(tz, || {
                let _settings = SettingsGuard::save();
                for relative in [true, false] {
                    for wks in [0, 1] {
                        Datetime::set_time_relative(relative);
                        Datetime::set_weekstart(wks);
                        f(&format!("TZ={tz} timeRelative={relative} weekstart={wks}"));
                    }
                }
            });
        }
    }

    /// Builds rows by parsing each input and pairing it with its expectation.
    fn rows_of(cases: Vec<(&str, i64)>) -> Vec<Row> {
        cases.into_iter().map(|(input, expected)| (input.to_string(), parse_at(input), expected)).collect()
    }

    #[test]
    fn named_now_days_and_later() {
        for_each_setting(|label| {
            let rows = same_second(|| {
                let t = today();
                let (y, m, d) = (t.y, t.m, t.d);
                rows_of(vec![
                    ("now", t.now),
                    ("yes", midnight(y, m, d, -1)),
                    ("yest", midnight(y, m, d, -1)),
                    ("yesterday", midnight(y, m, d, -1)),
                    ("tod", midnight(y, m, d, 0)),
                    ("today", midnight(y, m, d, 0)),
                    ("tom", midnight(y, m, d, 1)),
                    ("tomorrow", midnight(y, m, d, 1)),
                    ("sopd", midnight(y, m, d, -1)),
                    ("sod", midnight(y, m, d, 0)),
                    ("sond", midnight(y, m, d, 1)),
                    ("eopd", end_before(y, m, d, 0)),
                    ("eod", end_before(y, m, d, 1)),
                    ("eond", end_before(y, m, d, 2)),
                    ("lat", Datetime::from_ymd(9999, 12, 30)._date),
                    ("later", Datetime::from_ymd(9999, 12, 30)._date),
                    ("some", Datetime::from_ymd(9999, 12, 30)._date),
                    ("someday", Datetime::from_ymd(9999, 12, 30)._date),
                ])
            });
            check_rows(label, &rows);
        });
    }

    #[test]
    fn named_weeks() {
        for_each_setting(|label| {
            let rows = same_second(|| {
                let t = today();
                let (y, m, d, w) = (t.y, t.m, t.d, t.w);
                // Calendar weeks start on Monday regardless of weekstart.
                let monday = -((w + 6) % 7);
                rows_of(vec![
                    ("sopw", midnight(y, m, d, monday - 7)),
                    ("sow", midnight(y, m, d, monday)),
                    ("sonw", midnight(y, m, d, monday + 7)),
                    ("eopw", end_before(y, m, d, monday)),
                    ("eow", end_before(y, m, d, monday + 7)),
                    ("eonw", end_before(y, m, d, monday + 14)),
                    // Work weeks: Monday (relative to this Sunday) .. Friday.
                    ("sopww", midnight(y, m, d, -6 - w)),
                    ("soww", midnight(y, m, d, 1 - w)),
                    ("sonww", midnight(y, m, d, 8 - w)),
                    ("eopww", end_before(y, m, d, -w - 1)),
                    ("eoww", end_before(y, m, d, 6 - w)),
                    ("eonww", end_before(y, m, d, 13 - w)),
                ])
            });
            check_rows(label, &rows);
        });
    }

    #[test]
    fn named_months_quarters_years() {
        for_each_setting(|label| {
            let rows = same_second(|| {
                let t = today();
                let (y, m) = (t.y, t.m);
                let first = |delta: i32| {
                    let (y, m) = add_months(y, m, delta);
                    Datetime::from_ymd(y, m, 1)._date
                };
                let q = -((m - 1) % 3); // Months back to the quarter start.
                rows_of(vec![
                    ("sopm", first(-1)),
                    ("som", first(0)),
                    ("sonm", first(1)),
                    ("eopm", first(0) - 1),
                    ("eom", first(1) - 1),
                    ("eonm", first(2) - 1),
                    ("sopq", first(q - 3)),
                    ("soq", first(q)),
                    ("sonq", first(q + 3)),
                    ("eopq", first(q) - 1),
                    ("eoq", first(q + 3) - 1),
                    ("eonq", first(q + 6) - 1),
                    ("sopy", Datetime::from_ymd(y - 1, 1, 1)._date),
                    ("soy", Datetime::from_ymd(y, 1, 1)._date),
                    ("sony", Datetime::from_ymd(y + 1, 1, 1)._date),
                    ("eopy", Datetime::from_ymd(y, 1, 1)._date - 1),
                    ("eoy", Datetime::from_ymd(y + 1, 1, 1)._date - 1),
                    ("eony", Datetime::from_ymd(y + 2, 1, 1)._date - 1),
                ])
            });
            check_rows(label, &rows);
        });
    }

    #[test]
    fn named_day_names() {
        let names = [
            ["sunday", "sun", "Sunday", "SUNDAY"],
            ["monday", "mon", "Monday", "MoN"],
            ["tuesday", "tue", "Tuesday", "tues"],
            ["wednesday", "wed", "Wednesday", "wedn"],
            ["thursday", "thu", "Thursday", "thurs"],
            ["friday", "fri", "Friday", "frid"],
            ["saturday", "sat", "Saturday", "satur"],
        ];
        for_each_setting(|label| {
            let rows = same_second(|| {
                let t = today();
                let mut cases = Vec::new();
                for (day, spellings) in names.iter().enumerate() {
                    let day = day as i32;
                    // Relative: the next such day after today (1..7 days ahead);
                    // otherwise: the most recent one, today included.
                    let offset = if Datetime::time_relative() {
                        (day - t.w + 6).rem_euclid(7) + 1
                    } else {
                        -(t.w - day).rem_euclid(7)
                    };
                    for spelling in spellings {
                        cases.push((*spelling, midnight(t.y, t.m, t.d, offset)));
                    }
                }
                rows_of(cases)
            });
            check_rows(label, &rows);
        });
    }

    #[test]
    fn named_month_names() {
        let names = [
            ["january", "jan", "January", "JANU"],
            ["february", "feb", "February", "febr"],
            ["march", "mar", "March", "marc"],
            ["april", "apr", "April", "apri"],
            ["may", "may", "May", "MAY"],
            ["june", "jun", "June", "JUNE"],
            ["july", "jul", "July", "JULY"],
            ["august", "aug", "August", "augu"],
            ["september", "sep", "September", "sept"],
            ["october", "oct", "October", "octo"],
            ["november", "nov", "November", "nove"],
            ["december", "dec", "December", "dece"],
        ];
        for_each_setting(|label| {
            let rows = same_second(|| {
                let t = today();
                let mut cases = Vec::new();
                for (month, spellings) in names.iter().enumerate() {
                    let month = month as i32 + 1;
                    // Relative: months up to and including this one are next year.
                    let year = if Datetime::time_relative() && t.m >= month { t.y + 1 } else { t.y };
                    for spelling in spellings {
                        cases.push((*spelling, Datetime::from_ymd(year, month, 1)._date));
                    }
                }
                rows_of(cases)
            });
            check_rows(label, &rows);
        });
    }

    /// `1st` .. `31st` with the English suffix of each number.
    fn ordinal_spelling(n: i32) -> String {
        let suffix = match (n % 10, n % 100) {
            (_, 11..=13) => "th",
            (1, _) => "st",
            (2, _) => "nd",
            (3, _) => "rd",
            _ => "th",
        };
        format!("{n}{suffix}")
    }

    #[test]
    fn named_ordinals() {
        let spellings: Vec<String> = (1..=31).map(ordinal_spelling).collect();
        for_each_setting(|label| {
            let rows = same_second(|| {
                let t = today();
                let mut cases = Vec::new();
                for (k, spelling) in spellings.iter().enumerate() {
                    let n = k as i32 + 1;
                    let delta = if Datetime::time_relative() && n <= t.d {
                        1
                    } else if !Datetime::time_relative() && t.d < n && n <= Datetime::days_in_month(t.y, t.m) {
                        -1
                    } else {
                        0
                    };
                    let (y, m) = add_months(t.y, t.m, delta);
                    // mktime normalizes e.g. the 31st of a 30-day month.
                    cases.push((spelling.as_str(), midnight(y, m, n, 0)));
                }
                rows_of(cases)
            });
            check_rows(label, &rows);
        });
    }

    /// Easter Sunday (month, day) for 2020-2040.
    const EASTER: [(i32, i32); 21] = [
        (4, 12), // 2020
        (4, 4),
        (4, 17),
        (4, 9),
        (3, 31),
        (4, 20), // 2025
        (4, 5),
        (3, 28),
        (4, 16),
        (4, 1),
        (4, 21), // 2030
        (4, 13),
        (3, 28),
        (4, 17),
        (4, 9),
        (3, 25), // 2035
        (4, 13),
        (4, 5),
        (4, 25),
        (4, 10),
        (4, 1), // 2040
    ];

    fn easter_ymd(year: i32) -> (i32, i32, i32) {
        if (2020..=2040).contains(&year) {
            let (m, d) = EASTER[(year - 2020) as usize];
            (year, m, d)
        } else {
            // Outside the table: the anonymous Gregorian algorithm (checked
            // against known dates by `easter_dates`).
            let dt = Datetime::new();
            let mut t = Tm { tm_year: year - 1900, ..Tm::default() };
            dt.easter(&mut t);
            (year, t.tm_mon + 1, t.tm_mday)
        }
    }

    #[test]
    fn named_easter_and_midsommar() {
        for_each_setting(|label| {
            let rows = same_second(|| {
                let t = today();

                // This year's date unless it is already past (midnight < now).
                let upcoming = |date: &dyn Fn(i32) -> (i32, i32, i32)| {
                    let (y, m, d) = date(t.y);
                    if Datetime::from_ymd(y, m, d)._date < t.now {
                        date(t.y + 1)
                    } else {
                        (y, m, d)
                    }
                };
                let (ey, em, ed) = upcoming(&easter_ymd);
                let midsommar = |year: i32| {
                    (year, 6, 20 + 6 - Datetime::day_of_week_ymd(year, 6, 20)) // Saturday after June 20th.
                };
                let midsommarafton = |year: i32| {
                    (year, 6, 19 + 5 - Datetime::day_of_week_ymd(year, 6, 19)) // Friday after June 19th.
                };
                let (my, mm, md) = upcoming(&midsommar);
                let (ay, am, ad) = upcoming(&midsommarafton);

                rows_of(vec![
                    ("easter", midnight(ey, em, ed, 0)),
                    ("eastermonday", midnight(ey, em, ed, 1)),
                    ("ascension", midnight(ey, em, ed, 39)),
                    ("pentecost", midnight(ey, em, ed, 49)),
                    ("goodfriday", midnight(ey, em, ed, -2)),
                    ("midsommar", midnight(my, mm, md, 0)),
                    ("midsommarafton", midnight(ay, am, ad, 0)),
                    ("juhannus", midnight(ay, am, ad, 0)),
                    // C++ parity: a holiday literal followed by a letter is
                    // not undone before the next holiday is tried.
                    ("easterascension", midnight(ey, em, ed, 39)),
                ])
            });
            check_rows(label, &rows);
        });
    }

    #[test]
    fn named_informal_times() {
        let times: [(&str, i32, i32, i32); 22] = [
            ("8am", 8, 0, 0),
            ("8a", 8, 0, 0),
            ("8:30am", 8, 30, 0),
            ("8:30a", 8, 30, 0),
            ("8:30", 8, 30, 0),
            ("8:30:00", 8, 30, 0),
            ("8:30:15", 8, 30, 15),
            ("8:30:15pm", 20, 30, 15),
            ("8pm", 20, 0, 0),
            ("8p", 20, 0, 0),
            ("8:30pm", 20, 30, 0),
            ("8:30p", 20, 30, 0),
            ("10:30am", 10, 30, 0),
            ("10:30pm", 22, 30, 0),
            ("10:30p", 22, 30, 0),
            ("12am", 0, 0, 0),
            ("12:30am", 0, 30, 0),
            ("12pm", 12, 0, 0),
            ("1pm", 13, 0, 0),
            ("11pm", 23, 0, 0),
            ("3am", 3, 0, 0),
            ("0:00", 0, 0, 0),
        ];
        for_each_setting(|label| {
            let rows = same_second(|| {
                let t = today();
                let cases = times
                    .iter()
                    .map(|&(input, hh, mm, ss)| {
                        // timeRelative: a time already past today is tomorrow.
                        let calc = hh * 3600 + mm * 60 + ss;
                        let days = if Datetime::time_relative() && calc < t.secs { 1 } else { 0 };
                        (input, local_at(t.y, t.m, t.d, days, hh, mm, ss))
                    })
                    .collect();
                rows_of(cases)
            });
            check_rows(label, &rows);
        });
    }

    #[test]
    fn named_cursor_and_rejections() {
        for_each_setting(|label| {
            // Partial embedded parses stop at the first non-alphanumeric.
            for (input, cursor) in [
                ("now+1d", 3),
                ("now-1d", 3),
                ("now)", 3),
                ("yest+1d", 4),
                ("yest-1d", 4),
                ("yest)", 4),
                ("sod+1d", 3),
                ("eow)", 3),
                ("soww ", 4),
                ("monday-1d", 6),
                ("jan+1d", 3),
                ("1st+1d", 3),
                ("easter)", 6),
                ("8am)", 3),
                ("8:30 ", 4),
            ] {
                let (ok, start, _) = parse_at(input);
                assert!(ok, "{label}: Datetime::parse (\"{input}\") --> true");
                assert_eq!(start, cursor, "{label}: Datetime::parse (\"{input}\") --> []");
            }

            for input in [
                "nowadays", "now7", "tomorrov", "yesteryear", "yest7", "to", "ye", "TODAY", "Now", "sodx", "sod7",
                "eoww2", "sowx", "la", "lat3", "so", "su", "ju", "jan:", "jan=", "mon:", "mon=", "mo", "0th",
                "32nd", "1th", "2th", "3th", "11st", "12nd", "13rd", "21th", "1st5", "1stx", "1s", "easterx",
                "eastermondays", "midsommarx", "juhannusx", "pentecost2", "8", "13pm", "25am", "99am", "123am",
                "8:3", "8:30:75", "8amx", "8am:", "8am-", "8am+1d", "8:30-", "8:30+", "25:00", "8:60",
            ] {
                let (ok, start, _) = parse_at(input);
                assert!(!ok, "{label}: Datetime::parse negative '{input}' --> fail (cursor {start})");
            }
        });
    }

    #[test]
    fn named_minimum_match_length_and_later_quirk() {
        with_tz("UTC", || {
            let _settings = SettingsGuard::save();

            Datetime::set_minimum_match_length(4);
            for input in ["yes", "tod", "tom", "sun", "jan", "lat"] {
                assert!(!parse_at(input).0, "minimumMatchLength=4 '{input}' --> fail");
            }
            for input in ["yest", "toda", "tomo", "sund", "janu", "late", "some"] {
                assert!(parse_at(input).0, "minimumMatchLength=4 '{input}' --> success");
            }
            // 'som' is the start of the month, never 'someday' (which needs 4).
            let rows = same_second(|| {
                let t = today();
                rows_of(vec![("som", Datetime::from_ymd(t.y, t.m, 1)._date)])
            });
            check_rows("minimumMatchLength=4", &rows);

            Datetime::set_minimum_match_length(2);
            let rows = same_second(|| {
                let t = today();
                rows_of(vec![("to", midnight(t.y, t.m, t.d, 0)), ("ye", midnight(t.y, t.m, t.d, -1))])
            });
            check_rows("minimumMatchLength=2", &rows);

            Datetime::set_minimum_match_length(3);
            let later = Datetime::from_ymd(9999, 12, 30)._date;
            // 'som' at length 3 is still the start of month.
            let rows = same_second(|| {
                let t = today();
                rows_of(vec![
                    ("som", Datetime::from_ymd(t.y, t.m, 1)._date),
                    // C++ parity: a too-short 'later' prefix is not undone
                    // before 'someday' is tried.
                    ("lasomeday", later),
                ])
            });
            check_rows("minimumMatchLength=3", &rows);
        });
    }

    #[test]
    fn named_parse_named_restores_cursor_on_failure() {
        with_tz("UTC", || {
            for input in ["foo bar", "nowhere", "1stx"] {
                let mut dt = Datetime::new();
                let mut pig = Pig::new(input);
                assert!(!dt.parse_named(&mut pig), "parse_named ('{input}') --> false");
                assert_eq!(pig.cursor(), 0, "parse_named ('{input}') restores the cursor");
            }

            let mut dt = Datetime::new();
            let mut pig = Pig::new("xx today");
            pig.skip_n(3);
            assert!(dt.parse_named(&mut pig));
            assert_eq!(pig.cursor(), 8, "parse_named parses from the current cursor");

            // Phrases are tokenized but the phrase rule is disabled upstream.
            let mut pig = Pig::new("4th thursday in november");
            assert!(dt.parse_named(&mut pig), "'4th' is still an ordinal");
            assert_eq!(pig.cursor(), 3);
        });
    }
}
