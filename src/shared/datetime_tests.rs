//! Line-by-line port of libshared (d899eba6) `test/datetime.t.cpp` and
//! `test/dates.t.cpp`.
//!
//! The C++ `UnitTest` harness is mirrored by [`UnitTest`]: every `t.ok`,
//! `t.notok`, `t.is`, `t.pass` and `t.fail` becomes a call with the identical
//! message, so each upstream assertion has exactly one counterpart here. A run
//! collects all failures and then checks the number of executed assertions
//! against the static count of the C++ file (6640 for datetime.t.cpp; its
//! `UnitTest t (3458)` plan is stale; 201 for dates.t.cpp, equal to its plan).
//!
//! Every suite runs under each zone in [`ZONES`] (with `env_lock` held, `TZ`
//! set, `tzset ()` called and `TZ` restored through `EnvGuard` before the lock
//! is released) and, inside each zone, under the C++ default settings.
//!
//! Determinism. The C++ tests read the clock repeatedly, so a run that fails
//! while the clock second changed is discarded and repeated (see [`run`]).
//! Where the upstream expectation itself is wrong at particular moments or in
//! particular zones, the expectation is computed so that it is right at all
//! times, without changing what is asserted; each such place is marked
//! `Deviation:`:
//! - time-only rows: wall-clock local time instead of `local + secs` (DST
//!   transition days), and the "extra day" decided per row (`ld`/`ud` used
//!   12:34:56 for the 12:34 rows too, wrong between 12:34:00 and 12:34:56);
//! - the dated local references use `tm_isdst = -1`, not `0` (December is
//!   summer time in the southern hemisphere: Lord Howe, Chatham);
//! - informal-time `adjust` compares exact seconds (wrong in the minute after
//!   the time, as in the existing named-date tests);
//! - "next/previous <weekday>" compares with a calendar day, not
//!   `now + k * 86400` (off by the DST delta just before midnight);
//! - quantization startOfDay uses the local date of 1234526400 (2009-02-14
//!   east of UTC+12, e.g. Chatham);
//! - `timeRelative` 0:00:01 during the first second of a day;
//! - dates.t.cpp `midsommarafton < midsommar` on midsommarafton itself.
use super::*;
use crate::test_util::{env_lock, EnvGuard};
use std::fmt::Debug;

/// Zones every suite runs under: UTC, both DST hemispheres, a half-hour
/// offset without DST (Kolkata), a half-hour DST shift (Lord Howe) and a
/// +12:45/+13:45 offset (Chatham).
const ZONES: [&str; 6] = [
    "UTC",
    "America/New_York",
    "Europe/Berlin",
    "Asia/Kolkata",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
];

/// Static count of the assertions executed by datetime.t.cpp: 355 direct
/// `t.*` calls (22 if/else pairs, two 12-month loops, the unreached
/// `t.fail` in `catch`), 245 table rows x 2 weekstarts x 12, 20 ISO-week rows
/// x 12 and 165 one-assertion `testParse`/`testParseError` calls.
const DATETIME_T_ASSERTIONS: usize = 355 + 245 * 2 * 12 + 20 * 12 + 165;

/// dates.t.cpp: `UnitTest t (201)`.
const DATES_T_ASSERTIONS: usize = 201;

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

    fn notok(&mut self, expression: bool, name: impl AsRef<str>) {
        self.ok(!expression, name);
    }

    fn is<A: PartialEq<E> + Debug, E: Debug>(&mut self, actual: A, expected: E, name: impl AsRef<str>) {
        self.counter += 1;
        if actual != expected {
            self.failures.push(format!(
                "not ok {} - {}\n#   expected: {:?}\n#        got: {:?}",
                self.counter,
                name.as_ref(),
                expected,
                actual
            ));
        }
    }

    fn pass(&mut self, name: impl AsRef<str>) {
        self.ok(true, name);
    }

    fn fail(&mut self, name: impl AsRef<str>) {
        self.ok(false, name);
    }

    fn diag(&self, text: impl AsRef<str>) {
        println!("# {}", text.as_ref());
    }
}

/// Saves the thread-local `Datetime::*` statics, resets them to the C++
/// initial values (Datetime.cpp lines 62-76) and restores them on drop.
struct Settings {
    weekstart: i32,
    minimum_match_length: i32,
    iso_enabled: bool,
    standalone_date_enabled: bool,
    standalone_time_enabled: bool,
    time_relative: bool,
}

impl Settings {
    fn save() -> Self {
        Settings {
            weekstart: Datetime::weekstart(),
            minimum_match_length: Datetime::minimum_match_length(),
            iso_enabled: Datetime::iso_enabled(),
            standalone_date_enabled: Datetime::standalone_date_enabled(),
            standalone_time_enabled: Datetime::standalone_time_enabled(),
            time_relative: Datetime::time_relative(),
        }
    }

    fn reset_to_defaults() {
        Datetime::set_weekstart(1);
        Datetime::set_minimum_match_length(3);
        Datetime::set_iso_enabled(true);
        Datetime::set_standalone_date_enabled(true);
        Datetime::set_standalone_time_enabled(true);
        Datetime::set_time_relative(true);
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        Datetime::set_weekstart(self.weekstart);
        Datetime::set_minimum_match_length(self.minimum_match_length);
        Datetime::set_iso_enabled(self.iso_enabled);
        Datetime::set_standalone_date_enabled(self.standalone_date_enabled);
        Datetime::set_standalone_time_enabled(self.standalone_time_enabled);
        Datetime::set_time_relative(self.time_relative);
    }
}

/// Runs one upstream program (`suite`) with `TZ=tz` and the C++ default
/// settings, then asserts that it passed and executed `planned` assertions.
///
/// The upstream programs read the clock many times; a failing run during
/// which the clock second changed may have raced (e.g. a parse and its
/// reference straddling midnight), so it is discarded and repeated.
fn run(tz: &str, planned: usize, suite: fn(&mut UnitTest)) {
    const ATTEMPTS: usize = 20;

    let _lock = env_lock();
    let env = EnvGuard::save(&["TZ"]);
    std::env::set_var("TZ", tz);
    localtime::tzset();
    if !std::path::Path::new("/usr/share/zoneinfo").join(tz).exists() && tz != "UTC" {
        println!("# zoneinfo for {tz} not found; libc falls back to UTC");
    }

    let mut outcome = None;
    {
        let _settings = Settings::save();
        for _ in 0..ATTEMPTS {
            Settings::reset_to_defaults();
            let before = localtime::time_now();
            let mut t = UnitTest::new();
            suite(&mut t);
            let ticked = localtime::time_now() != before;
            let done = t.failures.is_empty() || !ticked;
            outcome = Some(t);
            if done {
                break;
            }
        }
    }

    drop(env);
    localtime::tzset();

    let t = outcome.expect("at least one attempt");
    assert!(
        t.failures.is_empty(),
        "TZ={tz}: {} of {} assertions failed:\n{}",
        t.failures.len(),
        t.counter,
        t.failures.join("\n")
    );
    assert_eq!(t.counter, planned, "TZ={tz}: number of assertions executed");
}

