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

// -----------------------------------------------------------------------------
// The grammar productions and named-date initializers (Datetime.cpp lines
// 235-2927), in C++ order. Their bodies are ported by the follow-up tasks;
// until then each returns `false`, so only `parse_*`-independent behaviour
// works. Remove the `allow` once every body is filled in.
#[allow(unused_variables, dead_code)]
impl Datetime {
    /// C++ `Datetime::parse_formatted` (Datetime.cpp line 235).
    fn parse_formatted(&mut self, pig: &mut Pig, format: &str) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_named` (Datetime.cpp line 611).
    fn parse_named(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_epoch` (Datetime.cpp line 697).
    fn parse_epoch(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_date_time_ext` (Datetime.cpp line 719).
    fn parse_date_time_ext(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_date_ext` (Datetime.cpp line 742).
    fn parse_date_ext(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_off_ext` (Datetime.cpp line 818).
    fn parse_off_ext(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_time_ext` (Datetime.cpp line 856).
    fn parse_time_ext(&mut self, pig: &mut Pig, terminated: bool) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_time_utc_ext` (Datetime.cpp line 900).
    fn parse_time_utc_ext(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_time_off_ext` (Datetime.cpp line 920).
    fn parse_time_off_ext(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_date_time` (Datetime.cpp line 937).
    fn parse_date_time(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_date` (Datetime.cpp line 959).
    fn parse_date(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_time_utc` (Datetime.cpp line 1031).
    fn parse_time_utc(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_time_off` (Datetime.cpp line 1049).
    fn parse_time_off(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_time` (Datetime.cpp line 1070).
    fn parse_time(&mut self, pig: &mut Pig, terminated: bool) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_off` (Datetime.cpp line 1098).
    fn parse_off(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_year` (Datetime.cpp line 1129).
    fn parse_year(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_month` (Datetime.cpp line 1146).
    fn parse_month(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_week` (Datetime.cpp line 1164).
    fn parse_week(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_julian` (Datetime.cpp line 1182).
    fn parse_julian(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_day` (Datetime.cpp line 1200).
    fn parse_day(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_weekday` (Datetime.cpp line 1218).
    fn parse_weekday(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_hour` (Datetime.cpp line 1236).
    fn parse_hour(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_minute` (Datetime.cpp line 1254).
    fn parse_minute(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_second` (Datetime.cpp line 1272).
    fn parse_second(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_off_hour` (Datetime.cpp line 1290).
    fn parse_off_hour(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::parse_off_minute` (Datetime.cpp line 1308).
    fn parse_off_minute(&mut self, pig: &mut Pig, value: &mut i32) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeNow` (Datetime.cpp line 1327).
    fn initialize_now(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeYesterday` (Datetime.cpp line 1348).
    fn initialize_yesterday(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeToday` (Datetime.cpp line 1377).
    fn initialize_today(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeTomorrow` (Datetime.cpp line 1406).
    fn initialize_tomorrow(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeOrdinal` (Datetime.cpp line 1435).
    fn initialize_ordinal(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeDayName` (Datetime.cpp line 1505).
    fn initialize_day_name(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeMonthName` (Datetime.cpp line 1548).
    fn initialize_month_name(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeLater` (Datetime.cpp line 1590).
    fn initialize_later(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSopd` (Datetime.cpp line 1626).
    fn initialize_sopd(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSod` (Datetime.cpp line 1653).
    fn initialize_sod(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSond` (Datetime.cpp line 1679).
    fn initialize_sond(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEopd` (Datetime.cpp line 1706).
    fn initialize_eopd(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEod` (Datetime.cpp line 1733).
    fn initialize_eod(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEond` (Datetime.cpp line 1761).
    fn initialize_eond(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSopw` (Datetime.cpp line 1789).
    fn initialize_sopw(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSow` (Datetime.cpp line 1819).
    fn initialize_sow(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSonw` (Datetime.cpp line 1848).
    fn initialize_sonw(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEopw` (Datetime.cpp line 1878).
    fn initialize_eopw(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEow` (Datetime.cpp line 1908).
    fn initialize_eow(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEonw` (Datetime.cpp line 1939).
    fn initialize_eonw(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSopww` (Datetime.cpp line 1969).
    fn initialize_sopww(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSoww` (Datetime.cpp line 1996).
    fn initialize_soww(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSonww` (Datetime.cpp line 2023).
    fn initialize_sonww(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEopww` (Datetime.cpp line 2050).
    fn initialize_eopww(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEoww` (Datetime.cpp line 2078).
    fn initialize_eoww(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEonww` (Datetime.cpp line 2106).
    fn initialize_eonww(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSopm` (Datetime.cpp line 2134).
    fn initialize_sopm(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSom` (Datetime.cpp line 2170).
    fn initialize_som(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSonm` (Datetime.cpp line 2197).
    fn initialize_sonm(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEopm` (Datetime.cpp line 2232).
    fn initialize_eopm(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEom` (Datetime.cpp line 2260).
    fn initialize_eom(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEonm` (Datetime.cpp line 2296).
    fn initialize_eonm(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSopq` (Datetime.cpp line 2331).
    fn initialize_sopq(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSoq` (Datetime.cpp line 2366).
    fn initialize_soq(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSonq` (Datetime.cpp line 2394).
    fn initialize_sonq(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEopq` (Datetime.cpp line 2428).
    fn initialize_eopq(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEoq` (Datetime.cpp line 2457).
    fn initialize_eoq(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEonq` (Datetime.cpp line 2492).
    fn initialize_eonq(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSopy` (Datetime.cpp line 2527).
    fn initialize_sopy(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSoy` (Datetime.cpp line 2556).
    fn initialize_soy(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeSony` (Datetime.cpp line 2584).
    fn initialize_sony(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEopy` (Datetime.cpp line 2613).
    fn initialize_eopy(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEoy` (Datetime.cpp line 2642).
    fn initialize_eoy(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEony` (Datetime.cpp line 2672).
    fn initialize_eony(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeEaster` (Datetime.cpp line 2706).
    fn initialize_easter(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeMidsommar` (Datetime.cpp line 2748).
    fn initialize_midsommar(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeMidsommarafton` (Datetime.cpp line 2783).
    fn initialize_midsommarafton(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
        false
    }

    /// C++ `Datetime::initializeInformalTime` (Datetime.cpp line 2826).
    fn initialize_informal_time(&mut self, pig: &mut Pig) -> bool {
        // Ported in the follow-up task
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
    #[allow(dead_code)] // Used by initialize_easter (follow-up task).
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
    #[allow(dead_code)] // Used by initialize_midsommar (follow-up task).
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
    #[allow(dead_code)] // Used by initialize_midsommarafton (follow-up task).
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
mod tests {
    //! Assertions from libshared `test/datetime.t.cpp` that do not need the
    //! (follow-up) grammar productions, plus direct tests of the ported
    //! private helpers. Tests that set `TZ` hold
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
        // Fails until the grammar productions land, and must fail for 'foo'
        // afterwards too.
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
}
