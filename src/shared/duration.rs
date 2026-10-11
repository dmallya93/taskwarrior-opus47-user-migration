//! Rust translation of libshared `src/Duration.h` / `src/Duration.cpp`.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.
//!
//! A `Duration` is a signed number of seconds (`_period`), parsed from raw
//! epoch seconds (`123`), ISO-8601 designated durations (`P1Y2M3DT4H5M6S`,
//! `-PT1H`, `P2W`) or natural units (`3days`, `1.5 hrs`, `monthly`,
//! `weekdays`, ...), and formatted as `format`, `formatHours`, `formatISO`
//! or `formatVague`.
//!
//! API mapping (see the libshared port conventions):
//! - `Duration ()` / `Duration (const std::string&)` / `Duration (time_t)`
//!   become [`Duration::new`], [`Duration::from_string`] and
//!   [`Duration::from_seconds`]; [`Default`] is `Duration ()`.
//! - `operator<`/`>`/`<=`/`>=` compare `_period` only and are provided by
//!   [`PartialOrd`]. C++ has no `operator==`; [`PartialEq`] is provided on
//!   `_period` as well, so that it is consistent with the ordering.
//! - `toString` is the [`std::fmt::Display`] impl (so `.to_string ()` works).
//! - The static `Duration::standaloneSecondsEnabled` is a thread-local
//!   setting ([`Duration::standalone_seconds_enabled`] /
//!   [`Duration::set_standalone_seconds_enabled`]), the project's pattern for
//!   C++ statics.
//! - The private `dump` is declared in `Duration.h` but never defined in
//!   `Duration.cpp`, so it has no counterpart here.
//!
//! Integer conversions mirror the C++ (`int` fields, `time_t` seconds and
//! period): `long long` → `int` assignments truncate, and the `int`
//! arithmetic in `resolve` wraps, as on the reference platforms (where the
//! C++ overflow is formally undefined).

use std::cell::Cell;
use std::cmp::Ordering;
use std::fmt;
use std::sync::OnceLock;

use crate::shared::format::printf_f;
use crate::shared::pig::Pig;
use crate::shared::unicode::{unicode_latin_alpha, unicode_latin_digit};

thread_local! {
    /// C++ `bool Duration::standaloneSecondsEnabled = true;`
    static STANDALONE_SECONDS_ENABLED: Cell<bool> = const { Cell::new(true) };
}

const DAY: i32 = 86400;
const HOUR: i32 = 3600;
const MINUTE: i32 = 60;
const SECOND: i32 = 1;

/// One entry of the C++ `durations[]` table.
struct DurationUnit {
    unit: &'static str,
    seconds: i32,
    standalone: bool,
}

const fn unit(unit: &'static str, seconds: i32, standalone: bool) -> DurationUnit {
    DurationUnit {
        unit,
        seconds,
        standalone,
    }
}

/// C++ `durations[]`.
///
/// These are sorted by first character, then length, so that Pig::getOneOf
/// returns a maximal match.
#[rustfmt::skip]
static DURATIONS: [DurationUnit; 56] = [
    unit("annual",     365 * DAY,    true ),
    unit("biannual",   730 * DAY,    true ),
    unit("bimonthly",   61 * DAY,    true ),
    unit("biweekly",    14 * DAY,    true ),
    unit("biyearly",   730 * DAY,    true ),
    unit("daily",        DAY,        true ),
    unit("days",         DAY,        false),
    unit("day",          DAY,        true ),
    unit("d",            DAY,        false),
    unit("fortnight",   14 * DAY,    true ),
    unit("hours",        HOUR,       false),
    unit("hour",         HOUR,       true ),
    unit("hrs",          HOUR,       false),
    unit("hr",           HOUR,       true ),
    unit("h",            HOUR,       false),
    unit("minutes",      MINUTE,     false),
    unit("minute",       MINUTE,     true ),
    unit("mins",         MINUTE,     false),
    unit("min",          MINUTE,     true ),
    unit("monthly",     30 * DAY,    true ),
    unit("months",      30 * DAY,    false),
    unit("month",       30 * DAY,    true ),
    unit("mnths",       30 * DAY,    false),
    unit("mths",        30 * DAY,    false),
    unit("mth",         30 * DAY,    true ),
    unit("mos",         30 * DAY,    false),
    unit("mo",          30 * DAY,    true ),
    unit("m",           30 * DAY,    false),
    unit("quarterly",   91 * DAY,    true ),
    unit("quarters",    91 * DAY,    false),
    unit("quarter",     91 * DAY,    true ),
    unit("qrtrs",       91 * DAY,    false),
    unit("qrtr",        91 * DAY,    true ),
    unit("qtrs",        91 * DAY,    false),
    unit("qtr",         91 * DAY,    true ),
    unit("q",           91 * DAY,    false),
    unit("semiannual", 183 * DAY,    true ),
    unit("sennight",    14 * DAY,    false),
    unit("seconds",      SECOND,     false),
    unit("second",       SECOND,     true ),
    unit("secs",         SECOND,     false),
    unit("sec",          SECOND,     true ),
    unit("s",            SECOND,     false),
    unit("weekdays",     DAY,        true ),
    unit("weekly",       7 * DAY,    true ),
    unit("weeks",        7 * DAY,    false),
    unit("week",         7 * DAY,    true ),
    unit("wks",          7 * DAY,    false),
    unit("wk",           7 * DAY,    true ),
    unit("w",            7 * DAY,    false),
    unit("yearly",     365 * DAY,    true ),
    unit("years",      365 * DAY,    false),
    unit("year",       365 * DAY,    true ),
    unit("yrs",        365 * DAY,    false),
    unit("yr",         365 * DAY,    true ),
    unit("y",          365 * DAY,    false),
];

/// C++ `unicodeLatinAlpha (pig.peek ())`: the signed `char` is passed as an
/// `unsigned int`.
fn latin_alpha(c: i32) -> bool {
    unicode_latin_alpha(c as u32)
}

/// C++ `unicodeLatinDigit (pig.peek ())`.
fn latin_digit(c: i32) -> bool {
    unicode_latin_digit(c as u32)
}

/// C++ `class Duration`.
#[derive(Debug, Clone, Copy)]
pub struct Duration {
    pub _year: i32,
    pub _month: i32,
    pub _weeks: i32,
    pub _day: i32,
    pub _hours: i32,
    pub _minutes: i32,
    pub _seconds: i64,
    pub _period: i64,
}

impl Default for Duration {
    fn default() -> Self {
        Duration::new()
    }
}

impl Duration {
    /// C++ `Duration::Duration ()`.
    pub fn new() -> Self {
        Duration {
            _year: 0,
            _month: 0,
            _weeks: 0,
            _day: 0,
            _hours: 0,
            _minutes: 0,
            _seconds: 0,
            _period: 0,
        }
    }

    /// C++ `Duration::Duration (const std::string&)`: parses `input` from
    /// the start, ignoring failure (the result is then zero).
    pub fn from_string(input: &str) -> Self {
        let mut d = Duration::new();
        d.clear();
        let mut idx = 0;
        d.parse(input, &mut idx);
        d
    }

    /// C++ `Duration::Duration (time_t)`.
    pub fn from_seconds(input: i64) -> Self {
        let mut d = Duration::new();
        d.clear();
        d._period = input;
        d
    }

    /// C++ `Duration::standaloneSecondsEnabled` (read).
    pub fn standalone_seconds_enabled() -> bool {
        STANDALONE_SECONDS_ENABLED.with(|c| c.get())
    }

    /// C++ `Duration::standaloneSecondsEnabled = value`.
    pub fn set_standalone_seconds_enabled(value: bool) {
        STANDALONE_SECONDS_ENABLED.with(|c| c.set(value));
    }