/// `Datetime (input, format)`; a throw is recorded as the C++ `catch` does
/// (`t.fail ("Exception thrown.")`) and the clock time is used instead.
fn dt(t: &mut UnitTest, input: &str, format_: &str) -> Datetime {
    match Datetime::from_string(input, format_) {
        Ok(value) => value,
        Err(e) => {
            t.fail(format!("Exception thrown. {e}"));
            Datetime::new()
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// datetime.t.cpp
////////////////////////////////////////////////////////////////////////////////

/// datetime.t.cpp line 33: `testParse (t, input, in_start, ...)`.
#[allow(clippy::too_many_arguments)]
fn p(
    t: &mut UnitTest,
    input: &str,
    in_start: i64,
    in_year: i64,
    in_month: i64,
    in_week: i64,
    in_weekday: i64,
    in_julian: i64,
    in_day: i64,
    in_seconds: i64,
    in_offset: i64,
    in_utc: bool,
    in_date: i64,
) {
    let label = format!("Datetime::parse (\"{input}\") --> ");

    let mut iso = Datetime::new();
    let mut start = 0usize;

    t.ok(iso.parse(input, &mut start, ""), format!("{label}true"));
    t.is(start as i64, in_start, format!("{label}[]"));
    t.is(iso._year as i64, in_year, format!("{label}_year"));
    t.is(iso._month as i64, in_month, format!("{label}_month"));
    t.is(iso._week as i64, in_week, format!("{label}_week"));
    t.is(iso._weekday as i64, in_weekday, format!("{label}_weekday"));
    t.is(iso._julian as i64, in_julian, format!("{label}_julian"));
    t.is(iso._day as i64, in_day, format!("{label}_day"));
    t.is(iso._seconds as i64, in_seconds, format!("{label}_seconds"));
    t.is(iso._offset as i64, in_offset, format!("{label}_offset"));
    t.is(iso._utc, in_utc, format!("{label}_utc"));
    t.is(iso._date, in_date, format!("{label}_date"));
}

/// datetime.t.cpp line 68: `testParse (t, input)`.
fn test_parse(t: &mut UnitTest, input: &str) {
    let label = format!("Datetime::parse positive '{input}' --> success");

    let mut positive = Datetime::new();
    let mut pos = 0usize;
    let parsed = positive.parse(input, &mut pos, "");
    t.ok(parsed && pos != 0, label);
}

/// datetime.t.cpp line 80: `testParseError`.
fn test_parse_error(t: &mut UnitTest, input: &str) {
    let label = format!("Datetime::parse negative '{input}' --> fail");

    let mut neg = Datetime::new();
    let mut pos = 0usize;
    t.notok(neg.parse(input, &mut pos, ""), label);
}

/// The reference values computed at the top of datetime.t.cpp `main`
/// (lines 99-180), including the reuse of one `struct tm` per zone whose
/// fields carry over between the `mktime`/`timegm` calls.
struct Reference {
    now: i64,
    local_s: i64,
    utc_s: i64,
    local6: i64,
    f_local6: i64,
    local1: i64,
    f_local1: i64,
    utc: i64,
    utc6: i64,
    f_utc6: i64,
    utc1: i64,
    f_utc1: i64,
}

impl Reference {
    fn compute(t: &UnitTest) -> Self {
        let year = 2013;
        let mo = 12;
        let f_yr = 9850; // future year with same starting weekday as 2013

        // Determine local and UTC time.
        let now = localtime::time_now();
        let mut local_now = localtime::localtime(now);
        let local_s = (local_now.tm_hour * 3600) + (local_now.tm_min * 60) + local_now.tm_sec;
        local_now.tm_hour = 0;
        local_now.tm_min = 0;
        local_now.tm_sec = 0;
        local_now.tm_isdst = -1;
        let local = localtime::mktime(&mut local_now);
        t.diag(format!("local midnight today {local}"));

        // Deviation: the C++ sets tm_isdst = 0 for the dated references,
        // which is one DST delta off where December is summer time (southern
        // hemisphere); Datetime itself always uses -1.
        local_now.tm_year = year - 1900;
        local_now.tm_mon = mo - 1;
        local_now.tm_mday = 6;
        local_now.tm_isdst = -1;
        let local6 = localtime::mktime(&mut local_now);
        t.diag(format!("local midnight 2013-12-06 {local6}"));

        local_now.tm_year = f_yr - 1900;
        local_now.tm_isdst = -1;
        let f_local6 = localtime::mktime(&mut local_now);
        t.diag(format!("future midnight 9850-12-06 {f_local6}"));

        local_now.tm_year = year - 1900;
        local_now.tm_mon = mo - 1;
        local_now.tm_mday = 1;
        local_now.tm_isdst = -1;
        let local1 = localtime::mktime(&mut local_now);
        t.diag(format!("local midnight 2013-12-01 {local1}"));

        local_now.tm_year = f_yr - 1900;
        local_now.tm_isdst = -1;
        let f_local1 = localtime::mktime(&mut local_now);
        t.diag(format!("future midnight 9850-12-01 {f_local1}"));

        let mut utc_now = localtime::gmtime(now);
        let utc_s = (utc_now.tm_hour * 3600) + (utc_now.tm_min * 60) + utc_now.tm_sec;
        utc_now.tm_hour = 0;
        utc_now.tm_min = 0;
        utc_now.tm_sec = 0;
        utc_now.tm_isdst = -1;
        let utc = localtime::timegm(&mut utc_now);
        t.diag(format!("utc midnight today {utc}"));

        utc_now.tm_year = year - 1900;
        utc_now.tm_mon = mo - 1;
        utc_now.tm_mday = 6;
        utc_now.tm_isdst = 0;
        let utc6 = localtime::timegm(&mut utc_now);
        t.diag(format!("utc midnight 2013-12-06 {utc6}"));

        utc_now.tm_year = f_yr - 1900;
        let f_utc6 = localtime::timegm(&mut utc_now);
        t.diag(format!("future midnight 9850-12-06 {f_utc6}"));

        utc_now.tm_year = year - 1900;
        utc_now.tm_mon = mo - 1;
        utc_now.tm_mday = 1;
        utc_now.tm_isdst = 0;
        let utc1 = localtime::timegm(&mut utc_now);
        t.diag(format!("utc midnight 2013-12-01 {utc1}"));

        utc_now.tm_year = f_yr - 1900;
        let f_utc1 = localtime::timegm(&mut utc_now);
        t.diag(format!("future midnight 9850-12-01 {f_utc1}"));

        Reference {
            now,
            local_s: local_s as i64,
            utc_s: utc_s as i64,
            local6,
            f_local6,
            local1,
            f_local1,
            utc,
            utc6,
            f_utc6,
            utc1,
            f_utc1,
        }
    }

    /// C++ `ld`: local extra day if now > secs.
    ///
    /// Deviation: decided per row; the C++ uses `local_s > hms` for the
    /// 12:34 rows too (wrong between 12:34:00 and 12:34:56).
    fn ld(&self, secs: i64) -> i64 {
        if self.local_s > secs {
            86400
        } else {
            0
        }
    }

    /// C++ `ud`: UTC extra day if now > secs (per row, as `ld`).
    fn ud(&self, secs: i64) -> i64 {
        if self.utc_s > secs {
            86400
        } else {
            0
        }
    }

    /// C++ `local + secs`: today's local midnight (`local`) plus `secs`.
    ///
    /// Deviation: computed on the wall clock (as `Datetime::resolve` and
    /// `initializeInformalTime` do), which differs from the C++ arithmetic
    /// only on a DST transition day.
    fn local_plus(&self, secs: i64) -> i64 {
        let mut t = localtime::localtime(self.now);
        t.tm_mday += (secs / 86400) as i32;
        let secs = secs % 86400;
        t.tm_hour = (secs / 3600) as i32;
        t.tm_min = ((secs % 3600) / 60) as i32;
        t.tm_sec = (secs % 60) as i32;
        t.tm_isdst = -1;
        localtime::mktime(&mut t)
    }
}

/// datetime.t.cpp lines 182-501: the testParse table, for both weekstarts.
fn parse_table(t: &mut UnitTest, r: &Reference) {
    let (local6, f_local6, utc, utc6, f_utc6) = (r.local6, r.f_local6, r.utc, r.utc6, r.f_utc6);
    let (mut local1, mut f_local1, mut utc1, mut f_utc1) = (r.local1, r.f_local1, r.utc1, r.f_utc1);

    let year: i64 = 2013;
    let mo: i64 = 12;
    let f_yr: i64 = 9850;
    let hms: i64 = (12 * 3600) + (34 * 60) + 56; // The time 12:34:56 in seconds.
    let hm: i64 = (12 * 3600) + (34 * 60); // The time 12:34:00 in seconds.
    let z: i64 = 3600; // TZ offset.
    t.diag(format!("ld {}", r.ld(hms)));
    t.diag(format!("ud {}", r.ud(hms)));

    for wks in 0..=1i64 {
        // Run the test parses against both weekstarts
        Datetime::set_weekstart(wks as i32);

        // 2013-W49 tests without weekday differ by one day depending on weekstart.
        // None of these variables are used following this loop.
        f_local1 += wks * 86400;
        f_utc1 += wks * 86400;
        local1 += wks * 86400;
        utc1 += wks * 86400;

        // `local+secs+ld` and `utc+secs+ud` (see Reference::local_plus / ld / ud).
        let lt = |secs: i64| r.local_plus(secs + r.ld(secs));
        let ud = |secs: i64| r.ud(secs);

        // Aggregated, time-ext, datetime-ext, datetime-ext in the future, the
        // only non-extended forms, non-extended time/datetime (and future).
        // Columns: input, i, Year, Mo, Wk, WD, Jul, Da, Secs, TZ, UTC, time_t
        p(t, "12:34:56  ", 8, 0, 0, 0, wks, 0, 0, hms, 0, false, lt(hms));
        p(t, "12:34:56Z", 9, 0, 0, 0, wks, 0, 0, hms, 0, true, utc+hms+ud(hms));
        p(t, "12:34Z", 6, 0, 0, 0, wks, 0, 0, hm, 0, true, utc+hm+ud(hm));
        p(t, "12:34:56+01:00", 14, 0, 0, 0, wks, 0, 0, hms, 3600, false, utc+hms-z+ud(hms));
        p(t, "12:34:56+01", 11, 0, 0, 0, wks, 0, 0, hms, 3600, false, utc+hms-z+ud(hms));
        p(t, "12:34+01:00", 11, 0, 0, 0, wks, 0, 0, hm, 3600, false, utc+hm-z+ud(hm));
        p(t, "12:34+01", 8, 0, 0, 0, wks, 0, 0, hm, 3600, false, utc+hm-z+ud(hm));
        p(t, "12:34:56", 8, 0, 0, 0, wks, 0, 0, hms, 0, false, lt(hms));
        p(t, "12:34", 5, 0, 0, 0, wks, 0, 0, hm, 0, false, lt(hm));
        p(t, "2013-12-06", 10, year, mo, 0, wks, 0, 6, 0, 0, false, local6);
        p(t, "2013-340", 8, year, 0, 0, wks, 340, 0, 0, 0, false, local6);
        p(t, "2013-W49-5", 10, year, 0, 49, 5, 0, 0, 0, 0, false, local6);
        p(t, "2013-W49", 8, year, 0, 49, wks, 0, 0, 0, 0, false, local1);
        p(t, "2013-12", 7, year, mo, 0, wks, 0, 1, 0, 0, false, local1 - 86400 * wks);
        p(t, "2013-12-06T12:34:56", 19, year, mo, 0, wks, 0, 6, hms, 0, false, local6+hms);
        p(t, "2013-12-06T12:34", 16, year, mo, 0, wks, 0, 6, hm, 0, false, local6+hm);
        p(t, "2013-340T12:34:56", 17, year, 0, 0, wks, 340, 0, hms, 0, false, local6+hms);
        p(t, "2013-340T12:34", 14, year, 0, 0, wks, 340, 0, hm, 0, false, local6+hm);
        p(t, "2013-W49-5T12:34:56", 19, year, 0, 49, 5, 0, 0, hms, 0, false, local6+hms);
        p(t, "2013-W49-5T12:34", 16, year, 0, 49, 5, 0, 0, hm, 0, false, local6+hm);
        p(t, "2013-W49T12:34:56", 17, year, 0, 49, wks, 0, 0, hms, 0, false, local1+hms);
        p(t, "2013-W49T12:34", 14, year, 0, 49, wks, 0, 0, hm, 0, false, local1+hm);
        p(t, "2013-12-06T12:34:56Z", 20, year, mo, 0, wks, 0, 6, hms, 0, true, utc6+hms);
        p(t, "2013-12-06T12:34Z", 17, year, mo, 0, wks, 0, 6, hm, 0, true, utc6+hm);
        p(t, "2013-340T12:34:56Z", 18, year, 0, 0, wks, 340, 0, hms, 0, true, utc6+hms);
        p(t, "2013-340T12:34Z", 15, year, 0, 0, wks, 340, 0, hm, 0, true, utc6+hm);
        p(t, "2013-W49-5T12:34:56Z", 20, year, 0, 49, 5, 0, 0, hms, 0, true, utc6+hms);
        p(t, "2013-W49-5T12:34Z", 17, year, 0, 49, 5, 0, 0, hm, 0, true, utc6+hm);
        p(t, "2013-W49T12:34:56Z", 18, year, 0, 49, wks, 0, 0, hms, 0, true, utc1+hms);
        p(t, "2013-W49T12:34Z", 15, year, 0, 49, wks, 0, 0, hm, 0, true, utc1+hm);
        p(t, "2013-12-06T12:34:56+01:00", 25, year, mo, 0, wks, 0, 6, hms, 3600, false, utc6+hms-z);
        p(t, "2013-12-06T12:34:56+01", 22, year, mo, 0, wks, 0, 6, hms, 3600, false, utc6+hms-z);
        p(t, "2013-12-06T12:34:56-01:00", 25, year, mo, 0, wks, 0, 6, hms, -3600, false, utc6+hms+z);
        p(t, "2013-12-06T12:34:56-01", 22, year, mo, 0, wks, 0, 6, hms, -3600, false, utc6+hms+z);
        p(t, "2013-12-06T12:34+01:00", 22, year, mo, 0, wks, 0, 6, hm, 3600, false, utc6+hm-z);
        p(t, "2013-12-06T12:34+01", 19, year, mo, 0, wks, 0, 6, hm, 3600, false, utc6+hm-z);
        p(t, "2013-12-06T12:34-01:00", 22, year, mo, 0, wks, 0, 6, hm, -3600, false, utc6+hm+z);
        p(t, "2013-12-06T12:34-01", 19, year, mo, 0, wks, 0, 6, hm, -3600, false, utc6+hm+z);
        p(t, "2013-340T12:34:56+01:00", 23, year, 0, 0, wks, 340, 0, hms, 3600, false, utc6+hms-z);
        p(t, "2013-340T12:34:56+01", 20, year, 0, 0, wks, 340, 0, hms, 3600, false, utc6+hms-z);
        p(t, "2013-340T12:34:56-01:00", 23, year, 0, 0, wks, 340, 0, hms, -3600, false, utc6+hms+z);
        p(t, "2013-340T12:34:56-01", 20, year, 0, 0, wks, 340, 0, hms, -3600, false, utc6+hms+z);
        p(t, "2013-340T12:34+01:00", 20, year, 0, 0, wks, 340, 0, hm, 3600, false, utc6+hm-z);
        p(t, "2013-340T12:34+01", 17, year, 0, 0, wks, 340, 0, hm, 3600, false, utc6+hm-z);
        p(t, "2013-340T12:34-01:00", 20, year, 0, 0, wks, 340, 0, hm, -3600, false, utc6+hm+z);
        p(t, "2013-340T12:34-01", 17, year, 0, 0, wks, 340, 0, hm, -3600, false, utc6+hm+z);
        p(t, "2013-W49-5T12:34:56+01:00", 25, year, 0, 49, 5, 0, 0, hms, 3600, false, utc6+hms-z);
        p(t, "2013-W49-5T12:34:56+01", 22, year, 0, 49, 5, 0, 0, hms, 3600, false, utc6+hms-z);
        p(t, "2013-W49-5T12:34:56-01:00", 25, year, 0, 49, 5, 0, 0, hms, -3600, false, utc6+hms+z);
        p(t, "2013-W49-5T12:34:56-01", 22, year, 0, 49, 5, 0, 0, hms, -3600, false, utc6+hms+z);
        p(t, "2013-W49-5T12:34+01:00", 22, year, 0, 49, 5, 0, 0, hm, 3600, false, utc6+hm-z);
        p(t, "2013-W49-5T12:34+01", 19, year, 0, 49, 5, 0, 0, hm, 3600, false, utc6+hm-z);
        p(t, "2013-W49-5T12:34-01:00", 22, year, 0, 49, 5, 0, 0, hm, -3600, false, utc6+hm+z);
        p(t, "2013-W49-5T12:34-01", 19, year, 0, 49, 5, 0, 0, hm, -3600, false, utc6+hm+z);
        p(t, "2013-W49T12:34:56+01:00", 23, year, 0, 49, wks, 0, 0, hms, 3600, false, utc1+hms-z);
        p(t, "2013-W49T12:34:56+01", 20, year, 0, 49, wks, 0, 0, hms, 3600, false, utc1+hms-z);
        p(t, "2013-W49T12:34:56-01:00", 23, year, 0, 49, wks, 0, 0, hms, -3600, false, utc1+hms+z);
        p(t, "2013-W49T12:34:56-01", 20, year, 0, 49, wks, 0, 0, hms, -3600, false, utc1+hms+z);
        p(t, "2013-W49T12:34+01:00", 20, year, 0, 49, wks, 0, 0, hm, 3600, false, utc1+hm-z);
        p(t, "2013-W49T12:34+01", 17, year, 0, 49, wks, 0, 0, hm, 3600, false, utc1+hm-z);
        p(t, "2013-W49T12:34-01:00", 20, year, 0, 49, wks, 0, 0, hm, -3600, false, utc1+hm+z);
        p(t, "2013-W49T12:34-01", 17, year, 0, 49, wks, 0, 0, hm, -3600, false, utc1+hm+z);
        p(t, "9850-12-06", 10, f_yr, mo, 0, wks, 0, 6, 0, 0, false, f_local6);
        p(t, "9850-340", 8, f_yr, 0, 0, wks, 340, 0, 0, 0, false, f_local6);
        p(t, "9850-W49-5", 10, f_yr, 0, 49, 5, 0, 0, 0, 0, false, f_local6);
        p(t, "9850-W49", 8, f_yr, 0, 49, wks, 0, 0, 0, 0, false, f_local1);
        p(t, "9850-12", 7, f_yr, mo, 0, wks, 0, 1, 0, 0, false, f_local1 - 86400 * wks);
        p(t, "9850-12-06T12:34:56", 19, f_yr, mo, 0, wks, 0, 6, hms, 0, false, f_local6+hms);
        p(t, "9850-12-06T12:34", 16, f_yr, mo, 0, wks, 0, 6, hm, 0, false, f_local6+hm);
        p(t, "9850-340T12:34:56", 17, f_yr, 0, 0, wks, 340, 0, hms, 0, false, f_local6+hms);
        p(t, "9850-340T12:34", 14, f_yr, 0, 0, wks, 340, 0, hm, 0, false, f_local6+hm);
        p(t, "9850-W49-5T12:34:56", 19, f_yr, 0, 49, 5, 0, 0, hms, 0, false, f_local6+hms);
        p(t, "9850-W49-5T12:34", 16, f_yr, 0, 49, 5, 0, 0, hm, 0, false, f_local6+hm);
        p(t, "9850-W49T12:34:56", 17, f_yr, 0, 49, wks, 0, 0, hms, 0, false, f_local1+hms);
        p(t, "9850-W49T12:34", 14, f_yr, 0, 49, wks, 0, 0, hm, 0, false, f_local1+hm);
        p(t, "9850-12-06T12:34:56Z", 20, f_yr, mo, 0, wks, 0, 6, hms, 0, true, f_utc6+hms);
        p(t, "9850-12-06T12:34Z", 17, f_yr, mo, 0, wks, 0, 6, hm, 0, true, f_utc6+hm);
        p(t, "9850-340T12:34:56Z", 18, f_yr, 0, 0, wks, 340, 0, hms, 0, true, f_utc6+hms);
        p(t, "9850-340T12:34Z", 15, f_yr, 0, 0, wks, 340, 0, hm, 0, true, f_utc6+hm);
        p(t, "9850-W49-5T12:34:56Z", 20, f_yr, 0, 49, 5, 0, 0, hms, 0, true, f_utc6+hms);
        p(t, "9850-W49-5T12:34Z", 17, f_yr, 0, 49, 5, 0, 0, hm, 0, true, f_utc6+hm);
        p(t, "9850-W49T12:34:56Z", 18, f_yr, 0, 49, wks, 0, 0, hms, 0, true, f_utc1+hms);
        p(t, "9850-W49T12:34Z", 15, f_yr, 0, 49, wks, 0, 0, hm, 0, true, f_utc1+hm);
        p(t, "9850-12-06T12:34:56+01:00", 25, f_yr, mo, 0, wks, 0, 6, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850-12-06T12:34:56+01", 22, f_yr, mo, 0, wks, 0, 6, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850-12-06T12:34:56-01:00", 25, f_yr, mo, 0, wks, 0, 6, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850-12-06T12:34:56-01", 22, f_yr, mo, 0, wks, 0, 6, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850-12-06T12:34+01:00", 22, f_yr, mo, 0, wks, 0, 6, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850-12-06T12:34+01", 19, f_yr, mo, 0, wks, 0, 6, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850-12-06T12:34-01:00", 22, f_yr, mo, 0, wks, 0, 6, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850-12-06T12:34-01", 19, f_yr, mo, 0, wks, 0, 6, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850-340T12:34:56+01:00", 23, f_yr, 0, 0, wks, 340, 0, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850-340T12:34:56+01", 20, f_yr, 0, 0, wks, 340, 0, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850-340T12:34:56-01:00", 23, f_yr, 0, 0, wks, 340, 0, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850-340T12:34:56-01", 20, f_yr, 0, 0, wks, 340, 0, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850-340T12:34+01:00", 20, f_yr, 0, 0, wks, 340, 0, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850-340T12:34+01", 17, f_yr, 0, 0, wks, 340, 0, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850-340T12:34-01:00", 20, f_yr, 0, 0, wks, 340, 0, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850-340T12:34-01", 17, f_yr, 0, 0, wks, 340, 0, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850-W49-5T12:34:56+01:00", 25, f_yr, 0, 49, 5, 0, 0, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850-W49-5T12:34:56+01", 22, f_yr, 0, 49, 5, 0, 0, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850-W49-5T12:34:56-01:00", 25, f_yr, 0, 49, 5, 0, 0, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850-W49-5T12:34:56-01", 22, f_yr, 0, 49, 5, 0, 0, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850-W49-5T12:34+01:00", 22, f_yr, 0, 49, 5, 0, 0, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850-W49-5T12:34+01", 19, f_yr, 0, 49, 5, 0, 0, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850-W49-5T12:34-01:00", 22, f_yr, 0, 49, 5, 0, 0, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850-W49-5T12:34-01", 19, f_yr, 0, 49, 5, 0, 0, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850-W49T12:34:56+01:00", 23, f_yr, 0, 49, wks, 0, 0, hms, 3600, false, f_utc1+hms-z);
        p(t, "9850-W49T12:34:56+01", 20, f_yr, 0, 49, wks, 0, 0, hms, 3600, false, f_utc1+hms-z);
        p(t, "9850-W49T12:34:56-01:00", 23, f_yr, 0, 49, wks, 0, 0, hms, -3600, false, f_utc1+hms+z);
        p(t, "9850-W49T12:34:56-01", 20, f_yr, 0, 49, wks, 0, 0, hms, -3600, false, f_utc1+hms+z);
        p(t, "9850-W49T12:34+01:00", 20, f_yr, 0, 49, wks, 0, 0, hm, 3600, false, f_utc1+hm-z);
        p(t, "9850-W49T12:34+01", 17, f_yr, 0, 49, wks, 0, 0, hm, 3600, false, f_utc1+hm-z);
        p(t, "9850-W49T12:34-01:00", 20, f_yr, 0, 49, wks, 0, 0, hm, -3600, false, f_utc1+hm+z);
        p(t, "9850-W49T12:34-01", 17, f_yr, 0, 49, wks, 0, 0, hm, -3600, false, f_utc1+hm+z);
        p(t, "20131206T123456Z", 16, year, mo, 0, wks, 0, 6, hms, 0, true, utc6+hms);
        p(t, "20131206T123456", 15, year, mo, 0, wks, 0, 6, hms, 0, false, local6+hms);
        p(t, "98501206T123456Z", 16, f_yr, mo, 0, wks, 0, 6, hms, 0, true, f_utc6+hms);
        p(t, "98501206T123456", 15, f_yr, mo, 0, wks, 0, 6, hms, 0, false, f_local6+hms);
        p(t, "123456Z", 7, 0, 0, 0, wks, 0, 0, hms, 0, true, utc+hms+ud(hms));
        p(t, "1234Z", 5, 0, 0, 0, wks, 0, 0, hm, 0, true, utc+hm+ud(hm));
        p(t, "123456+0100", 11, 0, 0, 0, wks, 0, 0, hms, 3600, false, utc+hms-z+ud(hms));
        p(t, "123456+01", 9, 0, 0, 0, wks, 0, 0, hms, 3600, false, utc+hms-z+ud(hms));
        p(t, "1234+0100", 9, 0, 0, 0, wks, 0, 0, hm, 3600, false, utc+hm-z+ud(hm));
        p(t, "1234+01", 7, 0, 0, 0, wks, 0, 0, hm, 3600, false, utc+hm-z+ud(hm));
        p(t, "123456", 6, 0, 0, 0, wks, 0, 0, hms, 0, false, lt(hms));
        p(t, "1234", 4, 0, 0, 0, wks, 0, 0, hm, 0, false, lt(hm));
        p(t, "20131206", 8, year, mo, 0, wks, 0, 6, 0, 0, false, local6);
        p(t, "2013340", 7, year, 0, 0, wks, 340, 0, 0, 0, false, local6);
        p(t, "2013W495", 8, year, 0, 49, 5, 0, 0, 0, 0, false, local6);
        p(t, "2013W49", 7, year, 0, 49, wks, 0, 0, 0, 0, false, local1);
        p(t, "201312", 6, year, mo, 0, wks, 0, 1, 0, 0, false, local1 - 86400 * wks);
        p(t, "20131206T123456", 15, year, mo, 0, wks, 0, 6, hms, 0, false, local6+hms);
        p(t, "20131206T1234", 13, year, mo, 0, wks, 0, 6, hm, 0, false, local6+hm);
        p(t, "2013340T123456", 14, year, 0, 0, wks, 340, 0, hms, 0, false, local6+hms);
        p(t, "2013340T1234", 12, year, 0, 0, wks, 340, 0, hm, 0, false, local6+hm);
        p(t, "2013W495T123456", 15, year, 0, 49, 5, 0, 0, hms, 0, false, local6+hms);
        p(t, "2013W495T1234", 13, year, 0, 49, 5, 0, 0, hm, 0, false, local6+hm);
        p(t, "2013W49T123456", 14, year, 0, 49, wks, 0, 0, hms, 0, false, local1+hms);
        p(t, "2013W49T1234", 12, year, 0, 49, wks, 0, 0, hm, 0, false, local1+hm);
        p(t, "20131206T123456Z", 16, year, mo, 0, wks, 0, 6, hms, 0, true, utc6+hms);
        p(t, "20131206T1234Z", 14, year, mo, 0, wks, 0, 6, hm, 0, true, utc6+hm);
        p(t, "2013340T123456Z", 15, year, 0, 0, wks, 340, 0, hms, 0, true, utc6+hms);
        p(t, "2013340T1234Z", 13, year, 0, 0, wks, 340, 0, hm, 0, true, utc6+hm);
        p(t, "2013W495T123456Z", 16, year, 0, 49, 5, 0, 0, hms, 0, true, utc6+hms);
        p(t, "2013W495T1234Z", 14, year, 0, 49, 5, 0, 0, hm, 0, true, utc6+hm);
        p(t, "2013W49T123456Z", 15, year, 0, 49, wks, 0, 0, hms, 0, true, utc1+hms);
        p(t, "2013W49T1234Z", 13, year, 0, 49, wks, 0, 0, hm, 0, true, utc1+hm);
        p(t, "20131206T123456+0100", 20, year, mo, 0, wks, 0, 6, hms, 3600, false, utc6+hms-z);
        p(t, "20131206T123456+01", 18, year, mo, 0, wks, 0, 6, hms, 3600, false, utc6+hms-z);
        p(t, "20131206T123456-0100", 20, year, mo, 0, wks, 0, 6, hms, -3600, false, utc6+hms+z);
        p(t, "20131206T123456-01", 18, year, mo, 0, wks, 0, 6, hms, -3600, false, utc6+hms+z);
        p(t, "20131206T1234+0100", 18, year, mo, 0, wks, 0, 6, hm, 3600, false, utc6+hm-z);
        p(t, "20131206T1234+01", 16, year, mo, 0, wks, 0, 6, hm, 3600, false, utc6+hm-z);
        p(t, "20131206T1234-0100", 18, year, mo, 0, wks, 0, 6, hm, -3600, false, utc6+hm+z);
        p(t, "20131206T1234-01", 16, year, mo, 0, wks, 0, 6, hm, -3600, false, utc6+hm+z);
        p(t, "2013340T123456+0100", 19, year, 0, 0, wks, 340, 0, hms, 3600, false, utc6+hms-z);
        p(t, "2013340T123456+01", 17, year, 0, 0, wks, 340, 0, hms, 3600, false, utc6+hms-z);
        p(t, "2013340T123456-0100", 19, year, 0, 0, wks, 340, 0, hms, -3600, false, utc6+hms+z);
        p(t, "2013340T123456-01", 17, year, 0, 0, wks, 340, 0, hms, -3600, false, utc6+hms+z);
        p(t, "2013340T1234+0100", 17, year, 0, 0, wks, 340, 0, hm, 3600, false, utc6+hm-z);
        p(t, "2013340T1234+01", 15, year, 0, 0, wks, 340, 0, hm, 3600, false, utc6+hm-z);
        p(t, "2013340T1234-0100", 17, year, 0, 0, wks, 340, 0, hm, -3600, false, utc6+hm+z);
        p(t, "2013340T1234-01", 15, year, 0, 0, wks, 340, 0, hm, -3600, false, utc6+hm+z);
        p(t, "2013W495T123456+0100", 20, year, 0, 49, 5, 0, 0, hms, 3600, false, utc6+hms-z);
        p(t, "2013W495T123456+01", 18, year, 0, 49, 5, 0, 0, hms, 3600, false, utc6+hms-z);
        p(t, "2013W495T123456-0100", 20, year, 0, 49, 5, 0, 0, hms, -3600, false, utc6+hms+z);
        p(t, "2013W495T123456-01", 18, year, 0, 49, 5, 0, 0, hms, -3600, false, utc6+hms+z);
        p(t, "2013W495T1234+0100", 18, year, 0, 49, 5, 0, 0, hm, 3600, false, utc6+hm-z);
        p(t, "2013W495T1234+01", 16, year, 0, 49, 5, 0, 0, hm, 3600, false, utc6+hm-z);
        p(t, "2013W495T1234-0100", 18, year, 0, 49, 5, 0, 0, hm, -3600, false, utc6+hm+z);
        p(t, "2013W495T1234-01", 16, year, 0, 49, 5, 0, 0, hm, -3600, false, utc6+hm+z);
        p(t, "2013W49T123456+0100", 19, year, 0, 49, wks, 0, 0, hms, 3600, false, utc1+hms-z);
        p(t, "2013W49T123456+01", 17, year, 0, 49, wks, 0, 0, hms, 3600, false, utc1+hms-z);
        p(t, "2013W49T123456-0100", 19, year, 0, 49, wks, 0, 0, hms, -3600, false, utc1+hms+z);
        p(t, "2013W49T123456-01", 17, year, 0, 49, wks, 0, 0, hms, -3600, false, utc1+hms+z);
        p(t, "2013W49T1234+0100", 17, year, 0, 49, wks, 0, 0, hm, 3600, false, utc1+hm-z);
        p(t, "2013W49T1234+01", 15, year, 0, 49, wks, 0, 0, hm, 3600, false, utc1+hm-z);
        p(t, "2013W49T1234-0100", 17, year, 0, 49, wks, 0, 0, hm, -3600, false, utc1+hm+z);
        p(t, "2013W49T1234-01", 15, year, 0, 49, wks, 0, 0, hm, -3600, false, utc1+hm+z);
        p(t, "98501206", 8, f_yr, mo, 0, wks, 0, 6, 0, 0, false, f_local6);
        p(t, "9850340", 7, f_yr, 0, 0, wks, 340, 0, 0, 0, false, f_local6);
        p(t, "9850W495", 8, f_yr, 0, 49, 5, 0, 0, 0, 0, false, f_local6);
        p(t, "9850W49", 7, f_yr, 0, 49, wks, 0, 0, 0, 0, false, f_local1);
        p(t, "985012", 6, f_yr, mo, 0, wks, 0, 1, 0, 0, false, f_local1 - 86400 * wks);
        p(t, "98501206T123456", 15, f_yr, mo, 0, wks, 0, 6, hms, 0, false, f_local6+hms);
        p(t, "98501206T1234", 13, f_yr, mo, 0, wks, 0, 6, hm, 0, false, f_local6+hm);
        p(t, "9850340T123456", 14, f_yr, 0, 0, wks, 340, 0, hms, 0, false, f_local6+hms);
        p(t, "9850340T1234", 12, f_yr, 0, 0, wks, 340, 0, hm, 0, false, f_local6+hm);
        p(t, "9850W495T123456", 15, f_yr, 0, 49, 5, 0, 0, hms, 0, false, f_local6+hms);
        p(t, "9850W495T1234", 13, f_yr, 0, 49, 5, 0, 0, hm, 0, false, f_local6+hm);
        p(t, "9850W49T123456", 14, f_yr, 0, 49, wks, 0, 0, hms, 0, false, f_local1+hms);
        p(t, "9850W49T1234", 12, f_yr, 0, 49, wks, 0, 0, hm, 0, false, f_local1+hm);
        p(t, "98501206T123456Z", 16, f_yr, mo, 0, wks, 0, 6, hms, 0, true, f_utc6+hms);
        p(t, "98501206T1234Z", 14, f_yr, mo, 0, wks, 0, 6, hm, 0, true, f_utc6+hm);
        p(t, "9850340T123456Z", 15, f_yr, 0, 0, wks, 340, 0, hms, 0, true, f_utc6+hms);
        p(t, "9850340T1234Z", 13, f_yr, 0, 0, wks, 340, 0, hm, 0, true, f_utc6+hm);
        p(t, "9850W495T123456Z", 16, f_yr, 0, 49, 5, 0, 0, hms, 0, true, f_utc6+hms);
        p(t, "9850W495T1234Z", 14, f_yr, 0, 49, 5, 0, 0, hm, 0, true, f_utc6+hm);
        p(t, "9850W49T123456Z", 15, f_yr, 0, 49, wks, 0, 0, hms, 0, true, f_utc1+hms);
        p(t, "9850W49T1234Z", 13, f_yr, 0, 49, wks, 0, 0, hm, 0, true, f_utc1+hm);
        p(t, "98501206T123456+0100", 20, f_yr, mo, 0, wks, 0, 6, hms, 3600, false, f_utc6+hms-z);
        p(t, "98501206T123456+01", 18, f_yr, mo, 0, wks, 0, 6, hms, 3600, false, f_utc6+hms-z);
        p(t, "98501206T123456-0100", 20, f_yr, mo, 0, wks, 0, 6, hms, -3600, false, f_utc6+hms+z);
        p(t, "98501206T123456-01", 18, f_yr, mo, 0, wks, 0, 6, hms, -3600, false, f_utc6+hms+z);
        p(t, "98501206T1234+0100", 18, f_yr, mo, 0, wks, 0, 6, hm, 3600, false, f_utc6+hm-z);
        p(t, "98501206T1234+01", 16, f_yr, mo, 0, wks, 0, 6, hm, 3600, false, f_utc6+hm-z);
        p(t, "98501206T1234-0100", 18, f_yr, mo, 0, wks, 0, 6, hm, -3600, false, f_utc6+hm+z);
        p(t, "98501206T1234-01", 16, f_yr, mo, 0, wks, 0, 6, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850340T123456+0100", 19, f_yr, 0, 0, wks, 340, 0, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850340T123456+01", 17, f_yr, 0, 0, wks, 340, 0, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850340T123456-0100", 19, f_yr, 0, 0, wks, 340, 0, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850340T123456-01", 17, f_yr, 0, 0, wks, 340, 0, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850340T1234+0100", 17, f_yr, 0, 0, wks, 340, 0, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850340T1234+01", 15, f_yr, 0, 0, wks, 340, 0, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850340T1234-0100", 17, f_yr, 0, 0, wks, 340, 0, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850340T1234-01", 15, f_yr, 0, 0, wks, 340, 0, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850W495T123456+0100", 20, f_yr, 0, 49, 5, 0, 0, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850W495T123456+01", 18, f_yr, 0, 49, 5, 0, 0, hms, 3600, false, f_utc6+hms-z);
        p(t, "9850W495T123456-0100", 20, f_yr, 0, 49, 5, 0, 0, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850W495T123456-01", 18, f_yr, 0, 49, 5, 0, 0, hms, -3600, false, f_utc6+hms+z);
        p(t, "9850W495T1234+0100", 18, f_yr, 0, 49, 5, 0, 0, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850W495T1234+01", 16, f_yr, 0, 49, 5, 0, 0, hm, 3600, false, f_utc6+hm-z);
        p(t, "9850W495T1234-0100", 18, f_yr, 0, 49, 5, 0, 0, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850W495T1234-01", 16, f_yr, 0, 49, 5, 0, 0, hm, -3600, false, f_utc6+hm+z);
        p(t, "9850W49T123456+0100", 19, f_yr, 0, 49, wks, 0, 0, hms, 3600, false, f_utc1+hms-z);
        p(t, "9850W49T123456+01", 17, f_yr, 0, 49, wks, 0, 0, hms, 3600, false, f_utc1+hms-z);
        p(t, "9850W49T123456-0100", 19, f_yr, 0, 49, wks, 0, 0, hms, -3600, false, f_utc1+hms+z);
        p(t, "9850W49T123456-01", 17, f_yr, 0, 49, wks, 0, 0, hms, -3600, false, f_utc1+hms+z);
        p(t, "9850W49T1234+0100", 17, f_yr, 0, 49, wks, 0, 0, hm, 3600, false, f_utc1+hm-z);
        p(t, "9850W49T1234+01", 15, f_yr, 0, 49, wks, 0, 0, hm, 3600, false, f_utc1+hm-z);
        p(t, "9850W49T1234-0100", 17, f_yr, 0, 49, wks, 0, 0, hm, -3600, false, f_utc1+hm+z);
        p(t, "9850W49T1234-01", 15, f_yr, 0, 49, wks, 0, 0, hm, -3600, false, f_utc1+hm+z);

        // Informal time.
        let t8a: i64 = 8 * 3600;
        let t830a: i64 = (8 * 3600) + (30 * 60);
        let t8p: i64 = 20 * 3600;
        let t830p: i64 = (20 * 3600) + (30 * 60);
        let t12p: i64 = 12 * 3600;
        let t1p: i64 = 13 * 3600;

        // Deviation: `adjust` compares the exact seconds of "now" with the
        // time (initializeInformalTime: `calc_seconds < now_seconds`); the C++
        // compares hours and minutes only.
        let time_now = Datetime::new();
        let now_s = (time_now.hour() * 3600 + time_now.minute() * 60 + time_now.second()) as i64;
        let adjust = |calc: i64| if now_s > calc { 86400 } else { 0 };
        let local_plus = |secs: i64| r.local_plus(secs);

        p(t, "10:30am", 7, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t830a + adjust(t830a + 2 * 3600) + (2 * 3600)));

        p(t, "8:30am", 6, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t830a + adjust(t830a)));
        p(t, "8:30a", 5, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t830a + adjust(t830a)));
        p(t, "8:30", 4, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t830a + adjust(t830a)));

        p(t, "8am", 3, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t8a + adjust(t8a)));
        p(t, "8a", 2, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t8a + adjust(t8a)));

        p(t, "8:30pm", 6, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t830p + adjust(t830p)));
        p(t, "8:30p", 5, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t830p + adjust(t830p)));

        p(t, "8pm", 3, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t8p + adjust(t8p)));
        p(t, "8p", 2, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t8p + adjust(t8p)));

        p(t, "12pm", 4, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t12p + adjust(t12p)));

        p(t, "1pm", 3, 0, 0, 0, wks, 0, 0, 0, 0, false, local_plus(t1p + adjust(t1p)));
    }
}

/// Deviation: the C++ `now + k * 86400` used with `sameDay`; computed on the
/// calendar (noon of the k-th following day) so a DST transition within the
/// k days cannot move it across midnight.
fn plus_days(now: &Datetime, k: i64) -> Datetime {
    let mut tm = localtime::localtime(now.to_epoch());
    tm.tm_mday += k as i32;
    tm.tm_hour = 12;
    tm.tm_min = 0;
    tm.tm_sec = 0;
    tm.tm_isdst = -1;
    Datetime::from_epoch(localtime::mktime(&mut tm))
}

/// datetime.t.cpp `main`.
fn datetime_t(t: &mut UnitTest) {
    let mut iso = Datetime::new();
    let mut start = 0usize;
    t.notok(iso.parse("foo", &mut start, ""), "foo --> false");
    t.is(start as i32, 0, "foo[0]");

    let r = Reference::compute(t);
    parse_table(t, &r);

    // try
    let now = Datetime::new();
    t.ok(!now.to_iso().contains("1969"), "'now' != 1969");

    let mut yesterday = Datetime::new();
    yesterday -= 86400;
    let mut tomorrow = Datetime::new();
    tomorrow += 86400;

    t.ok(yesterday <= now, "yesterday <= now");
    t.ok(yesterday < now, "yesterday < now");
    t.notok(yesterday == now, "!(yesterday == now)");
    t.ok(yesterday != now, "yesterday != now");
    t.ok(now >= yesterday, "now >= yesterday");
    t.ok(now > yesterday, "now > yesterday");

    t.ok(tomorrow >= now, "tomorrow >= now");
    t.ok(tomorrow > now, "tomorrow > now");
    t.notok(tomorrow == now, "!(tomorrow == now)");
    t.ok(tomorrow != now, "tomorrow != now");
    t.ok(now <= tomorrow, "now <= tomorrow");
    t.ok(now < tomorrow, "now < tomorrow");

    // ctor ("now")
    Datetime::set_weekstart(1);
    let relative_now = Datetime::new();
    t.ok(relative_now.same_hour(&now), "Datetime ().sameHour (Datetime (now))");
    t.ok(relative_now.same_day(&now), "Datetime ().sameDay (Datetime (now))");
    t.ok(relative_now.same_week(&now), "Datetime ().sameWeek (Datetime (now))");
    t.ok(relative_now.same_month(&now), "Datetime ().sameMonth (Datetime (now))");
    t.ok(relative_now.same_year(&now), "Datetime ().sameYear (Datetime (now))");

    // Loose comparisons.
    let left = dt(t, "7/4/2008", "m/d/Y");
    let comp1 = dt(t, "7/4/2008", "m/d/Y");
    t.ok(left.same_day(&comp1), "7/4/2008 is on the same day as 7/4/2008");
    t.ok(left.same_week(&comp1), "7/4/2008 is on the same week as 7/4/2008");
    t.ok(left.same_month(&comp1), "7/4/2008 is in the same month as 7/4/2008");
    t.ok(left.same_quarter(&comp1), "7/4/2008 is in the same quarter as 7/4/2008");
    t.ok(left.same_year(&comp1), "7/4/2008 is in the same year as 7/4/2008");

    let comp2 = dt(t, "7/5/2008", "m/d/Y");
    t.notok(left.same_day(&comp2), "7/4/2008 is not on the same day as 7/5/2008");
    t.ok(left.same_month(&comp2), "7/4/2008 is in the same month as 7/5/2008");
    t.ok(left.same_quarter(&comp2), "7/4/2008 is in the same quarter as 7/5/2008");
    t.ok(left.same_year(&comp2), "7/4/2008 is in the same year as 7/5/2008");

    let comp3 = dt(t, "8/4/2008", "m/d/Y");
    t.notok(left.same_day(&comp3), "7/4/2008 is not on the same day as 8/4/2008");
    t.notok(left.same_week(&comp3), "7/4/2008 is not on the same week as 8/4/2008");
    t.notok(left.same_month(&comp3), "7/4/2008 is not in the same month as 8/4/2008");
    t.ok(left.same_quarter(&comp3), "7/4/2008 is in the same quarter as 8/4/2008");
    t.ok(left.same_year(&comp3), "7/4/2008 is in the same year as 8/4/2008");

    let comp4 = dt(t, "7/4/2009", "m/d/Y");
    t.notok(left.same_day(&comp4), "7/4/2008 is not on the same day as 7/4/2009");
    t.notok(left.same_week(&comp4), "7/4/2008 is not on the same week as 7/4/2009");
    t.notok(left.same_month(&comp4), "7/4/2008 is not in the same month as 7/4/2009");
    t.notok(left.same_quarter(&comp4), "7/4/2008 is not in the same quarter as 7/4/2009");
    t.notok(left.same_year(&comp4), "7/4/2008 is not in the same year as 7/4/2009");

    // Validity.
    t.ok(Datetime::valid_ymd(2008, 2, 29), "valid: 2/29/2008");
    t.notok(Datetime::valid_ymd(2007, 2, 29), "invalid: 2/29/2007");

    t.ok(Datetime::valid_str("2/29/2008", "m/d/Y"), "valid: 2/29/2008");
    t.notok(Datetime::valid_str("2/29/2007", "m/d/Y"), "invalid: 2/29/2007");

    t.ok(Datetime::valid_yd(2008, 366), "valid: 366 days in 2008");
    t.notok(Datetime::valid_yd(2007, 366), "invalid: 366 days in 2007");

    // Time validity.
    t.ok(Datetime::valid_ymdhms(2010, 2, 28, 0, 0, 0), "valid 2/28/2010 0:00:00");
    t.ok(Datetime::valid_ymdhms(2010, 2, 28, 23, 59, 59), "valid 2/28/2010 23:59:59");
    t.notok(Datetime::valid_ymdhms(2010, 2, 28, 24, 59, 59), "valid 2/28/2010 24:59:59");
    t.notok(Datetime::valid_ymdhms(2010, 2, 28, -1, 0, 0), "valid 2/28/2010 -1:00:00");

    // Leap year.
    t.ok(Datetime::leap_year(2008), "2008 is a leap year");
    t.notok(Datetime::leap_year(2007), "2007 is not a leap year");
    t.ok(Datetime::leap_year(2000), "2000 is a leap year");
    t.notok(Datetime::leap_year(1900), "1900 is not a leap year");

    // Days in year.
    t.is(Datetime::days_in_year(2016), 366, "366 days in 2016");
    t.is(Datetime::days_in_year(2015), 365, "365 days in 2015");

    // Days in month.
    t.is(Datetime::days_in_month(2008, 2), 29, "29 days in February 2008");
    t.is(Datetime::days_in_month(2007, 2), 28, "28 days in February 2007");

    // Names.
    t.is(Datetime::month_name(1), "January", "1 = January");
    t.is(Datetime::month_name(2), "February", "2 = February");
    t.is(Datetime::month_name(3), "March", "3 = March");
    t.is(Datetime::month_name(4), "April", "4 = April");
    t.is(Datetime::month_name(5), "May", "5 = May");
    t.is(Datetime::month_name(6), "June", "6 = June");
    t.is(Datetime::month_name(7), "July", "7 = July");
    t.is(Datetime::month_name(8), "August", "8 = August");
    t.is(Datetime::month_name(9), "September", "9 = September");
    t.is(Datetime::month_name(10), "October", "10 = October");
    t.is(Datetime::month_name(11), "November", "11 = November");
    t.is(Datetime::month_name(12), "December", "12 = December");

    // Names.
    t.is(Datetime::month_name_short(1), "Jan", "1 = Jan");
    t.is(Datetime::month_name_short(2), "Feb", "2 = Feb");
    t.is(Datetime::month_name_short(3), "Mar", "3 = Mar");
    t.is(Datetime::month_name_short(4), "Apr", "4 = Apr");
    t.is(Datetime::month_name_short(5), "May", "5 = May");
    t.is(Datetime::month_name_short(6), "Jun", "6 = Jun");
    t.is(Datetime::month_name_short(7), "Jul", "7 = Jul");
    t.is(Datetime::month_name_short(8), "Aug", "8 = Aug");
    t.is(Datetime::month_name_short(9), "Sep", "9 = Sep");
    t.is(Datetime::month_name_short(10), "Oct", "10 = Oct");
    t.is(Datetime::month_name_short(11), "Nov", "11 = Nov");
    t.is(Datetime::month_name_short(12), "Dec", "12 = Dec");

    // Names.
    t.is(Datetime::month_of_year("January"), 1, "January   =  1");
    t.is(Datetime::month_of_year("February"), 2, "February  =  2");
    t.is(Datetime::month_of_year("March"), 3, "March     =  3");
    t.is(Datetime::month_of_year("April"), 4, "April     =  4");
    t.is(Datetime::month_of_year("May"), 5, "May       =  5");
    t.is(Datetime::month_of_year("June"), 6, "June      =  6");
    t.is(Datetime::month_of_year("July"), 7, "July      =  7");
    t.is(Datetime::month_of_year("August"), 8, "August    =  8");
    t.is(Datetime::month_of_year("September"), 9, "September =  9");
    t.is(Datetime::month_of_year("October"), 10, "October   = 10");
    t.is(Datetime::month_of_year("November"), 11, "November  = 11");
    t.is(Datetime::month_of_year("December"), 12, "December  = 12");

    t.is(Datetime::day_name(0), "Sunday", "0 == Sunday");
    t.is(Datetime::day_name(1), "Monday", "1 == Monday");
    t.is(Datetime::day_name(2), "Tuesday", "2 == Tuesday");
    t.is(Datetime::day_name(3), "Wednesday", "3 == Wednesday");
    t.is(Datetime::day_name(4), "Thursday", "4 == Thursday");
    t.is(Datetime::day_name(5), "Friday", "5 == Friday");
    t.is(Datetime::day_name(6), "Saturday", "6 == Saturday");

    t.is(Datetime::day_name_short(0), "Sun", "0 == Sun");
    t.is(Datetime::day_name_short(1), "Mon", "1 == Mon");
    t.is(Datetime::day_name_short(2), "Tue", "2 == Tue");
    t.is(Datetime::day_name_short(3), "Wed", "3 == Wed");
    t.is(Datetime::day_name_short(4), "Thu", "4 == Thu");
    t.is(Datetime::day_name_short(5), "Fri", "5 == Fri");
    t.is(Datetime::day_name_short(6), "Sat", "6 == Sat");

    t.is(Datetime::day_of_week_str("SUNDAY"), 0, "SUNDAY == 0");
    t.is(Datetime::day_of_week_str("sunday"), 0, "sunday == 0");
    t.is(Datetime::day_of_week_str("Sunday"), 0, "Sunday == 0");
    t.is(Datetime::day_of_week_str("Monday"), 1, "Monday == 1");
    t.is(Datetime::day_of_week_str("Tuesday"), 2, "Tuesday == 2");
    t.is(Datetime::day_of_week_str("Wednesday"), 3, "Wednesday == 3");
    t.is(Datetime::day_of_week_str("Thursday"), 4, "Thursday == 4");
    t.is(Datetime::day_of_week_str("Friday"), 5, "Friday == 5");
    t.is(Datetime::day_of_week_str("Saturday"), 6, "Saturday == 6");

    let happy_new_year = Datetime::from_ymd(2008, 1, 1);
    t.is(happy_new_year.day_of_week(), 2, "1/1/2008 == Tuesday");
    t.is(happy_new_year.month(), 1, "1/1/2008 == January");
    t.is(happy_new_year.day(), 1, "1/1/2008 == 1");
    t.is(happy_new_year.year(), 2008, "1/1/2008 == 2008");
    t.is(happy_new_year.to_string("Y-M-D"), "2008-01-01", "toString 2008-01-01");

    let (mut m, mut d, mut y) = (0, 0, 0);
    happy_new_year.to_ymd(&mut y, &mut m, &mut d);
    t.is(m, 1, "1/1/2008 == January");
    t.is(d, 1, "1/1/2008 == 1");
    t.is(y, 2008, "1/1/2008 == 2008");

    let mut epoch = Datetime::from_ymd(2001, 9, 8);
    t.ok((epoch.to_epoch() as i32) < 1000000000, "9/8/2001 < 1,000,000,000");
    epoch += 172800;
    t.ok((epoch.to_epoch() as i32) > 1000000000, "9/10/2001 > 1,000,000,000");

    let from_epoch = Datetime::from_epoch(epoch.to_epoch());
    t.is(from_epoch.to_string("Y-M-D"), epoch.to_string("Y-M-D"), "ctor (time_t)");

    let iso = Datetime::from_epoch(1000000000);
    t.is(iso.to_iso(), "20010909T014640Z", "1,000,000,000 -> 20010909T014640Z");

    // Test for Y2038 problem
    let f_epoch = Datetime::from_ymd(2039, 1, 1);
    t.diag(format!("2039-1-1 is {}", f_epoch.to_epoch()));
    t.ok(f_epoch.to_epoch() > 2147483647, "9/01/2039 > 2,147,483,647");

    let f_iso = Datetime::from_epoch(2147483650);
    t.is(f_iso.to_iso(), "20380119T031410Z", "2147483650 -> 20380119T031410Z");

    // Test known-overlapped ISO8601 year boundaries, including leap year 1980.
    // Only makes sense for weekstart == 1 which activates the ISO8601 algorithm.
    // Dates are taken from the table at https://en.wikipedia.org/wiki/ISO_week_date
    Datetime::set_weekstart(1);
    //  input         y   m     mo  wk wd jul d sec tz utc    time_t
    p(t, "1976-W53-6", 10, 1976, 0, 53, 6, 0, 0, 0, 0, false, Datetime::from_ymd(1977, 1, 1).to_epoch());
    p(t, "1976-W53-6", 10, 1976, 0, 53, 6, 0, 0, 0, 0, false, Datetime::from_ymd(1977, 1, 1).to_epoch());
    p(t, "1976-W53-7", 10, 1976, 0, 53, 7, 0, 0, 0, 0, false, Datetime::from_ymd(1977, 1, 2).to_epoch());
    p(t, "1977-W52-6", 10, 1977, 0, 52, 6, 0, 0, 0, 0, false, Datetime::from_ymd(1977, 12, 31).to_epoch());
    p(t, "1977-W52-7", 10, 1977, 0, 52, 7, 0, 0, 0, 0, false, Datetime::from_ymd(1978, 1, 1).to_epoch());
    p(t, "1978-W01-1", 10, 1978, 0, 1, 1, 0, 0, 0, 0, false, Datetime::from_ymd(1978, 1, 2).to_epoch());
    p(t, "1978-W52-7", 10, 1978, 0, 52, 7, 0, 0, 0, 0, false, Datetime::from_ymd(1978, 12, 31).to_epoch());
    p(t, "1979-W01-1", 10, 1979, 0, 1, 1, 0, 0, 0, 0, false, Datetime::from_ymd(1979, 1, 1).to_epoch());
    p(t, "1979-W52-7", 10, 1979, 0, 52, 7, 0, 0, 0, 0, false, Datetime::from_ymd(1979, 12, 30).to_epoch());
    p(t, "1980-W01-1", 10, 1980, 0, 1, 1, 0, 0, 0, 0, false, Datetime::from_ymd(1979, 12, 31).to_epoch());
    p(t, "1980-W01-2", 10, 1980, 0, 1, 2, 0, 0, 0, 0, false, Datetime::from_ymd(1980, 1, 1).to_epoch());
    p(t, "1980-W52-7", 10, 1980, 0, 52, 7, 0, 0, 0, 0, false, Datetime::from_ymd(1980, 12, 28).to_epoch());
    p(t, "1981-W01-1", 10, 1981, 0, 1, 1, 0, 0, 0, 0, false, Datetime::from_ymd(1980, 12, 29).to_epoch());
    p(t, "1981-W01-2", 10, 1981, 0, 1, 2, 0, 0, 0, 0, false, Datetime::from_ymd(1980, 12, 30).to_epoch());
    p(t, "1981-W01-3", 10, 1981, 0, 1, 3, 0, 0, 0, 0, false, Datetime::from_ymd(1980, 12, 31).to_epoch());
    p(t, "1981-W01-4", 10, 1981, 0, 1, 4, 0, 0, 0, 0, false, Datetime::from_ymd(1981, 1, 1).to_epoch());
    p(t, "1981-W53-4", 10, 1981, 0, 53, 4, 0, 0, 0, 0, false, Datetime::from_ymd(1981, 12, 31).to_epoch());
    p(t, "1981-W53-5", 10, 1981, 0, 53, 5, 0, 0, 0, 0, false, Datetime::from_ymd(1982, 1, 1).to_epoch());
    p(t, "1981-W53-6", 10, 1981, 0, 53, 6, 0, 0, 0, 0, false, Datetime::from_ymd(1982, 1, 2).to_epoch());
    p(t, "1981-W53-7", 10, 1981, 0, 53, 7, 0, 0, 0, 0, false, Datetime::from_ymd(1982, 1, 3).to_epoch());

    // Quantization.
    let quant = Datetime::from_epoch(1234526400);
    // Deviation: 1234526400 is 2009-02-13 12:00:00 UTC, which is already
    // 2009-02-14 east of UTC+12 (Chatham); startOfDay is the local date.
    let quant_day = format!("{}000000", quant.to_string("YMD"));
    let expected_day = if quant.day() == 13 { "20090213000000" } else { quant_day.as_str() };
    t.is(quant.start_of_day().to_string("YMDHNS"), expected_day, "1234526400 -> 2/13/2009 12:00:00 UTC -> 2/13/2009 0:00:00");
    t.is(quant.start_of_week().to_string("YMDHNS"), "20090208000000", "1234526400 -> 2/13/2009 12:00:00 UTC -> 2/8/2009 0:00:00");
    t.is(quant.start_of_month().to_string("YMDHNS"), "20090201000000", "1234526400 -> 2/13/2009 12:00:00 UTC -> 2/1/2009 0:00:00");
    t.is(quant.start_of_year().to_string("YMDHNS"), "20090101000000", "1234526400 -> 2/13/2009 12:00:00 UTC -> 1/1/2009 0:00:00");

    // Format parsing.
    let from_string1 = dt(t, "1/1/2008", "m/d/Y");
    t.is(from_string1.month(), 1, "ctor (std::string) -> m");
    t.is(from_string1.day(), 1, "ctor (std::string) -> d");
    t.is(from_string1.year(), 2008, "ctor (std::string) -> y");

    let from_string2 = dt(t, "20080101", "YMD");
    t.is(from_string2.month(), 1, "ctor (std::string) -> m");
    t.is(from_string2.day(), 1, "ctor (std::string) -> d");
    t.is(from_string2.year(), 2008, "ctor (std::string) -> y");

    let from_string3 = dt(t, "12/31/2007", "m/d/Y");
    t.is(from_string3.month(), 12, "ctor (std::string) -> m");
    t.is(from_string3.day(), 31, "ctor (std::string) -> d");
    t.is(from_string3.year(), 2007, "ctor (std::string) -> y");

    let from_string4 = dt(t, "01/01/2008", "m/d/Y");
    t.is(from_string4.month(), 1, "ctor (std::string) -> m");
    t.is(from_string4.day(), 1, "ctor (std::string) -> d");
    t.is(from_string4.year(), 2008, "ctor (std::string) -> y");

    let from_string5 = dt(t, "Tue 05 Feb 2008 (06)", "a D b Y (V)");
    t.is(from_string5.month(), 2, "ctor (std::string) -> m");
    t.is(from_string5.day(), 5, "ctor (std::string) -> d");
    t.is(from_string5.year(), 2008, "ctor (std::string) -> y");

    let from_string6 = dt(t, "Tuesday, February 5, 2008", "A, B d, Y");
    t.is(from_string6.month(), 2, "ctor (std::string) -> m");
    t.is(from_string6.day(), 5, "ctor (std::string) -> d");
    t.is(from_string6.year(), 2008, "ctor (std::string) -> y");

    let from_string7 = dt(t, "w01 Tue 2008-01-01", "wV a Y-M-D");
    t.is(from_string7.month(), 1, "ctor (std::string) -> m");
    t.is(from_string7.day(), 1, "ctor (std::string) -> d");
    t.is(from_string7.year(), 2008, "ctor (std::string) -> y");

    let from_string8 = dt(t, "6/7/2010 1:23:45", "m/d/Y h:N:S");
    t.is(from_string8.month(), 6, "ctor (std::string) -> m");
    t.is(from_string8.day(), 7, "ctor (std::string) -> d");
    t.is(from_string8.year(), 2010, "ctor (std::string) -> Y");
    t.is(from_string8.hour(), 1, "ctor (std::string) -> h");
    t.is(from_string8.minute(), 23, "ctor (std::string) -> N");
    t.is(from_string8.second(), 45, "ctor (std::string) -> S");

    let from_string9 = dt(t, "6/7/2010 01:23:45", "m/d/Y H:N:S");
    t.is(from_string9.month(), 6, "ctor (std::string) -> m");
    t.is(from_string9.day(), 7, "ctor (std::string) -> d");
    t.is(from_string9.year(), 2010, "ctor (std::string) -> Y");
    t.is(from_string9.hour(), 1, "ctor (std::string) -> h");
    t.is(from_string9.minute(), 23, "ctor (std::string) -> N");
    t.is(from_string9.second(), 45, "ctor (std::string) -> S");

    let from_string10 = dt(t, "6/7/2010 12:34:56", "m/d/Y H:N:S");
    t.is(from_string10.month(), 6, "ctor (std::string) -> m");
    t.is(from_string10.day(), 7, "ctor (std::string) -> d");
    t.is(from_string10.year(), 2010, "ctor (std::string) -> Y");
    t.is(from_string10.hour(), 12, "ctor (std::string) -> h");
    t.is(from_string10.minute(), 34, "ctor (std::string) -> N");
    t.is(from_string10.second(), 56, "ctor (std::string) -> S");

    let from_string11 = dt(t, "6/7/3010 12:34:56", "m/d/Y H:N:S");
    t.is(from_string11.month(), 6, "ctor (std::string) -> m");
    t.is(from_string11.day(), 7, "ctor (std::string) -> d");
    t.is(from_string11.year(), 3010, "ctor (std::string) -> Y");
    t.is(from_string11.hour(), 12, "ctor (std::string) -> h");
    t.is(from_string11.minute(), 34, "ctor (std::string) -> N");
    t.is(from_string11.second(), 56, "ctor (std::string) -> S");

    // Day of year
    let doy = dt(t, "1/1/2011", "m/d/Y").day_of_year();
    t.is(doy, 1, "dayOfYear (1/1/2011)   ->   1");
    let doy = dt(t, "5/1/2011", "m/d/Y").day_of_year();
    t.is(doy, 121, "dayOfYear (5/1/2011)   -> 121");
    let doy = dt(t, "12/31/2011", "m/d/Y").day_of_year();
    t.is(doy, 365, "dayOfYear (12/31/2011) -> 365");

    let day_names = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

    // Relative dates - look ahead
    {
        Datetime::set_time_relative(true);

        let r1 = dt(t, "today", "");
        t.ok(r1.same_day(&now), "today = now");

        // r4 .. r10
        for (day, name) in day_names.iter().enumerate() {
            let day = day as i64;
            let r = dt(t, name, "");
            let dow = now.day_of_week() as i64;
            if dow >= day {
                t.ok(r.same_day(&plus_days(&now, day - dow + 7)), format!("{name} -> next {name}"));
            } else {
                t.ok(r.same_day(&plus_days(&now, day - dow)), format!("{name} -> next {name}"));
            }
        }
    }

    // Relative dates - look back
    {
        Datetime::set_time_relative(false);

        let r1 = dt(t, "today", "");
        t.ok(r1.same_day(&now), "today = now");

        for (day, name) in day_names.iter().enumerate() {
            let day = day as i64;
            let r = dt(t, name, "");
            let dow = now.day_of_week() as i64;
            if dow >= day {
                t.ok(r.same_day(&plus_days(&now, day - dow)), format!("{name} -> previous {name}"));
            } else {
                t.ok(r.same_day(&plus_days(&now, day - dow - 7)), format!("{name} -> previous {name}"));
            }
        }
    }

    let r11 = dt(t, "eow", "");
    t.ok(r11 < now + (8 * 86400), "eow < 7 days away");

    let r12 = dt(t, "eow", "");
    t.ok(r12 > now - (8 * 86400), "eow < 7 days in the past");

    let r16 = dt(t, "sonw", "");
    t.ok(r16 < now + (8 * 86400), "sonw < 7 days away");

    let r23 = dt(t, "sow", "");
    t.ok(r23 > now - (8 * 86400), "sow < 7 days in the past");

    let r17 = dt(t, "sonm", "");
    t.notok(r17.same_month(&now), "sonm not in same month as now");

    let r18 = dt(t, "som", "");
    t.ok(r18.same_month(&now), "som in same month as now");
    t.ok(r18.same_quarter(&now), "som in same quarter as now");

    let r19 = dt(t, "sony", "");
    t.notok(r19.same_year(&now), "sony not in same year as now");

    let r19a = dt(t, "soy", "");
    t.ok(r19a.same_year(&now), "soy in same year as now");
    t.ok(r19a < now, "soy < now");

    let r19b = dt(t, "eoy", "");
    t.ok(r19b > now, "eoy > now");

    let r19c = dt(t, "soq", "");
    t.ok(r19c.same_year(&now), "soq in same year as now");
    t.ok(r19c < now, "soq < now");

    let r19d = dt(t, "eoq", "");
    t.ok(r19d > now, "eoq > now");

    {
        Datetime::set_time_relative(true);

        let first = dt(t, "1st", "");
        t.diag(format!("actual (first - relative:true): {} now: {}", first.to_iso(), now.to_iso()));
        if now.day() >= 1 {
            t.notok(first.same_month(&now), "1st not in same month as now");
        } else {
            t.ok(first.same_month(&now), "1st in same month as now");
        }
        t.is(first.day(), 1, "1st day is 1");

        let second = dt(t, "2nd", "");
        t.diag(format!("actual (second - relative:true): {} now: {}", second.to_iso(), now.to_iso()));
        if now.day() >= 2 {
            t.notok(second.same_month(&now), "2nd not in same month as now");
        } else {
            t.ok(second.same_month(&now), "2nd in same month as now");
        }
        t.is(second.day(), 2, "2nd day is 2");

        let third = dt(t, "3rd", "");
        t.diag(format!("actual (third - relative:true): {} now: {}", third.to_iso(), now.to_iso()));
        if now.day() >= 3 {
            t.notok(third.same_month(&now), "3rd not in same month as now");
        } else {
            t.ok(third.same_month(&now), "3rd in same month as now");
        }
        t.is(third.day(), 3, "3rd day is 3");

        let fourth = dt(t, "4th", "");
        t.diag(format!("actual (fourth - relative:true): {} now: {}", fourth.to_iso(), now.to_iso()));
        if now.day() >= 4 {
            t.notok(fourth.same_month(&now), "4th not in same month as now");
        } else {
            t.ok(fourth.same_month(&now), "4th not in same month as now");
        }
        t.is(fourth.day(), 4, "4th day is 4");
    }

    {
        Datetime::set_time_relative(false);

        let first = dt(t, "1st", "");
        t.diag(format!("actual (first - relative:false): {} now: {}", first.to_iso(), now.to_iso()));
        if now.day() >= 1 {
            t.ok(first.month() == now.month(), "1st in same month as now");
        } else {
            t.ok((first.month() - now.month() - 12) % 12 == -1, "1st in previous month");
        }
        t.is(first.day(), 1, "1st day is 1");

        let second = dt(t, "2nd", "");
        t.diag(format!("actual (second - relative:false): {} now: {}", second.to_iso(), now.to_iso()));
        if now.day() >= 2 {
            t.ok(second.month() == now.month(), "2nd in same month as now");
        } else {
            t.ok((second.month() - now.month() - 12) % 12 == -1, "2nd in previous month");
        }
        t.is(second.day(), 2, "2nd day is 2");

        let third = dt(t, "3rd", "");
        t.diag(format!("actual (third - relative:false): {} now: {}", third.to_iso(), now.to_iso()));
        if now.day() >= 3 {
            t.ok(third.month() == now.month(), "3rd in same month as now");
        } else {
            t.ok((third.month() - now.month() - 12) % 12 == -1, "3rd in previous month");
        }
        t.is(third.day(), 3, "3rd day is 3");

        let fourth = dt(t, "4th", "");
        t.diag(format!("actual (fourth - relative:false): {} now: {}", fourth.to_iso(), now.to_iso()));
        if now.day() >= 4 {
            t.ok(fourth.month() == now.month(), "4th in same month as now");
        } else {
            t.ok((fourth.month() - now.month() - 12) % 12 == -1, "4th in previous month");
        }
        t.is(fourth.day(), 4, "4th day is 4");
    }

    let later = dt(t, "later", "");
    t.is(later.month(), 12, "later -> m = 12");
    t.is(later.day(), 30, "later -> d = 30");
    t.is(later.year(), 9999, "later -> y = 9999");

    // Quarters
    let soq = dt(t, "soq", "");
    let eoq = dt(t, "eoq", "");
    t.is(soq.day(), 1, "soq day is the first day of a quarter");
    t.is(eoq.day() / 10, 3, "eoq day is the last day of a quarter");
    t.is(soq.month() % 3, 1, "soq month is 1, 4, 7 or 10");
    t.is(eoq.month() % 3, 0, "eoq month is 3, 6, 9 or 12");

    // Note: these fail during the night of daylight savings end.
    t.ok(
        soq.same_year(&now) || (now.month() >= 10 && soq.year() == now.year() + 1),
        "soq is in same year as now",
    );

    // Datetime::sameHour
    let r20 = dt(t, "6/7/2010 01:00:00", "m/d/Y H:N:S");
    let r21 = dt(t, "6/7/2010 01:59:59", "m/d/Y H:N:S");
    t.ok(r20.same_hour(&r21), "two dates within the same hour");

    let r22 = dt(t, "6/7/2010 00:59:59", "m/d/Y H:N:S");
    t.notok(r20.same_hour(&r22), "two dates not within the same hour");

    // Datetime::operator-
    let r25 = Datetime::from_epoch(1234567890);
    t.is((r25 - 1).to_epoch(), 1234567889i64, "1234567890 - 1 = 1234567889");

    // Datetime::operator--
    let mut r26 = Datetime::from_ymdhms(2010, 11, 7, 23, 59, 59);
    r26.dec();
    t.is(r26.to_string("YMDHNS"), "20101106235959", "decrement across fall DST boundary");

    let mut r27 = Datetime::from_ymdhms(2010, 3, 14, 23, 59, 59);
    r27.dec();
    t.is(r27.to_string("YMDHNS"), "20100313235959", "decrement across spring DST boundary");

    // Datetime::operator++
    let mut r28 = Datetime::from_ymdhms(2010, 11, 6, 23, 59, 59);
    r28.inc();
    t.is(r28.to_string("YMDHNS"), "20101107235959", "increment across fall DST boundary");

    let mut r29 = Datetime::from_ymdhms(2010, 3, 13, 23, 59, 59);
    r29.inc();
    t.is(r29.to_string("YMDHNS"), "20100314235959", "increment across spring DST boundary");

    // int Datetime::length (const std::string&);
    t.is(Datetime::length("m"), 2, "length 'm' --> 2");
    t.is(Datetime::length("M"), 2, "length 'M' --> 2");
    t.is(Datetime::length("d"), 2, "length 'd' --> 2");
    t.is(Datetime::length("D"), 2, "length 'D' --> 2");
    t.is(Datetime::length("y"), 2, "length 'y' --> 2");
    t.is(Datetime::length("Y"), 4, "length 'Y' --> 4");
    t.is(Datetime::length("a"), 3, "length 'a' --> 3");
    t.is(Datetime::length("A"), 10, "length 'A' --> 10");
    t.is(Datetime::length("b"), 3, "length 'b' --> 3");
    t.is(Datetime::length("B"), 10, "length 'B' --> 10");
    t.is(Datetime::length("v"), 2, "length 'v' --> 2");
    t.is(Datetime::length("V"), 2, "length 'V' --> 2");
    t.is(Datetime::length("h"), 2, "length 'h' --> 2");
    t.is(Datetime::length("H"), 2, "length 'H' --> 2");
    t.is(Datetime::length("n"), 2, "length 'n' --> 2");
    t.is(Datetime::length("N"), 2, "length 'N' --> 2");
    t.is(Datetime::length("s"), 2, "length 's' --> 2");
    t.is(Datetime::length("S"), 2, "length 'S' --> 2");
    t.is(Datetime::length("j"), 3, "length 'j' --> 3");
    t.is(Datetime::length("J"), 3, "length 'J' --> 3");
    t.is(Datetime::length(" "), 1, "length ' ' --> 1");

    // Depletion requirement.
    let r30 = dt(t, "Mon Jun 30 2014", "a b D Y");
    t.is(r30.to_string("YMDHNS"), "20140630000000", "Depletion required on complex format with spaces");

    let r31 = dt(t, "Mon Jun 30 2014 xxx", "a b D Y");
    t.is(r31.to_string("YMDHNS"), "20140630000000", "Depletion not required on complex format with spaces");

    // Test all format options.
    let r32 = dt(t, "2015-10-28T12:55:00", "");
    t.is(r32.to_string("Y"), "2015", "2015-10-28T12:55:01 -> Y ->      2015");
    t.is(r32.to_string("y"), "15", "2015-10-28T12:55:01 -> y ->        15");
    t.is(r32.to_string("M"), "10", "2015-10-28T12:55:01 -> M ->        10");
    t.is(r32.to_string("m"), "10", "2015-10-28T12:55:01 -> m ->        10");
    t.is(r32.to_string("D"), "28", "2015-10-28T12:55:01 -> D ->        28");
    t.is(r32.to_string("d"), "28", "2015-10-28T12:55:01 -> d ->        28");
    t.is(r32.to_string("H"), "12", "2015-10-28T12:55:01 -> H ->        12");
    t.is(r32.to_string("h"), "12", "2015-10-28T12:55:01 -> h ->        12");
    t.is(r32.to_string("N"), "55", "2015-10-28T12:55:01 -> N ->        55");
    t.is(r32.to_string("n"), "55", "2015-10-28T12:55:01 -> n ->        55");
    t.is(r32.to_string("S"), "00", "2015-10-28T12:55:01 -> S ->        01");
    t.is(r32.to_string("s"), "0", "2015-10-28T12:55:01 -> s ->         1");
    t.is(r32.to_string("A"), "Wednesday", "2015-10-28T12:55:01 -> A -> Wednesday");
    t.is(r32.to_string("a"), "Wed", "2015-10-28T12:55:01 -> a ->       Wed");
    t.is(r32.to_string("B"), "October", "2015-10-28T12:55:01 -> B ->   October");
    t.is(r32.to_string("b"), "Oct", "2015-10-28T12:55:01 -> b ->       Oct");
    t.is(r32.to_string("V"), "44", "2015-10-28T12:55:01 -> V ->        44");
    t.is(r32.to_string("v"), "44", "2015-10-28T12:55:01 -> v ->        44");
    t.is(r32.to_string("J"), "301", "2015-10-28T12:55:01 -> J ->       301");
    t.is(r32.to_string("j"), "301", "2015-10-28T12:55:01 -> j ->       301");
    t.is(r32.to_string("w"), "3", "2015-10-28T12:55:01 -> w ->         3");

    // Test all parse options.
    let r33 = dt(t, "2015 10 28 19 28 01", "Y M D H N S");
    t.is(r33.year(), 2015, "Y works");
    t.is(r33.month(), 10, "M works");
    t.is(r33.day(), 28, "D works");
    t.is(r33.hour(), 19, "H works");
    t.is(r33.minute(), 28, "N works");
    t.is(r33.second(), 1, "S works");

    let r34 = dt(t, "15 5 4 3 2 1", "y m d h n s");
    t.is(r34.year(), 2015, "y works");
    t.is(r34.month(), 5, "m works");
    t.is(r34.day(), 4, "d works");
    t.is(r34.hour(), 3, "h works");
    t.is(r34.minute(), 2, "n works");
    t.is(r34.second(), 1, "s works");

    let r35 = dt(t, "Wednesday October 28 2015", "A B D Y");
    t.is(r35.year(), 2015, "Y works");
    t.is(r35.month(), 10, "B works");
    t.is(r35.day(), 28, "D works");
    t.is(r35.day_of_week(), 3, "A works");

    let r36 = dt(t, "Wed Oct 28 15", "a b d y");
    t.is(r36.year(), 2015, "y works");
    t.is(r36.month(), 10, "b works");
    t.is(r36.day(), 28, "d works");
    t.is(r36.day_of_week(), 3, "a works");

    let r37 = dt(t, "19th", "");
    t.is(r37.day(), 19, "'19th' --> 19");

    // Test all format options for a future date
    let r38 = dt(t, "9015-10-28T12:55:00", "");
    t.is(r38.to_string("Y"), "9015", "9015-10-28T12:55:01 -> Y ->      9015");
    t.is(r38.to_string("y"), "15", "9015-10-28T12:55:01 -> y ->        15");
    t.is(r38.to_string("M"), "10", "9015-10-28T12:55:01 -> M ->        10");
    t.is(r38.to_string("m"), "10", "9015-10-28T12:55:01 -> m ->        10");
    t.is(r38.to_string("D"), "28", "9015-10-28T12:55:01 -> D ->        28");
    t.is(r38.to_string("d"), "28", "9015-10-28T12:55:01 -> d ->        28");
    t.is(r38.to_string("H"), "12", "9015-10-28T12:55:01 -> H ->        12");
    t.is(r38.to_string("h"), "12", "9015-10-28T12:55:01 -> h ->        12");
    t.is(r38.to_string("N"), "55", "9015-10-28T12:55:01 -> N ->        55");
    t.is(r38.to_string("n"), "55", "9015-10-28T12:55:01 -> n ->        55");
    t.is(r38.to_string("S"), "00", "9015-10-28T12:55:01 -> S ->        01");
    t.is(r38.to_string("s"), "0", "9015-10-28T12:55:01 -> s ->         1");
    t.is(r38.to_string("A"), "Saturday", "9015-10-28T12:55:01 -> A ->  Saturday");
    t.is(r38.to_string("a"), "Sat", "9015-10-28T12:55:01 -> a ->       Sat");
    t.is(r38.to_string("B"), "October", "9015-10-28T12:55:01 -> B ->   October");
    t.is(r38.to_string("b"), "Oct", "9015-10-28T12:55:01 -> b ->       Oct");
    t.is(r38.to_string("V"), "43", "9015-10-28T12:55:01 -> V ->        43");
    t.is(r38.to_string("v"), "43", "9015-10-28T12:55:01 -> v ->        43");
    t.is(r38.to_string("J"), "301", "9015-10-28T12:55:01 -> J ->       301");
    t.is(r38.to_string("j"), "301", "9015-10-28T12:55:01 -> j ->       301");
    t.is(r38.to_string("w"), "6", "9015-10-28T12:55:01 -> w ->         6");

    // Phrases (commented out upstream):
    //   Datetime r42 ("4th thursday in november");

    // Embedded parsing.
    test_parse_error(t, "nowadays");
    test_parse(t, "now+1d");
    test_parse(t, "now-1d");
    test_parse(t, "now)");
    test_parse_error(t, "now7");
    test_parse_error(t, "tomorrov");

    test_parse_error(t, "yesteryear");
    test_parse(t, "yest+1d");
    test_parse(t, "yest-1d");
    test_parse(t, "yest)");
    test_parse_error(t, "yest7");
    test_parse(t, "yesterday");

    test_parse(t, "1234567890+0");
    test_parse(t, "1234567890-0");
    test_parse(t, "1234567890)");

    // Negative tests, all expected to fail.
    test_parse_error(t, "");
    test_parse_error(t, "foo");
    test_parse_error(t, "-2014-07-07");
    test_parse_error(t, "2014-07-");
    test_parse_error(t, "2014-0-12");
    test_parse_error(t, "abcd-ab-ab");
    test_parse_error(t, "2014-000");
    test_parse(t, "2014-001");
    test_parse(t, "2014-365");
    test_parse_error(t, "2014-366");
    test_parse_error(t, "2014-367");
    test_parse_error(t, "2014-999");
    test_parse_error(t, "2014-999999999");
    test_parse_error(t, "2014-W00");
    test_parse_error(t, "2014-W54");
    test_parse_error(t, "2014-W240");
    test_parse_error(t, "2014-W248");
    test_parse_error(t, "2014-W24200");
    // test_parse_error(t, "2014-00"); // (commented out upstream) Looks like Datetime::parse_time_off 'hhmm-hh'
    test_parse_error(t, "2014-13");
    test_parse_error(t, "2014-99");
    test_parse_error(t, "25:00");
    test_parse_error(t, "99:00");
    test_parse_error(t, "12:60");
    test_parse_error(t, "12:99");
    test_parse_error(t, "12:ab");
    test_parse_error(t, "ab:12");
    test_parse_error(t, "ab:cd");
    test_parse_error(t, "-12:12");
    test_parse_error(t, "12:-12");
    test_parse_error(t, "25:00Z");
    test_parse_error(t, "99:00Z");
    test_parse_error(t, "12:60Z");
    test_parse_error(t, "12:99Z");
    test_parse_error(t, "12:abZ");
    test_parse_error(t, "ab:12Z");
    test_parse_error(t, "ab:cdZ");
    test_parse_error(t, "-12:12Z");
    test_parse_error(t, "12:-12Z");
    test_parse_error(t, "25:00+01:00");
    test_parse_error(t, "99:00+01:00");
    test_parse_error(t, "12:60+01:00");
    test_parse_error(t, "12:99+01:00");
    test_parse_error(t, "12:ab+01:00");
    test_parse_error(t, "ab:12+01:00");
    test_parse_error(t, "ab:cd+01:00");
    test_parse_error(t, "-12:12+01:00");
    test_parse_error(t, "12:-12+01:00");
    test_parse_error(t, "25:00-01:00");
    test_parse_error(t, "99:00-01:00");
    test_parse_error(t, "12:60-01:00");
    test_parse_error(t, "12:99-01:00");
    test_parse_error(t, "12:ab-01:00");
    test_parse_error(t, "ab:12-01:00");
    test_parse_error(t, "ab:cd-01:00");
    test_parse_error(t, "-12:12-01:00");
    test_parse_error(t, "12:-12-01:00");
    test_parse_error(t, "25:00:00");
    test_parse_error(t, "99:00:00");
    test_parse_error(t, "12:60:00");
    test_parse_error(t, "12:99:00");
    test_parse_error(t, "12:12:60");
    test_parse_error(t, "12:12:99");
    test_parse_error(t, "12:ab:00");
    test_parse_error(t, "ab:12:00");
    test_parse_error(t, "12:12:ab");
    test_parse_error(t, "ab:cd:ef");
    test_parse_error(t, "-12:12:12");
    test_parse_error(t, "12:-12:12");
    test_parse_error(t, "12:12:-12");
    test_parse_error(t, "25:00:00Z");
    test_parse_error(t, "99:00:00Z");
    test_parse_error(t, "12:60:00Z");
    test_parse_error(t, "12:99:00Z");
    test_parse_error(t, "12:12:60Z");
    test_parse_error(t, "12:12:99Z");
    test_parse_error(t, "12:ab:00Z");
    test_parse_error(t, "ab:12:00Z");
    test_parse_error(t, "12:12:abZ");
    test_parse_error(t, "ab:cd:efZ");
    test_parse_error(t, "-12:12:12Z");
    test_parse_error(t, "12:-12:12Z");
    test_parse_error(t, "12:12:-12Z");
    test_parse_error(t, "25:00:00+01:00");
    test_parse_error(t, "95:00:00+01:00");
    test_parse_error(t, "12:60:00+01:00");
    test_parse_error(t, "12:99:00+01:00");
    test_parse_error(t, "12:12:60+01:00");
    test_parse_error(t, "12:12:99+01:00");
    test_parse_error(t, "12:ab:00+01:00");
    test_parse_error(t, "ab:12:00+01:00");
    test_parse_error(t, "12:12:ab+01:00");
    test_parse_error(t, "ab:cd:ef+01:00");
    test_parse_error(t, "-12:12:12+01:00");
    test_parse_error(t, "12:-12:12+01:00");
    test_parse_error(t, "12:12:-12+01:00");
    test_parse_error(t, "25:00:00-01:00");
    test_parse_error(t, "95:00:00-01:00");
    test_parse_error(t, "12:60:00-01:00");
    test_parse_error(t, "12:99:00-01:00");
    test_parse_error(t, "12:12:60-01:00");
    test_parse_error(t, "12:12:99-01:00");
    test_parse_error(t, "12:ab:00-01:00");
    test_parse_error(t, "ab:12:00-01:00");
    test_parse_error(t, "12:12:ab-01:00");
    test_parse_error(t, "ab:cd:ef-01:00");
    test_parse_error(t, "-12:12:12-01:00");
    test_parse_error(t, "12:-12:12-01:00");
    test_parse_error(t, "12:12:-12-01:00");
    test_parse_error(t, "12:12:12-13:00");
    test_parse_error(t, "12:12:12-24:00");
    test_parse_error(t, "12:12:12-99:00");
    test_parse_error(t, "12:12:12-03:60");
    test_parse_error(t, "12:12:12-03:99");
    test_parse_error(t, "12:12:12-3:20");
    test_parse_error(t, "12:12:12-03:2");
    test_parse_error(t, "12:12:12-3:2");
    test_parse_error(t, "12:12:12+13:00");
    test_parse_error(t, "12:12:12+24:00");
    test_parse_error(t, "12:12:12+99:00");
    test_parse_error(t, "12:12:12+03:60");
    test_parse_error(t, "12:12:12+03:99");
    test_parse_error(t, "12:12:12+3:20");
    test_parse_error(t, "12:12:12+03:2");
    test_parse_error(t, "12:12:12+3:2");
    test_parse_error(t, "12:12-13:00");
    test_parse_error(t, "12:12-24:00");
    test_parse_error(t, "12:12-99:00");
    test_parse_error(t, "12:12-03:60");
    test_parse_error(t, "12:12-03:99");
    test_parse_error(t, "12:12-3:20");
    test_parse_error(t, "12:12-03:2");
    test_parse_error(t, "12:12-3:2");
    test_parse_error(t, "12:12+13:00");
    test_parse_error(t, "12:12+24:00");
    test_parse_error(t, "12:12+99:00");
    test_parse_error(t, "12:12+03:60");
    test_parse_error(t, "12:12+03:99");
    test_parse_error(t, "12:12+3:20");
    test_parse_error(t, "12:12+03:2");
    test_parse_error(t, "12:12+3:2");

    // Test weekday validity under different weekstarts.
    Datetime::set_weekstart(0);
    test_parse_error(t, "2024-W01-7");
    Datetime::set_weekstart(1);
    test_parse_error(t, "2024-W01-0");

    // Test with standlalone date enable/disabled.
    Datetime::set_standalone_date_enabled(true);
    test_parse(t, "20170319");
    Datetime::set_standalone_date_enabled(false);
    test_parse_error(t, "20170319");
    Datetime::set_standalone_date_enabled(true);

    Datetime::set_standalone_time_enabled(true);
    test_parse(t, "235959");
    Datetime::set_standalone_time_enabled(false);
    test_parse_error(t, "235959");
    Datetime::set_standalone_time_enabled(true);

    // Weekdays and month names can no longer be followed by ':' or '='.
    test_parse(t, "jan");
    test_parse_error(t, "jan:");
    test_parse(t, "mon");
    test_parse_error(t, "mon:");

    {
        // Verify Datetime::timeRelative is working as expected.
        Datetime::set_time_relative(true);
        let today = Datetime::new();
        let r38 = dt(t, "0:00:01", "");
        // Deviation: during the first second of a day (00:00:00, 00:00:01)
        // 0:00:01 is not earlier than now, so it is today even when relative.
        let first_second = today.hour() == 0 && today.minute() == 0 && today.second() <= 1;
        if first_second {
            t.ok(today.same_day(&r38), "Datetime::timeRelative=true 0:00:01 --> tomorrow");
        } else {
            t.notok(today.same_day(&r38), "Datetime::timeRelative=true 0:00:01 --> tomorrow");
        }
        Datetime::set_time_relative(false);
        let r39 = dt(t, "0:00:01", "");
        t.ok(today.same_day(&r39), "Datetime::timeRelative=false 0:00:01 --> today");
        Datetime::set_time_relative(true);
    }

    let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

    {
        // Verify Datetime::timeRelative=true puts months before and including current month into next year
        Datetime::set_time_relative(true);

        let today = Datetime::new();

        for month in months {
            let test = dt(t, month, "");
            let is_after = test.month() > today.month();
            let message = format!(
                "Datetime::timeRelative=true --> {month} in {} year",
                if is_after { "same" } else { "next" }
            );
            t.ok(test.year() == today.year() + if is_after { 0 } else { 1 }, message);
        }
    }

    {
        // Verify Datetime::timeRelative=false puts months into current year
        Datetime::set_time_relative(false);

        let today = Datetime::new();

        for month in months {
            let test = dt(t, month, "");
            let message = format!("Datetime::timeRelative=false --> {month} in same year");
            t.ok(test.year() == today.year(), message);
        }
    }

    // This is just a diagnostic dump of all named dates, and is used to verify
    // correctness manually.
    t.diag("--------------------------------------------");
    for (label, name) in [
        ("  now            ", "now"),
        ("  yesterday      ", "yesterday"),
        ("  today          ", "today"),
        ("  tomorrow       ", "tomorrow"),
        ("  1st            ", "1st"),
        ("  monday         ", "monday"),
        ("  january        ", "january"),
        ("  later          ", "later"),
        ("  someday        ", "someday"),
        ("  sopd           ", "sopd"),
        ("  sod            ", "sod"),
        ("  sond           ", "sond"),
        ("  eopd           ", "eopd"),
        ("  eod            ", "eod"),
        ("  eond           ", "eond"),
        ("  sopw           ", "sopw"),
        ("  sow            ", "sow"),
        ("  sonw           ", "sonw"),
        ("  eopw           ", "eopw"),
        ("  eow            ", "eow"),
        ("  eonw           ", "eonw"),
        ("  sopww          ", "sopww"),
        ("  sonww          ", "sonww"),
        ("  soww           ", "soww"),
        ("  eopww          ", "eopww"),
        ("  eonww          ", "eonww"),
        ("  eoww           ", "eoww"),
        ("  sopm           ", "sopm"),
        ("  som            ", "som"),
        ("  sonm           ", "sonm"),
        ("  eopm           ", "eopm"),
        ("  eom            ", "eom"),
        ("  eonm           ", "eonm"),
        ("  sopq           ", "sopq"),
        ("  soq            ", "soq"),
        ("  sonq           ", "sonq"),
        ("  eopq           ", "eopq"),
        ("  eoq            ", "eoq"),
        ("  eonq           ", "eonq"),
        ("  sopy           ", "sopy"),
        ("  soy            ", "soy"),
        ("  sony           ", "sony"),
        ("  eopy           ", "eopy"),
        ("  eoy            ", "eoy"),
        ("  eony           ", "eony"),
        ("  easter         ", "easter"),
        ("  eastermonday   ", "eastermonday"),
        ("  ascension      ", "ascension"),
        ("  pentecost      ", "pentecost"),
        ("  goodfriday     ", "goodfriday"),
        ("  midsommar      ", "midsommar"),
        ("  midsommarafton ", "midsommarafton"),
        ("  juhannus       ", "juhannus"),
        ("  3am            ", "3am"),
        ("  12am           ", "12am"),
        ("  12pm           ", "12pm"),
        ("  1234567890     ", "1234567890"),
    ] {
        let value = dt(t, name, "").to_iso_local_extended();
        t.diag(format!("{label}{value}"));
    }
    t.diag("--------------------------------------------");
}