    /// C++ `Duration::toTime_t`.
    pub fn to_time_t(&self) -> i64 {
        self._period
    }

    /// C++ `Duration::parse (const std::string&, std::string::size_type&)`.
    ///
    /// As in C++, a non-zero `start` is skipped with `Pig::skipN`, i.e. as a
    /// count of UTF-8 characters, while the updated `start` is the Pig's byte
    /// cursor. On failure `start` is left untouched.
    pub fn parse(&mut self, input: &str, start: &mut usize) -> bool {
        let i = *start;
        let mut pig = Pig::new(input);
        if i != 0 {
            pig.skip_n(i as i32);
        }

        if Duration::standalone_seconds_enabled() && self.parse_seconds(&mut pig) {
            // ::resolve is not needed in this case.
            *start = pig.cursor();
            return true;
        } else if self.parse_designated(&mut pig)
            || self.parse_weeks(&mut pig)
            || self.parse_units(&mut pig)
        {
            *start = pig.cursor();
            self.resolve();
            return true;
        }

        false
    }

    /// C++ `Duration::parse_seconds`: a raw number of seconds, which must be
    /// 0 or greater than 60, and not followed by a Latin letter.
    pub fn parse_seconds(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        let mut epoch: i64 = 0;
        if pig.get_digits_i64(&mut epoch) && !latin_alpha(pig.peek()) && (epoch == 0 || epoch > 60)
        {
            self._period = epoch;
            return true;
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Duration::parse_designated`:
    /// `'P' [nn 'Y'] [nn 'M'] [nn 'D'] ['T' [nn 'H'] [nn 'M'] [nn 'S']]`,
    /// optionally preceded by `-`.
    pub fn parse_designated(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        // sign = -1 if a '-' is present, else 1
        let sign: i64 = if pig.skip('-') { -1 } else { 1 };

        if pig.skip('P') && !pig.eos() {
            let mut value: i64 = 0;
            pig.save();
            if pig.get_digits_i64(&mut value) && pig.skip('Y') {
                self._year = sign.wrapping_mul(value) as i32;
            } else {
                pig.restore();
            }

            pig.save();
            if pig.get_digits_i64(&mut value) && pig.skip('M') {
                self._month = sign.wrapping_mul(value) as i32;
            } else {
                pig.restore();
            }

            pig.save();
            if pig.get_digits_i64(&mut value) && pig.skip('D') {
                self._day = sign.wrapping_mul(value) as i32;
            } else {
                pig.restore();
            }

            if pig.skip('T') && !pig.eos() {
                pig.save();
                if pig.get_digits_i64(&mut value) && pig.skip('H') {
                    self._hours = sign.wrapping_mul(value) as i32;
                } else {
                    pig.restore();
                }

                pig.save();
                if pig.get_digits_i64(&mut value) && pig.skip('M') {
                    self._minutes = sign.wrapping_mul(value) as i32;
                } else {
                    pig.restore();
                }

                pig.save();
                if pig.get_digits_i64(&mut value) && pig.skip('S') {
                    self._seconds = sign.wrapping_mul(value);
                } else {
                    pig.restore();
                }
            }

            let following = pig.peek();
            if pig.cursor() - checkpoint >= 3 && !latin_alpha(following) && !latin_digit(following)
            {
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Duration::parse_weeks`: `'P' [nn 'W']`.
    pub fn parse_weeks(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        if pig.skip('P') && !pig.eos() {
            let mut value: i64 = 0;
            pig.save();
            if pig.get_digits_i64(&mut value) && pig.skip('W') {
                self._weeks = value as i32;
            } else {
                pig.restore();
            }

            let following = pig.peek();
            if pig.cursor() - checkpoint >= 3 && !latin_alpha(following) && !latin_digit(following)
            {
                return true;
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Duration::parse_units`: a standalone unit (`weekly`), or a
    /// decimal quantity, optional whitespace, and a unit (`1.5 hrs`).
    pub fn parse_units(&mut self, pig: &mut Pig) -> bool {
        let checkpoint = pig.cursor();

        // Static and so preserved between calls.
        static UNITS: OnceLock<Vec<&'static str>> = OnceLock::new();
        let units = UNITS.get_or_init(|| DURATIONS.iter().map(|d| d.unit).collect());

        let mut number: f64 = 0.0;
        let mut unit = String::new();
        if pig.get_one_of(units, &mut unit) {
            let following = pig.peek();
            if !latin_alpha(following) && !latin_digit(following) {
                for d in DURATIONS.iter() {
                    if d.unit == unit && d.standalone {
                        self._period = d.seconds as i64;
                        return true;
                    }
                }
            } else {
                pig.restore_to(checkpoint);
            }
        } else if pig.get_decimal_f64(&mut number) {
            pig.skip_ws();
            if pig.get_one_of(units, &mut unit) {
                // The "d" unit is a special case, because it is the only one that can
                // legitimately occur at the beginning of a UUID, and be followed by an
                // operator:
                //
                //   1111111d-0000-0000-0000-000000000000
                //
                // Because Lexer::isDuration is higher precedence than Lexer::isUUID,
                // the above UUID looks like:
                //
                //   <1111111d> <-> ...
                //   duration   op  ...
                //
                // So as a special case, durations, with units of "d" are rejected if the
                // quantity exceeds 10000.
                //
                if unit == "d" && number > 10000.0 {
                    pig.restore_to(checkpoint);
                    return false;
                }

                let following = pig.peek();
                if !latin_alpha(following) && !latin_digit(following) {
                    // Linear lookup - should instead be logarithmic.
                    for d in DURATIONS.iter() {
                        if d.unit == unit {
                            let seconds = d.seconds as f64;
                            self._period = (number * seconds) as i64;
                            return true;
                        }
                    }
                }
            }
        }

        pig.restore_to(checkpoint);
        false
    }

    /// C++ `Duration::clear` (private in C++).
    pub fn clear(&mut self) {
        self._year = 0;
        self._month = 0;
        self._weeks = 0;
        self._day = 0;
        self._hours = 0;
        self._minutes = 0;
        self._seconds = 0;
        self._period = 0;
    }

    /// C++ `Duration::format`: `[-][<days>d ]<h>:<mm>:<ss>`.
    pub fn format(&self) -> String {
        if self._period != 0 {
            let mut t = self._period;

            let mut s = String::new();
            if t < 0 {
                s.push('-');
                t = t.wrapping_neg();
            }

            let seconds = (t % 60) as i32;
            t /= 60;
            let minutes = (t % 60) as i32;
            t /= 60;
            let hours = (t % 24) as i32;
            t /= 24;
            let days = t as i32;

            if days != 0 {
                s.push_str(&format!("{days}d "));
            }

            s.push_str(&format!("{hours}:{minutes:02}:{seconds:02}"));
            s
        } else {
            "0:00:00".to_string()
        }
    }

    /// C++ `Duration::formatHours`: `[-]<h>:<mm>:<ss>`, hours unbounded.
    pub fn format_hours(&self) -> String {
        if self._period != 0 {
            let mut t = self._period;

            let mut s = String::new();
            if t < 0 {
                s.push('-');
                t = t.wrapping_neg();
            }

            let seconds = (t % 60) as i32;
            t /= 60;
            let minutes = (t % 60) as i32;
            t /= 60;
            let hours = t as i32;

            s.push_str(&format!("{hours}:{minutes:02}:{seconds:02}"));
            s
        } else {
            "0:00:00".to_string()
        }
    }

    /// C++ `Duration::formatISO`: `[-]P[<n>D][T[<n>H][<n>M][<n>S]]`.
    pub fn format_iso(&self) -> String {
        if self._period != 0 {
            let mut t = self._period;

            let mut s = String::new();
            if t < 0 {
                s.push('-');
                t = t.wrapping_neg();
            }

            let seconds = (t % 60) as i32;
            t /= 60;
            let minutes = (t % 60) as i32;
            t /= 60;
            let hours = (t % 24) as i32;
            t /= 24;
            let days = t as i32;

            s.push('P');
            if days != 0 {
                s.push_str(&format!("{days}D"));
            }

            if hours != 0 || minutes != 0 || seconds != 0 {
                s.push('T');
                if hours != 0 {
                    s.push_str(&format!("{hours}H"));
                }
                if minutes != 0 {
                    s.push_str(&format!("{minutes}M"));
                }
                if seconds != 0 {
                    s.push_str(&format!("{seconds}S"));
                }
            }

            s
        } else {
            "PT0S".to_string()
        }
    }

    /// C++ `Duration::formatVague (bool padding = false)`.
    ///
    /// ```text
    /// Range      Representation
    /// ---------  ---------------------
    /// >= 365d    {n.n}y
    /// >= 90d     {n}mo
    /// >= 14d     {n}w
    /// >= 1d      {n}d
    /// >= 1h      {n}h
    /// >= 1min    {n}min
    ///            {n}s
    /// ```
    ///
    /// As in C++, `days` is a `float` (computed in `double`, then narrowed),
    /// the year count is a `float` division streamed with `std::fixed` and
    /// `std::setprecision (1)`, and the other counts truncate to `int`.
    pub fn format_vague(&self, padding: bool) -> String {
        let mut t = self._period;
        let mut days: f32 = ((self._period as f32) as f64 / 86400.0) as f32;

        let mut formatted = String::new();
        if t < 0 {
            formatted.push('-');
            t = t.wrapping_neg();
            days = (-(days as f64)) as f32; // C++ `days *= -1.0`
        }

        if t >= 86400 * 365 {
            formatted.push_str(&printf_f(f64::from(days / 365.0), 1));
            formatted.push_str(if padding { "y  " } else { "y" });
        } else if t >= 86400 * 90 {
            formatted.push_str(&((days / 30.0) as i32).to_string());
            formatted.push_str(if padding { "mo " } else { "mo" });
        } else if t >= 86400 * 14 {
            formatted.push_str(&((days / 7.0) as i32).to_string());
            formatted.push_str(if padding { "w  " } else { "w" });
        } else if t >= 86400 {
            formatted.push_str(&(days as i32).to_string());
            formatted.push_str(if padding { "d  " } else { "d" });
        } else if t >= 3600 {
            formatted.push_str(&((t / 3600) as i32).to_string());
            formatted.push_str(if padding { "h  " } else { "h" });
        } else if t >= 60 {
            formatted.push_str(&((t / 60) as i32).to_string());
            formatted.push_str("min"); // Longest suffix - no padding
        } else if t >= 1 {
            formatted.push_str(&(t as i32).to_string());
            formatted.push_str(if padding { "s  " } else { "s" });
        }

        formatted
    }

    /// C++ `Duration::days`.
    pub fn days(&self) -> i32 {
        (self._period / 86400) as i32
    }

    /// C++ `Duration::hours`.
    pub fn hours(&self) -> i32 {
        (self._period / 3600) as i32
    }

    /// C++ `Duration::minutes`.
    pub fn minutes(&self) -> i32 {
        (self._period / 60) as i32
    }

    /// C++ `Duration::seconds`.
    pub fn seconds(&self) -> i64 {
        self._period
    }

    /// C++ `Duration::resolve` (private in C++): allow un-normalized values.
    ///
    /// The year/month/day/hour/minute terms are C++ `int` arithmetic (summed
    /// as `int`, then added to the `time_t` seconds); they wrap here.
    pub fn resolve(&mut self) {
        if self._period == 0 {
            if self._weeks != 0 {
                self._period = self._weeks.wrapping_mul(7).wrapping_mul(86400) as i64;
            } else {
                let ints = self
                    ._year
                    .wrapping_mul(365)
                    .wrapping_mul(86400)
                    .wrapping_add(self._month.wrapping_mul(30).wrapping_mul(86400))
                    .wrapping_add(self._day.wrapping_mul(86400))
                    .wrapping_add(self._hours.wrapping_mul(3600))
                    .wrapping_add(self._minutes.wrapping_mul(60));
                self._period = (ints as i64).wrapping_add(self._seconds);
            }
        }
    }
}

/// C++ `Duration::toString`: `_period` in decimal.
impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self._period)
    }
}

/// Not in C++; equality on `_period`, consistent with the C++ ordering
/// operators.
impl PartialEq for Duration {
    fn eq(&self, other: &Self) -> bool {
        self._period == other._period
    }
}

impl Eq for Duration {}

/// C++ `operator<`, `operator>`, `operator<=`, `operator>=`: compare
/// `_period`.
impl PartialOrd for Duration {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Duration {
    fn cmp(&self, other: &Self) -> Ordering {
        self._period.cmp(&other._period)
    }
}

#[cfg(test)]
mod tests {
    //! Port of libshared `test/duration.t.cpp` (2229 assertions). Each C++
    //! `t.ok`/`t.notok`/`t.is` is one assertion here, carrying the C++ test
    //! description as its message; `t.diag` lines are printed.
    //!
    //! `standaloneSecondsEnabled` is thread-local and every `#[test]` runs on
    //! its own thread, so tests that change it cannot affect other tests; they
    //! still restore it (via `StandaloneGuard`) before returning.

    use super::*;

    /// Restores `standaloneSecondsEnabled` on drop.
    struct StandaloneGuard(bool);

    impl StandaloneGuard {
        fn save() -> Self {
            StandaloneGuard(Duration::standalone_seconds_enabled())
        }
    }

    impl Drop for StandaloneGuard {
        fn drop(&mut self) {
            Duration::set_standalone_seconds_enabled(self.0);
        }
    }

    /// One `testParse (t, input, in_start, ...)` row: input, start, year,
    /// month, weeks, day, hours, minutes, seconds, period, format,
    /// formatHours, formatISO, formatVague.
    type Row = (
        &'static str,
        usize,
        i32,
        i32,
        i32,
        i32,
        i32,
        i32,
        i64,
        i64,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    );

    /// C++ `testParse (UnitTest&, input, in_start, ...)` (14 assertions).
    #[track_caller]
    fn test_parse_full(row: &Row) {
        let (
            input,
            in_start,
            in_year,
            in_month,
            in_weeks,
            in_day,
            in_hours,
            in_minutes,
            in_seconds,
            in_period,
            output,
            hours,
            iso,
            vague,
        ) = *row;
        let label = format!("parse (\"{input}\") --> ");

        let mut dur = Duration::new();
        let mut start: usize = 0;

        assert!(dur.parse(input, &mut start), "{label}true");
        assert_eq!(start, in_start, "{label}[]");
        assert_eq!(dur._year, in_year, "{label}_year");
        assert_eq!(dur._month, in_month, "{label}_month");
        assert_eq!(dur._weeks, in_weeks, "{label}_weeks");
        assert_eq!(dur._day, in_day, "{label}_day");
        assert_eq!(dur._hours, in_hours, "{label}_hours");
        assert_eq!(dur._minutes, in_minutes, "{label}_minutes");
        assert_eq!(dur._seconds, in_seconds, "{label}_seconds");
        assert_eq!(dur._period as usize, in_period as usize, "{label}_period");
        assert_eq!(dur.format(), output, "{label} format");
        assert_eq!(dur.format_hours(), hours, "{label} formatHours");
        assert_eq!(dur.format_iso(), iso, "{label} formatISO");
        assert_eq!(dur.format_vague(false), vague, "{label} formatVague");
    }

    /// C++ `testParse (UnitTest&, input)`.
    #[track_caller]
    fn test_parse(input: &str) {
        let label = format!("Duration::parse positive '{input}' --> success");

        let mut positive = Duration::new();
        let mut pos: usize = 0;
        assert!(positive.parse(input, &mut pos) && pos != 0, "{label}");
    }

    /// C++ `testParseError (UnitTest&, input)`.
    #[track_caller]
    fn test_parse_error(input: &str) {
        let label = format!("Duration::parse negative '{input}' --> fail");

        let mut neg = Duration::new();
        let mut pos: usize = 0;
        assert!(!neg.parse(input, &mut pos), "{label}");
    }

    fn d(input: &str) -> Duration {
        Duration::from_string(input)
    }

    #[test]
    fn simple_negative() {
        // Simple negative tests.
        test_parse_error("foo");
        test_parse_error("P");
        test_parse_error("PT");
        test_parse_error("P1");
        test_parse_error("P1T");
        test_parse_error("PT1");
    }

    #[test]
    #[rustfmt::skip]
    fn parse_table() {
        let year: i64 = 365 * 86400;
        let month: i64 = 30 * 86400;
        let day: i64 = 86400;
        let h: i64 = 3600;
        let m: i64 = 60;

        //   input  i  Year  Mo  We  Da  Ho  Mi  Se  time_t  format  hours  iso  vague
        let rows: Vec<Row> = vec![
            ("P1Y", 3, 1, 0, 0, 0, 0, 0, 0, year, "365d 0:00:00", "8760:00:00", "P365D", "1.0y"),
            ("P1M", 3, 0, 1, 0, 0, 0, 0, 0, month, "30d 0:00:00", "720:00:00", "P30D", "4w"),
            ("P1D", 3, 0, 0, 0, 1, 0, 0, 0, day, "1d 0:00:00", "24:00:00", "P1D", "1d"),
            ("P1Y1M", 5, 1, 1, 0, 0, 0, 0, 0, year + month, "395d 0:00:00", "9480:00:00", "P395D", "1.1y"),
            ("P1Y1D", 5, 1, 0, 0, 1, 0, 0, 0, year + day, "366d 0:00:00", "8784:00:00", "P366D", "1.0y"),
            ("P1M1D", 5, 0, 1, 0, 1, 0, 0, 0, month + day, "31d 0:00:00", "744:00:00", "P31D", "4w"),
            ("P1Y1M1D", 7, 1, 1, 0, 1, 0, 0, 0, year + month + day, "396d 0:00:00", "9504:00:00", "P396D", "1.1y"),
            ("PT1H", 4, 0, 0, 0, 0, 1, 0, 0, h, "1:00:00", "1:00:00", "PT1H", "1h"),
            ("PT1M", 4, 0, 0, 0, 0, 0, 1, 0, m, "0:01:00", "0:01:00", "PT1M", "1min"),
            ("PT1S", 4, 0, 0, 0, 0, 0, 0, 1, 1, "0:00:01", "0:00:01", "PT1S", "1s"),
            ("PT1H1M", 6, 0, 0, 0, 0, 1, 1, 0, h + m, "1:01:00", "1:01:00", "PT1H1M", "1h"),
            ("PT1H1S", 6, 0, 0, 0, 0, 1, 0, 1, h + 1, "1:00:01", "1:00:01", "PT1H1S", "1h"),
            ("PT1M1S", 6, 0, 0, 0, 0, 0, 1, 1, m + 1, "0:01:01", "0:01:01", "PT1M1S", "1min"),
            ("PT1H1M1S", 8, 0, 0, 0, 0, 1, 1, 1, h + m + 1, "1:01:01", "1:01:01", "PT1H1M1S", "1h"),
            ("P1Y1M1DT1H1M1S", 14, 1, 1, 0, 1, 1, 1, 1, year + month + day + h + m + 1, "396d 1:01:01", "9505:01:01", "P396DT1H1M1S", "1.1y"),
            ("PT24H", 5, 0, 0, 0, 0, 24, 0, 0, day, "1d 0:00:00", "24:00:00", "P1D", "1d"),
            ("PT40000000S", 11, 0, 0, 0, 0, 0, 0, 40000000, 40000000, "462d 23:06:40", "11111:06:40", "P462DT23H6M40S", "1.3y"),
            ("PT3600S", 7, 0, 0, 0, 0, 0, 0, 3600, h, "1:00:00", "1:00:00", "PT1H", "1h"),
            ("PT60M", 5, 0, 0, 0, 0, 0, 60, 0, h, "1:00:00", "1:00:00", "PT1H", "1h"),
            ("0seconds", 8, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("-P1Y", 4, -1, 0, 0, 0, 0, 0, 0, -year, "-365d 0:00:00", "-8760:00:00", "-P365D", "-1.0y"),
            ("-P1M", 4, 0, -1, 0, 0, 0, 0, 0, -month, "-30d 0:00:00", "-720:00:00", "-P30D", "-4w"),
            ("-P1D", 4, 0, 0, 0, -1, 0, 0, 0, -day, "-1d 0:00:00", "-24:00:00", "-P1D", "-1d"),
            ("-P1Y1M", 6, -1, -1, 0, 0, 0, 0, 0, -(year + month), "-395d 0:00:00", "-9480:00:00", "-P395D", "-1.1y"),
            ("-P1Y1D", 6, -1, 0, 0, -1, 0, 0, 0, -(year + day), "-366d 0:00:00", "-8784:00:00", "-P366D", "-1.0y"),
            ("-P1M1D", 6, 0, -1, 0, -1, 0, 0, 0, -(month + day), "-31d 0:00:00", "-744:00:00", "-P31D", "-4w"),
            ("-P1Y1M1D", 8, -1, -1, 0, -1, 0, 0, 0, -(year + month + day), "-396d 0:00:00", "-9504:00:00", "-P396D", "-1.1y"),
            ("-PT1H", 5, 0, 0, 0, 0, -1, 0, 0, -h, "-1:00:00", "-1:00:00", "-PT1H", "-1h"),
            ("-PT1M", 5, 0, 0, 0, 0, 0, -1, 0, -m, "-0:01:00", "-0:01:00", "-PT1M", "-1min"),
            ("-PT1S", 5, 0, 0, 0, 0, 0, 0, -1, -1, "-0:00:01", "-0:00:01", "-PT1S", "-1s"),
            ("-PT1H1M", 7, 0, 0, 0, 0, -1, -1, 0, -(h + m), "-1:01:00", "-1:01:00", "-PT1H1M", "-1h"),
            ("-PT1H1S", 7, 0, 0, 0, 0, -1, 0, -1, -(h + 1), "-1:00:01", "-1:00:01", "-PT1H1S", "-1h"),
            ("-PT1M1S", 7, 0, 0, 0, 0, 0, -1, -1, -(m + 1), "-0:01:01", "-0:01:01", "-PT1M1S", "-1min"),
            ("-PT1H1M1S", 9, 0, 0, 0, 0, -1, -1, -1, -(h + m + 1), "-1:01:01", "-1:01:01", "-PT1H1M1S", "-1h"),
            ("-P1Y1M1DT1H1M1S", 15, -1, -1, 0, -1, -1, -1, -1, -(year + month + day + h + m + 1), "-396d 1:01:01", "-9505:01:01", "-P396DT1H1M1S", "-1.1y"),
            ("-PT24H", 6, 0, 0, 0, 0, -24, 0, 0, -day, "-1d 0:00:00", "-24:00:00", "-P1D", "-1d"),
            ("-PT40000000S", 12, 0, 0, 0, 0, 0, 0, -40000000, -40000000, "-462d 23:06:40", "-11111:06:40", "-P462DT23H6M40S", "-1.3y"),
            ("-PT3600S", 8, 0, 0, 0, 0, 0, 0, -3600, -h, "-1:00:00", "-1:00:00", "-PT1H", "-1h"),
            ("-PT60M", 6, 0, 0, 0, 0, 0, -60, 0, -h, "-1:00:00", "-1:00:00", "-PT1H", "-1h"),
            ("2 seconds", 9, 0, 0, 0, 0, 0, 0, 0, 2, "0:00:02", "0:00:02", "PT2S", "2s"),
            ("10seconds", 9, 0, 0, 0, 0, 0, 0, 0, 10, "0:00:10", "0:00:10", "PT10S", "10s"),
            ("1.5seconds", 10, 0, 0, 0, 0, 0, 0, 0, 1, "0:00:01", "0:00:01", "PT1S", "1s"),
            ("0second", 7, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 second", 8, 0, 0, 0, 0, 0, 0, 0, 2, "0:00:02", "0:00:02", "PT2S", "2s"),
            ("10second", 8, 0, 0, 0, 0, 0, 0, 0, 10, "0:00:10", "0:00:10", "PT10S", "10s"),
            ("1.5second", 9, 0, 0, 0, 0, 0, 0, 0, 1, "0:00:01", "0:00:01", "PT1S", "1s"),
            ("0s", 2, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 s", 3, 0, 0, 0, 0, 0, 0, 0, 2, "0:00:02", "0:00:02", "PT2S", "2s"),
            ("10s", 3, 0, 0, 0, 0, 0, 0, 0, 10, "0:00:10", "0:00:10", "PT10S", "10s"),
            ("1.5s", 4, 0, 0, 0, 0, 0, 0, 0, 1, "0:00:01", "0:00:01", "PT1S", "1s"),
            ("0minutes", 8, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 minutes", 9, 0, 0, 0, 0, 0, 0, 0, 2 * m, "0:02:00", "0:02:00", "PT2M", "2min"),
            ("10minutes", 9, 0, 0, 0, 0, 0, 0, 0, 10 * m, "0:10:00", "0:10:00", "PT10M", "10min"),
            ("1.5minutes", 10, 0, 0, 0, 0, 0, 0, 0, m + 30, "0:01:30", "0:01:30", "PT1M30S", "1min"),
            ("0minute", 7, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 minute", 8, 0, 0, 0, 0, 0, 0, 0, 2 * m, "0:02:00", "0:02:00", "PT2M", "2min"),
            ("10minute", 8, 0, 0, 0, 0, 0, 0, 0, 10 * m, "0:10:00", "0:10:00", "PT10M", "10min"),
            ("1.5minute", 9, 0, 0, 0, 0, 0, 0, 0, m + 30, "0:01:30", "0:01:30", "PT1M30S", "1min"),
            ("0min", 4, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 min", 5, 0, 0, 0, 0, 0, 0, 0, 2 * m, "0:02:00", "0:02:00", "PT2M", "2min"),
            ("10min", 5, 0, 0, 0, 0, 0, 0, 0, 10 * m, "0:10:00", "0:10:00", "PT10M", "10min"),
            ("1.5min", 6, 0, 0, 0, 0, 0, 0, 0, m + 30, "0:01:30", "0:01:30", "PT1M30S", "1min"),
            ("0hours", 6, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 hours", 7, 0, 0, 0, 0, 0, 0, 0, 2 * h, "2:00:00", "2:00:00", "PT2H", "2h"),
            ("10hours", 7, 0, 0, 0, 0, 0, 0, 0, 10 * h, "10:00:00", "10:00:00", "PT10H", "10h"),
            ("1.5hours", 8, 0, 0, 0, 0, 0, 0, 0, h + 30 * m, "1:30:00", "1:30:00", "PT1H30M", "1h"),
            ("0hour", 5, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 hour", 6, 0, 0, 0, 0, 0, 0, 0, 2 * h, "2:00:00", "2:00:00", "PT2H", "2h"),
            ("10hour", 6, 0, 0, 0, 0, 0, 0, 0, 10 * h, "10:00:00", "10:00:00", "PT10H", "10h"),
            ("1.5hour", 7, 0, 0, 0, 0, 0, 0, 0, h + 30 * m, "1:30:00", "1:30:00", "PT1H30M", "1h"),
            ("0h", 2, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 h", 3, 0, 0, 0, 0, 0, 0, 0, 2 * h, "2:00:00", "2:00:00", "PT2H", "2h"),
            ("10h", 3, 0, 0, 0, 0, 0, 0, 0, 10 * h, "10:00:00", "10:00:00", "PT10H", "10h"),
            ("1.5h", 4, 0, 0, 0, 0, 0, 0, 0, h + 30 * m, "1:30:00", "1:30:00", "PT1H30M", "1h"),
            ("weekdays", 8, 0, 0, 0, 0, 0, 0, 0, day, "1d 0:00:00", "24:00:00", "P1D", "1d"),
            ("daily", 5, 0, 0, 0, 0, 0, 0, 0, day, "1d 0:00:00", "24:00:00", "P1D", "1d"),
            ("0days", 5, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 days", 6, 0, 0, 0, 0, 0, 0, 0, 2 * day, "2d 0:00:00", "48:00:00", "P2D", "2d"),
            ("10days", 6, 0, 0, 0, 0, 0, 0, 0, 10 * day, "10d 0:00:00", "240:00:00", "P10D", "10d"),
            ("1.5days", 7, 0, 0, 0, 0, 0, 0, 0, day + 12 * h, "1d 12:00:00", "36:00:00", "P1DT12H", "1d"),
            ("0day", 4, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 day", 5, 0, 0, 0, 0, 0, 0, 0, 2 * day, "2d 0:00:00", "48:00:00", "P2D", "2d"),
            ("10day", 5, 0, 0, 0, 0, 0, 0, 0, 10 * day, "10d 0:00:00", "240:00:00", "P10D", "10d"),
            ("1.5day", 6, 0, 0, 0, 0, 0, 0, 0, day + 12 * h, "1d 12:00:00", "36:00:00", "P1DT12H", "1d"),
            ("0d", 2, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 d", 3, 0, 0, 0, 0, 0, 0, 0, 2 * day, "2d 0:00:00", "48:00:00", "P2D", "2d"),
            ("10d", 3, 0, 0, 0, 0, 0, 0, 0, 10 * day, "10d 0:00:00", "240:00:00", "P10D", "10d"),
            ("1.5d", 4, 0, 0, 0, 0, 0, 0, 0, day + 12 * h, "1d 12:00:00", "36:00:00", "P1DT12H", "1d"),
            ("weekly", 6, 0, 0, 0, 0, 0, 0, 0, 7 * day, "7d 0:00:00", "168:00:00", "P7D", "7d"),
            ("0weeks", 6, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 weeks", 7, 0, 0, 0, 0, 0, 0, 0, 14 * day, "14d 0:00:00", "336:00:00", "P14D", "2w"),
            ("10weeks", 7, 0, 0, 0, 0, 0, 0, 0, 70 * day, "70d 0:00:00", "1680:00:00", "P70D", "10w"),
            ("1.5weeks", 8, 0, 0, 0, 0, 0, 0, 0, 10 * day + 12 * h, "10d 12:00:00", "252:00:00", "P10DT12H", "10d"),
            ("0week", 5, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 week", 6, 0, 0, 0, 0, 0, 0, 0, 14 * day, "14d 0:00:00", "336:00:00", "P14D", "2w"),
            ("10week", 6, 0, 0, 0, 0, 0, 0, 0, 70 * day, "70d 0:00:00", "1680:00:00", "P70D", "10w"),
            ("1.5week", 7, 0, 0, 0, 0, 0, 0, 0, 10 * day + 12 * h, "10d 12:00:00", "252:00:00", "P10DT12H", "10d"),
            ("0w", 2, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 w", 3, 0, 0, 0, 0, 0, 0, 0, 14 * day, "14d 0:00:00", "336:00:00", "P14D", "2w"),
            ("10w", 3, 0, 0, 0, 0, 0, 0, 0, 70 * day, "70d 0:00:00", "1680:00:00", "P70D", "10w"),
            ("1.5w", 4, 0, 0, 0, 0, 0, 0, 0, 10 * day + 12 * h, "10d 12:00:00", "252:00:00", "P10DT12H", "10d"),
            ("monthly", 7, 0, 0, 0, 0, 0, 0, 0, 30 * day, "30d 0:00:00", "720:00:00", "P30D", "4w"),
            ("0months", 7, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 months", 8, 0, 0, 0, 0, 0, 0, 0, 60 * day, "60d 0:00:00", "1440:00:00", "P60D", "8w"),
            ("10months", 8, 0, 0, 0, 0, 0, 0, 0, 300 * day, "300d 0:00:00", "7200:00:00", "P300D", "10mo"),
            ("1.5months", 9, 0, 0, 0, 0, 0, 0, 0, 45 * day, "45d 0:00:00", "1080:00:00", "P45D", "6w"),
            ("0month", 6, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 month", 7, 0, 0, 0, 0, 0, 0, 0, 60 * day, "60d 0:00:00", "1440:00:00", "P60D", "8w"),
            ("10month", 7, 0, 0, 0, 0, 0, 0, 0, 300 * day, "300d 0:00:00", "7200:00:00", "P300D", "10mo"),
            ("1.5month", 8, 0, 0, 0, 0, 0, 0, 0, 45 * day, "45d 0:00:00", "1080:00:00", "P45D", "6w"),
            ("0mo", 3, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 mo", 4, 0, 0, 0, 0, 0, 0, 0, 60 * day, "60d 0:00:00", "1440:00:00", "P60D", "8w"),
            ("10mo", 4, 0, 0, 0, 0, 0, 0, 0, 300 * day, "300d 0:00:00", "7200:00:00", "P300D", "10mo"),
            ("1.5mo", 5, 0, 0, 0, 0, 0, 0, 0, 45 * day, "45d 0:00:00", "1080:00:00", "P45D", "6w"),
            ("quarterly", 9, 0, 0, 0, 0, 0, 0, 0, 91 * day, "91d 0:00:00", "2184:00:00", "P91D", "3mo"),
            ("0quarters", 9, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 quarters", 10, 0, 0, 0, 0, 0, 0, 0, 182 * day, "182d 0:00:00", "4368:00:00", "P182D", "6mo"),
            ("10quarters", 10, 0, 0, 0, 0, 0, 0, 0, 910 * day, "910d 0:00:00", "21840:00:00", "P910D", "2.5y"),
            ("1.5quarters", 11, 0, 0, 0, 0, 0, 0, 0, 136 * day + 12 * h, "136d 12:00:00", "3276:00:00", "P136DT12H", "4mo"),
            ("0quarter", 8, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 quarter", 9, 0, 0, 0, 0, 0, 0, 0, 182 * day, "182d 0:00:00", "4368:00:00", "P182D", "6mo"),
            ("10quarter", 9, 0, 0, 0, 0, 0, 0, 0, 910 * day, "910d 0:00:00", "21840:00:00", "P910D", "2.5y"),
            ("1.5quarter", 10, 0, 0, 0, 0, 0, 0, 0, 136 * day + 12 * h, "136d 12:00:00", "3276:00:00", "P136DT12H", "4mo"),
            ("0q", 2, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 q", 3, 0, 0, 0, 0, 0, 0, 0, 182 * day, "182d 0:00:00", "4368:00:00", "P182D", "6mo"),
            ("10q", 3, 0, 0, 0, 0, 0, 0, 0, 910 * day, "910d 0:00:00", "21840:00:00", "P910D", "2.5y"),
            ("1.5q", 4, 0, 0, 0, 0, 0, 0, 0, 136 * day + 12 * h, "136d 12:00:00", "3276:00:00", "P136DT12H", "4mo"),
            ("yearly", 6, 0, 0, 0, 0, 0, 0, 0, year, "365d 0:00:00", "8760:00:00", "P365D", "1.0y"),
            ("0years", 6, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 years", 7, 0, 0, 0, 0, 0, 0, 0, 2 * year, "730d 0:00:00", "17520:00:00", "P730D", "2.0y"),
            ("10years", 7, 0, 0, 0, 0, 0, 0, 0, 10 * year, "3650d 0:00:00", "87600:00:00", "P3650D", "10.0y"),
            ("1.5years", 8, 0, 0, 0, 0, 0, 0, 0, 547 * day + 12 * h, "547d 12:00:00", "13140:00:00", "P547DT12H", "1.5y"),
            ("0year", 5, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 year", 6, 0, 0, 0, 0, 0, 0, 0, 2 * year, "730d 0:00:00", "17520:00:00", "P730D", "2.0y"),
            ("10year", 6, 0, 0, 0, 0, 0, 0, 0, 10 * year, "3650d 0:00:00", "87600:00:00", "P3650D", "10.0y"),
            ("1.5year", 7, 0, 0, 0, 0, 0, 0, 0, 547 * day + 12 * h, "547d 12:00:00", "13140:00:00", "P547DT12H", "1.5y"),
            ("0y", 2, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 y", 3, 0, 0, 0, 0, 0, 0, 0, 2 * year, "730d 0:00:00", "17520:00:00", "P730D", "2.0y"),
            ("10y", 3, 0, 0, 0, 0, 0, 0, 0, 10 * year, "3650d 0:00:00", "87600:00:00", "P3650D", "10.0y"),
            ("1.5y", 4, 0, 0, 0, 0, 0, 0, 0, 547 * day + 12 * h, "547d 12:00:00", "13140:00:00", "P547DT12H", "1.5y"),
            ("annual", 6, 0, 0, 0, 0, 0, 0, 0, year, "365d 0:00:00", "8760:00:00", "P365D", "1.0y"),
            ("biannual", 8, 0, 0, 0, 0, 0, 0, 0, 2 * year, "730d 0:00:00", "17520:00:00", "P730D", "2.0y"),
            ("bimonthly", 9, 0, 0, 0, 0, 0, 0, 0, 61 * day, "61d 0:00:00", "1464:00:00", "P61D", "8w"),
            ("biweekly", 8, 0, 0, 0, 0, 0, 0, 0, 14 * day, "14d 0:00:00", "336:00:00", "P14D", "2w"),
            ("biyearly", 8, 0, 0, 0, 0, 0, 0, 0, 2 * year, "730d 0:00:00", "17520:00:00", "P730D", "2.0y"),
            ("fortnight", 9, 0, 0, 0, 0, 0, 0, 0, 14 * day, "14d 0:00:00", "336:00:00", "P14D", "2w"),
            ("semiannual", 10, 0, 0, 0, 0, 0, 0, 0, 183 * day, "183d 0:00:00", "4392:00:00", "P183D", "6mo"),
            ("0sennight", 9, 0, 0, 0, 0, 0, 0, 0, 0, "0:00:00", "0:00:00", "PT0S", ""),
            ("2 sennight", 10, 0, 0, 0, 0, 0, 0, 0, 28 * day, "28d 0:00:00", "672:00:00", "P28D", "4w"),
            ("10sennight", 10, 0, 0, 0, 0, 0, 0, 0, 140 * day, "140d 0:00:00", "3360:00:00", "P140D", "4mo"),
            ("1.5sennight", 11, 0, 0, 0, 0, 0, 0, 0, 21 * day, "21d 0:00:00", "504:00:00", "P21D", "3w"),
            ("P1W", 3, 0, 0, 1, 0, 0, 0, 0, 7 * day, "7d 0:00:00", "168:00:00", "P7D", "7d"),
        ];

        assert_eq!(rows.len(), 152);
        for row in &rows {
            test_parse_full(row);
        }
    }

    #[test]
    // `t.notok (left <= right)` is kept literally rather than as `>`.
    #[allow(clippy::nonminimal_bool)]
    fn operators() {
        let mut left;
        let mut right;

        // operator<
        left = d("1s");
        right = d("2s");
        assert!(left < right, "Duration 1s < 2s");
        left = d("-2s");
        right = d("-1s");
        assert!(left < right, "Duration -2s < -1s");
        left = d("1s");
        right = d("1min");
        assert!(left < right, "Duration 1s < 1min");
        left = d("1min");
        right = d("1h");
        assert!(left < right, "Duration 1min < 1h");
        left = d("1h");
        right = d("1d");
        assert!(left < right, "Duration 1h < 1d");
        left = d("1d");
        right = d("1w");
        assert!(left < right, "Duration 1d < 1w");
        left = d("1w");
        right = d("1mo");
        assert!(left < right, "Duration 1w < 1mo");
        left = d("1mo");
        right = d("1q");
        assert!(left < right, "Duration 1mo < 1q");
        left = d("1q");
        right = d("1y");
        assert!(left < right, "Duration 1q < 1y");
        left = d("-3s");
        right = d("-6s");
        assert!(right < left, "Duration -6s < -3s");

        // operator>
        left = d("2s");
        right = d("1s");
        assert!(left > right, "Duration 2s > 1s");
        left = d("-1s");
        right = d("-2s");
        assert!(left > right, "Duration -1s > -2s");
        left = d("1min");
        right = d("1s");
        assert!(left > right, "Duration 1min > 1s");
        left = d("1h");
        right = d("1min");
        assert!(left > right, "Duration 1h > 1min");
        left = d("1d");
        right = d("1h");
        assert!(left > right, "Duration 1d > 1h");
        left = d("1w");
        right = d("1d");
        assert!(left > right, "Duration 1w > 1d");
        left = d("1mo");
        right = d("1w");
        assert!(left > right, "Duration 1mo > 1w");
        left = d("1q");
        right = d("1mo");
        assert!(left > right, "Duration 1q > 1mo");
        left = d("1y");
        right = d("1q");
        assert!(left > right, "Duration 1y > 1q");
        left = d("-3s");
        right = d("-6s");
        assert!(left > right, "Duration -3s > -6s");

        // operator<=
        left = d("1s");
        right = d("2s");
        assert!(left <= right, "Duration 1s <= 2s");
        left = d("2s");
        right = d("2s");
        assert!(left <= right, "Duration 2s <= 2s");
        left = d("2s");
        right = d("1s");
        assert!(!(left <= right), "Duration NOT 2s <= 1s");

        // operator<=
        left = d("2s");
        right = d("1s");
        assert!(left >= right, "Duration 2s >= 1s");
        left = d("2s");
        right = d("2s");
        assert!(left >= right, "Duration 2s >= 2s");
        left = d("1s");
        right = d("2s");
        assert!(!(left >= right), "Duration NOT 1s >= 2s");
    }

    #[test]
    fn accessors() {
        // toString
        let d = Duration::from_seconds(1234567890);
        // 14288d 23:31:30
        assert!(d.days() == 14288, "Duration 1234567890 -> days = 14288");
        assert!(d.hours() == 342935, "Duration 1234567890 -> hours = 342935");
        assert!(
            d.minutes() == 20576131,
            "Duration 1234567890 -> minutes = 20576131"
        );
        assert!(
            d.seconds() == 1234567890,
            "Duration 1234567890 -> seconds = 123456789030"
        );
    }

    #[test]
    fn standalone_seconds() {
        let _guard = StandaloneGuard::save();

        // formatVague (true)
        Duration::set_standalone_seconds_enabled(true);
        assert_eq!(
            d("123").format_vague(false),
            "2min",
            "formatVague: 123 -> '2min'"
        );
        assert_eq!(
            d("123").format_vague(true),
            "2min",
            "formatVague (true): 123 -> '2min'"
        );
        assert_eq!(
            d("3610").format_vague(false),
            "1h",
            "formatVague: 3610 -> '1h'"
        );
        assert_eq!(
            d("3610").format_vague(true),
            "1h  ",
            "formatVague (true): 3610 -> '1h  '"
        );

        // Seconds must be > 60 for a raw number.
        test_parse("0");
        test_parse_error("59");
        test_parse_error("60");
        test_parse("61");
        test_parse("61+0");
        test_parse("61-0");
        test_parse("61)");

        // Repeat with seconds disabled.
        Duration::set_standalone_seconds_enabled(false);
        test_parse_error("0");
        test_parse_error("59");
        test_parse_error("60");
        test_parse_error("61");
        test_parse_error("61+0");
        test_parse_error("61-0");
        test_parse_error("61)");
        Duration::set_standalone_seconds_enabled(true);
    }

    #[test]
    fn embedded_parsing() {
        // Embedded parsing.
        test_parse_error("1weekend");
        test_parse("1d+now");
        test_parse("1d-now");
        test_parse("1d)");
        test_parse_error("1d6");
    }

    #[test]
    fn diagnostic_dump() {
        // This is jus ta diagnostic dump of all named dates, and us used to verify
        // correctness manually.
        let diag = |s: String| println!("# {s}");
        diag("--------------------------------------------".to_string());
        diag(format!("  PT1S       {}", d("PT1S").format_iso()));
        diag(format!("  PT1M       {}", d("PT1M").format_iso()));
        diag(format!("  PT1H       {}", d("PT1H").format_iso()));
        diag(format!("  P1D        {}", d("P1D").format_iso()));
        diag(format!("  P1M        {}", d("P1M").format_iso()));
        diag(format!("  P1Y        {}", d("P1Y").format_iso()));

        // Standalone units, no numbers.
        diag(format!("  sec        {}", d("sec").format_iso()));
        diag(format!("  min        {}", d("min").format_iso()));
        diag(format!("  hr         {}", d("hr").format_iso()));
        diag(format!("  day        {}", d("day").format_iso()));
        diag(format!("  wk         {}", d("wk").format_iso()));
        diag(format!("  biweekly   {}", d("biweekly").format_iso()));
        diag(format!("  fortnight  {}", d("fortnight").format_iso()));
        diag(format!("  mo         {}", d("mo").format_iso()));
        diag(format!("  bimonthly  {}", d("bimonthly").format_iso()));
        diag(format!("  qtr        {}", d("qtr").format_iso()));
        diag(format!("  yr         {}", d("yr").format_iso()));
        diag(format!("  annual     {}", d("annual").format_iso()));
        diag(format!("  biannual   {}", d("biannual").format_iso()));
        diag(format!("  biyearly   {}", d("biyearly").format_iso()));

        // Units.
        diag(format!("  1s         {}", d("1s").format_iso()));
        diag(format!("  1min       {}", d("1min").format_iso()));
        diag(format!("  1h         {}", d("1h").format_iso()));
        diag(format!("  1d         {}", d("1d").format_iso()));
        diag(format!("  1w         {}", d("1w").format_iso()));
        diag(format!("  1m         {}", d("1m").format_iso()));
        diag(format!("  1q         {}", d("1q").format_iso()));
        diag(format!("  1y         {}", d("1y").format_iso()));

        diag("--------------------------------------------".to_string());
    }

    #[test]
    #[rustfmt::skip]
    fn negative_formatting() {
        // Test negative duration formatting
        // Verify that negative durations are formatted with a leading '-'
        assert_eq!(d("-1s").format(),        "-0:00:01", "format: -1s -> '-0:00:01'");
        assert_eq!(d("-60s").format(),       "-0:01:00", "format: -60s -> '-0:01:00'");
        assert_eq!(d("-3600s").format(),     "-1:00:00", "format: -3600s -> '-1:00:00'");
        assert_eq!(d("-86400s").format(),    "-1d 0:00:00", "format: -86400s -> '-1d 0:00:00'");
        assert_eq!(d("-90061s").format(),    "-1d 1:01:01", "format: -90061s -> '-1d 1:01:01'");

        assert_eq!(d("-1s").format_hours(),   "-0:00:01", "formatHours: -1s -> '-0:00:01'");
        assert_eq!(d("-60s").format_hours(),  "-0:01:00", "formatHours: -60s -> '-0:01:00'");
        assert_eq!(d("-3600s").format_hours(), "-1:00:00", "formatHours: -3600s -> '-1:00:00'");
        assert_eq!(d("-90061s").format_hours(), "-25:01:01", "formatHours: -90061s -> '-25:01:01'");

        assert_eq!(d("-1s").format_iso(),     "-PT1S", "formatISO: -1s -> '-PT1S'");
        assert_eq!(d("-60s").format_iso(),    "-PT1M", "formatISO: -60s -> '-PT1M'");
        assert_eq!(d("-3600s").format_iso(),  "-PT1H", "formatISO: -3600s -> '-PT1H'");
        assert_eq!(d("-86400s").format_iso(), "-P1D", "formatISO: -86400s -> '-P1D'");
        assert_eq!(d("-90061s").format_iso(), "-P1DT1H1M1S", "formatISO: -90061s -> '-P1DT1H1M1S'");

        assert_eq!(d("-1s").format_vague(false),   "-1s", "formatVague: -1s -> '-1s'");
        assert_eq!(d("-60s").format_vague(false),   "-1min", "formatVague: -60s -> '-1min'");
        assert_eq!(d("-3600s").format_vague(false), "-1h", "formatVague: -3600s -> '-1h'");
        assert_eq!(d("-86400s").format_vague(false), "-1d", "formatVague: -86400s -> '-1d'");
        assert_eq!(d("-604800s").format_vague(false), "-7d", "formatVague: -604800s -> '-7d'");
        assert_eq!(d("-2592000s").format_vague(false), "-4w", "formatVague: -2592000 -> '-4w'");
        assert_eq!(d("-7776000s").format_vague(false), "-3mo", "formatVague: -7776000 -> '-3mo'");
        assert_eq!(d("-31536000s").format_vague(false), "-1.0y", "formatVague: -31536000 -> '-1.0y'");

        // Test round-trip: parse negative ISO format, then format back
        assert_eq!(d("-PT1S").format_iso(), "-PT1S", "round-trip: -PT1S -> parse -> formatISO -> -PT1S");
        assert_eq!(d("-PT1M").format_iso(), "-PT1M", "round-trip: -PT1M -> parse -> formatISO -> -PT1M");
        assert_eq!(d("-PT1H").format_iso(), "-PT1H", "round-trip: -PT1H -> parse -> formatISO -> -PT1H");
        assert_eq!(d("-P1D").format_iso(), "-P1D", "round-trip: -P1D -> parse -> formatISO -> -P1D");
        assert_eq!(d("-P1DT1H1M1S").format_iso(), "-P1DT1H1M1S", "round-trip: -P1DT1H1M1S -> parse -> formatISO -> -P1DT1H1M1S");
        assert_eq!(d("-PT1H1M1S").format_iso(), "-PT1H1M1S", "round-trip: -PT1H1M1S -> parse -> formatISO -> -PT1H1M1S");
        assert_eq!(d("-P1Y").format_iso(), "-P365D", "round-trip: -P1Y -> parse -> formatISO -> -P365D");
        assert_eq!(d("-P1M").format_iso(), "-P30D", "round-trip: -P1M -> parse -> formatISO -> -P30D");

        // Test formatVague with padding
        assert_eq!(d("-1s").format_vague(true),   "-1s  ", "formatVague (true): -1s -> '-1s  '");
        assert_eq!(d("-3600s").format_vague(true), "-1h  ", "formatVague (true): -3600s -> '-1h  '");
        assert_eq!(d("-86400s").format_vague(true), "-1d  ", "formatVague (true): -86400s -> '-1d  '");
        assert_eq!(d("-604800s").format_vague(true), "-7d  ", "formatVague (true): -604800s -> '-7d  '");

        // Test that positive durations don't get a leading '-'
        assert_eq!(d("1s").format(),        "0:00:01", "format: 1s -> '0:00:01' (no leading '-')");
        assert_eq!(d("1s").format_hours(),   "0:00:01", "formatHours: 1s -> '0:00:01' (no leading '-')");
        assert_eq!(d("1s").format_iso(),     "PT1S", "formatISO: 1s -> 'PT1S' (no leading '-')");
        assert_eq!(d("1s").format_vague(false),   "1s", "formatVague: 1s -> '1s' (no leading '-')");

        // Test zero duration
        assert_eq!(d("0s").format(),        "0:00:00", "format: 0s -> '0:00:00'");
        assert_eq!(d("0s").format_hours(),   "0:00:00", "formatHours: 0s -> '0:00:00'");
        assert_eq!(d("0s").format_iso(),     "PT0S", "formatISO: 0s -> 'PT0S'");
        assert_eq!(d("0s").format_vague(false),   "", "formatVague: 0s -> ''");
    }

    // Rust-only checks of C++ behaviour not covered by duration.t.cpp.

    #[test]
    fn to_string_and_to_time_t() {
        assert_eq!(
            Duration::from_seconds(-42).to_string(),
            "-42",
            "toString: -42"
        );
        assert_eq!(d("1h").to_string(), "3600", "toString: 1h -> 3600");
        assert_eq!(d("1h").to_time_t(), 3600, "toTime_t: 1h -> 3600");
        assert_eq!(Duration::default()._period, 0, "Duration () -> 0");
    }

    #[test]
    fn parse_with_offset_and_failure_keeps_start() {
        let mut dur = Duration::new();
        let mut start = 4;
        assert!(
            dur.parse("due:3d", &mut start),
            "parse ('due:3d', 4) --> true"
        );
        assert_eq!(start, 6, "parse ('due:3d', 4) --> [6]");
        assert_eq!(dur._period, 3 * 86400, "parse ('due:3d', 4) --> 3d");

        let mut dur = Duration::new();
        let mut start = 0;
        assert!(!dur.parse("foo", &mut start), "parse ('foo') --> false");
        assert_eq!(start, 0, "parse ('foo') leaves start");
    }

    #[test]
    fn d_unit_uuid_guard_and_non_standalone_units() {
        test_parse_error("1111111d-0000-0000-0000-000000000000");
        test_parse("10000d");
        // A non-standalone unit on its own is rejected.
        test_parse_error("days");
        test_parse_error("d");
        test_parse("day");
    }
}