////////////////////////////////////////////////////////////////////////////////
// dates.t.cpp
////////////////////////////////////////////////////////////////////////////////

/// dates.t.cpp line 31: `testInit`.
fn test_init(t: &mut UnitTest, value: &str) -> Datetime {
    match Datetime::from_string(value, "") {
        Ok(var) => {
            t.pass(format!("{value} --> valid"));
            var
        }
        Err(e) => {
            t.fail(format!("{value} --> valid"));
            t.diag(e.to_string());
            Datetime::new()
        }
    }
}

/// dates.t.cpp `main`.
#[allow(clippy::eq_op)] // eoq == eoq, eow == eow, eoy == eoy are upstream assertions.
fn dates_t(t: &mut UnitTest) {
    let sunday = test_init(t, "sunday");
    let monday = test_init(t, "monday");
    let tuesday = test_init(t, "tuesday");
    let wednesday = test_init(t, "wednesday");
    let thursday = test_init(t, "thursday");
    let friday = test_init(t, "friday");
    let saturday = test_init(t, "saturday");

    let sun = test_init(t, "sun");
    let mon = test_init(t, "mon");
    let tue = test_init(t, "tue");
    let wed = test_init(t, "wed");
    let thu = test_init(t, "thu");
    let fri = test_init(t, "fri");
    let sat = test_init(t, "sat");

    t.ok(sunday == sun, "sunday == sun");
    t.ok(monday == mon, "monday == mon");
    t.ok(tuesday == tue, "tuesday == tue");
    t.ok(wednesday == wed, "wednesday == wed");
    t.ok(thursday == thu, "thursday == thu");
    t.ok(friday == fri, "friday == fri");
    t.ok(saturday == sat, "saturday == sat");

    let sunday_capital = test_init(t, "Sunday");
    let monday_capital = test_init(t, "Monday");
    let tuesday_capital = test_init(t, "Tuesday");
    let wednesday_capital = test_init(t, "Wednesday");
    let thursday_capital = test_init(t, "Thursday");
    let friday_capital = test_init(t, "Friday");
    let saturday_capital = test_init(t, "Saturday");

    t.ok(sunday_capital == sun, "Sunday == sun");
    t.ok(monday_capital == mon, "Monday == mon");
    t.ok(tuesday_capital == tue, "Tuesday == tue");
    t.ok(wednesday_capital == wed, "Wednesday == wed");
    t.ok(thursday_capital == thu, "Thursday == thu");
    t.ok(friday_capital == fri, "Friday == fri");
    t.ok(saturday_capital == sat, "Saturday == sat");

    let january = test_init(t, "january");
    let february = test_init(t, "february");
    let march = test_init(t, "march");
    let april = test_init(t, "april");
    let _may = test_init(t, "may");
    let june = test_init(t, "june");
    let july = test_init(t, "july");
    let august = test_init(t, "august");
    let september = test_init(t, "september");
    let october = test_init(t, "october");
    let november = test_init(t, "november");
    let december = test_init(t, "december");

    let jan = test_init(t, "jan");
    let feb = test_init(t, "feb");
    let mar = test_init(t, "mar");
    let apr = test_init(t, "apr");
    let jun = test_init(t, "jun");
    let jul = test_init(t, "jul");
    let aug = test_init(t, "aug");
    let sep = test_init(t, "sep");
    let oct = test_init(t, "oct");
    let nov = test_init(t, "nov");
    let dec = test_init(t, "dec");

    t.ok(january == jan, "january == jan");
    t.ok(february == feb, "february == feb");
    t.ok(march == mar, "march == mar");
    t.ok(april == apr, "april == apr");
    // May has only three letters.
    t.ok(june == jun, "june == jun");
    t.ok(july == jul, "july == jul");
    t.ok(august == aug, "august == aug");
    t.ok(september == sep, "september == sep");
    t.ok(october == oct, "october == oct");
    t.ok(november == nov, "november == nov");
    t.ok(december == dec, "december == dec");

    let january_capital = test_init(t, "January");
    let february_capital = test_init(t, "February");
    let march_capital = test_init(t, "March");
    let april_capital = test_init(t, "April");
    let _may_capital = test_init(t, "May");
    let june_capital = test_init(t, "June");
    let july_capital = test_init(t, "July");
    let august_capital = test_init(t, "August");
    let september_capital = test_init(t, "September");
    let october_capital = test_init(t, "October");
    let november_capital = test_init(t, "November");
    let december_capital = test_init(t, "December");

    t.ok(january_capital == jan, "January == jan");
    t.ok(february_capital == feb, "February == feb");
    t.ok(march_capital == mar, "March == mar");
    t.ok(april_capital == apr, "April == apr");
    // May has only three letters.
    t.ok(june_capital == jun, "June == jun");
    t.ok(july_capital == jul, "July == jul");
    t.ok(august_capital == aug, "August == aug");
    t.ok(september_capital == sep, "September == sep");
    t.ok(october_capital == oct, "October == oct");
    t.ok(november_capital == nov, "November == nov");
    t.ok(december_capital == dec, "December == dec");

    // Simply instantiate these for now.  Test later.
    let now = test_init(t, "now");
    let yesterday = test_init(t, "yesterday");
    let today = test_init(t, "today");
    let tomorrow = test_init(t, "tomorrow");
    let _first = test_init(t, "1st");
    let _second = test_init(t, "2nd");
    let _third = test_init(t, "3rd");
    let _fourth = test_init(t, "4th");
    let later = test_init(t, "later");
    let someday = test_init(t, "someday");
    let sopd = test_init(t, "sopd");
    let sond = test_init(t, "sond");
    let sod = test_init(t, "sod");
    let eopd = test_init(t, "eopd");
    let eond = test_init(t, "eond");
    let eod = test_init(t, "eod");
    let sopw = test_init(t, "sopw");
    let sonw = test_init(t, "sonw");
    let sow = test_init(t, "sow");
    let eopw = test_init(t, "eopw");
    let eonw = test_init(t, "eonw");
    let eow = test_init(t, "eow");
    let sopww = test_init(t, "sopww");
    let sonww = test_init(t, "sonww");
    let soww = test_init(t, "soww");
    let eopww = test_init(t, "eopww");
    let eonww = test_init(t, "eonww");
    let eoww = test_init(t, "eoww");
    let sopm = test_init(t, "sopm");
    let sonm = test_init(t, "sonm");
    let som = test_init(t, "som");
    let eopm = test_init(t, "eopm");
    let eonm = test_init(t, "eonm");
    let eom = test_init(t, "eom");
    let sopq = test_init(t, "sopq");
    let sonq = test_init(t, "sonq");
    let soq = test_init(t, "soq");
    let eopq = test_init(t, "eopq");
    let eonq = test_init(t, "eonq");
    let eoq = test_init(t, "eoq");
    let sopy = test_init(t, "sopy");
    let sony = test_init(t, "sony");
    let soy = test_init(t, "soy");
    let eopy = test_init(t, "eopy");
    let eony = test_init(t, "eony");
    let eoy = test_init(t, "eoy");
    let easter = test_init(t, "easter");
    let eastermonday = test_init(t, "eastermonday");
    let _ascension = test_init(t, "ascension");
    let _pentecost = test_init(t, "pentecost");
    let goodfriday = test_init(t, "goodfriday");

    let midsommar = test_init(t, "midsommar");
    let midsommarafton = test_init(t, "midsommarafton");
    let juhannus = test_init(t, "juhannus");

    // Check abbreviations.
    // TW-1515: abbreviation.minimum does not apply to date recognition
    let _yesterday2 = test_init(t, "yesterday");
    let _yesterday3 = test_init(t, "yesterda");
    let _yesterday4 = test_init(t, "yesterd");
    let _yesterday5 = test_init(t, "yester");
    let _yesterday6 = test_init(t, "yeste");
    let _yesterday7 = test_init(t, "yest");
    let _yesterday8 = test_init(t, "yes");

    {
        t.ok(eod < tomorrow, "eod  < tomorrow");
        t.ok(eom < sonm, "eom  < sonm");
        t.ok(eond > tomorrow, "eond > tomorrow");
        t.ok(eonm > sonm, "eonm > sonm");
        t.ok(eonq > sonq, "eonq > sonq");
        t.ok(eonw > sonw, "eonw > sonw");
        t.ok(eonww > sonww, "eonww > sonww");
        t.ok(eony > sony, "eony > sony");
        t.ok(eopd < today, "eopd < today");
        t.ok(eopm < som, "eopm < som");
        t.ok(eopq < soq, "eopq < soq");
        t.ok(eopw < sow, "eopw < sow");
        t.ok(eopww < soww, "eopww < soww");
        t.ok(eopy < soy, "eopy < soy");
        t.ok(eoq == eoq, "eoq == eoq");
        t.ok(eoq < sonq, "eoq < sonq");
        t.ok(eow == eow, "eow == eow");
        t.ok(eow < sonw, "eow < sonw");
        t.ok(eoww >= eopww, "eoww >= eopww");
        t.ok(eoy == eoy, "eoy == eoy");
        t.ok(eoy < sony, "eoy < sony");
        t.ok(later == someday, "later == someday");
        t.ok(now < later, "now < later");
        t.ok(now < someday, "now < someday");
        t.ok(now >= today, "now >= today");
        t.ok(sod == today, "sod == today");
        t.ok(som < eom, "som < eom");
        t.ok(sond == tomorrow, "sond == tomorrow");
        t.ok(sonq > eoq, "sonq > eoq");
        t.ok(sonq > soq, "sonq > soq");
        t.ok(sonw > eow, "sonw > eow");
        t.ok(sony > eoy, "sony > eoy");
        t.ok(sony > soy, "sony > soy");
        t.ok(sopd == yesterday, "sopd == yesterday");
        t.ok(sopm < som, "sopm < som");
        t.ok(sopq < soq, "sopq < soq");
        t.ok(sopw < sow, "sopw < sow");
        t.ok(sopww < soww, "sopww < soww");
        t.ok(sopy < soy, "sopy < soy");
        t.ok(sow < sonw, "sow < sonw");
        t.ok(soww < sonww, "soww < sonww");
        t.ok(soy > sopy, "soy > sopy");
        t.ok(today < tomorrow, "today < tomorrow");
        t.ok(yesterday < today, "yesterday < today");
        t.ok(goodfriday < easter, "goodfriday < easter");
        t.ok(easter < eastermonday, "easter < eastermonday");
    }

    // Deviation: on midsommarafton itself (after midnight) this year's
    // midsommarafton is already past, so initializeMidsommarafton rolls to
    // next year while midsommar (tomorrow) stays in this year; the upstream
    // assertion fails on that one day. There, assert the rollover instead.
    let mut afton = localtime::localtime(localtime::time_now());
    let today_tm = afton;
    Datetime::new().midsommarafton(&mut afton);
    localtime::mktime(&mut afton);
    let on_midsommarafton = (afton.tm_year, afton.tm_mon, afton.tm_mday) == (today_tm.tm_year, today_tm.tm_mon, today_tm.tm_mday);
    if on_midsommarafton {
        t.ok(
            midsommarafton > midsommar && midsommarafton.year() == midsommar.year() + 1,
            "midsommarafton < midsommar",
        );
    } else {
        t.ok(midsommarafton < midsommar, "midsommarafton < midsommar");
    }
    t.ok(juhannus == midsommarafton, "juhannus == midsommarafton");
}

////////////////////////////////////////////////////////////////////////////////

macro_rules! zone_tests {
    ($($name:ident => $tz:expr),+ $(,)?) => {
        mod datetime_t_cpp {
            use super::*;
            $(
                #[test]
                fn $name() {
                    run($tz, DATETIME_T_ASSERTIONS, datetime_t);
                }
            )+
        }

        mod dates_t_cpp {
            use super::*;
            $(
                #[test]
                fn $name() {
                    run($tz, DATES_T_ASSERTIONS, dates_t);
                }
            )+
        }

        #[test]
        fn zone_list_is_complete() {
            let names: Vec<&str> = vec![$($tz),+];
            assert_eq!(names, ZONES);
        }
    };
}

zone_tests! {
    utc => "UTC",
    america_new_york => "America/New_York",
    europe_berlin => "Europe/Berlin",
    asia_kolkata => "Asia/Kolkata",
    australia_lord_howe => "Australia/Lord_Howe",
    pacific_chatham => "Pacific/Chatham",
}
