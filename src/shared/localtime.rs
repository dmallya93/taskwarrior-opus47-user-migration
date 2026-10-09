//! The libc time boundary used by [`crate::shared::datetime`] (milestone 2,
//! Design Decision 1).
//!
//! libshared's `Datetime` stores a `time_t` and converts it with the C
//! library on every access (`localtime`, `gmtime`, `mktime`, `timegm`,
//! `strftime`, `time`). The Rust port keeps that model: epochs are plain
//! `i64` (`time_t`) values and every broken-down conversion goes through the
//! wrappers below, so `TZ`, the system zoneinfo database and libfaketime
//! (which intercepts these libc symbols) behave exactly as they do for the
//! C++ binary. chrono and `std` time-zone logic are deliberately not used.
//!
//! # `TZ` changes
//! glibc's `localtime ()` re-reads `TZ` on every call, but `localtime_r ()`
//! only initializes the zone once. [`localtime`] therefore calls `tzset ()`
//! before `localtime_r ()`, reproducing the non-reentrant C++ call's
//! behaviour while staying thread safe. `mktime ()` already calls `tzset ()`
//! internally.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.

// `time_t` and `long` are 32 bits on some targets, so the `as` casts between
// them and `i64` are not redundant everywhere.
#![allow(clippy::unnecessary_cast)]

use std::ffi::CString;

/// A broken-down time: the portable fields of C `struct tm`.
///
/// Field meanings are those of C (`tm_mon` 0-11, `tm_year` years since 1900,
/// `tm_wday` 0 = Sunday, `tm_yday` 0-365, `tm_isdst` -1 = "let mktime
/// decide"). `tm_gmtoff` is the offset from UTC in seconds as reported by the
/// C library (an output only; `mktime`/`timegm` ignore it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tm {
    pub tm_sec: i32,
    pub tm_min: i32,
    pub tm_hour: i32,
    pub tm_mday: i32,
    pub tm_mon: i32,
    pub tm_year: i32,
    pub tm_wday: i32,
    pub tm_yday: i32,
    pub tm_isdst: i32,
    pub tm_gmtoff: i64,
}

impl Tm {
    /// Converts to a C `struct tm` (with a null `tm_zone`).
    fn to_libc(self) -> libc::tm {
        // SAFETY: an all-zero `struct tm` (null `tm_zone`) is a valid value.
        let mut t: libc::tm = unsafe { std::mem::zeroed() };
        t.tm_sec = self.tm_sec;
        t.tm_min = self.tm_min;
        t.tm_hour = self.tm_hour;
        t.tm_mday = self.tm_mday;
        t.tm_mon = self.tm_mon;
        t.tm_year = self.tm_year;
        t.tm_wday = self.tm_wday;
        t.tm_yday = self.tm_yday;
        t.tm_isdst = self.tm_isdst;
        t.tm_gmtoff = self.tm_gmtoff as libc::c_long;
        t
    }

    /// Copies the portable fields out of a C `struct tm`.
    fn from_libc(t: &libc::tm) -> Tm {
        Tm {
            tm_sec: t.tm_sec,
            tm_min: t.tm_min,
            tm_hour: t.tm_hour,
            tm_mday: t.tm_mday,
            tm_mon: t.tm_mon,
            tm_year: t.tm_year,
            tm_wday: t.tm_wday,
            tm_yday: t.tm_yday,
            tm_isdst: t.tm_isdst,
            tm_gmtoff: t.tm_gmtoff as i64,
        }
    }
}

/// C `time (nullptr)`: the current epoch (intercepted by libfaketime).
pub fn time_now() -> i64 {
    // SAFETY: time(NULL) only returns the current time.
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

/// C `tzset ()`: re-reads `TZ`. Call after changing `TZ` in the environment.
pub fn tzset() {
    extern "C" {
        fn tzset();
    }
    // SAFETY: tzset has no arguments; it reads the environment.
    unsafe { tzset() }
}

/// C `localtime (&t)`: `t` broken down in the local time zone.
///
/// Calls `tzset ()` first so a changed `TZ` is honoured, as glibc's
/// `localtime` does.
///
/// # Panics
/// If the C library cannot represent `t` (the C++ code would dereference the
/// returned null pointer).
pub fn localtime(t: i64) -> Tm {
    tzset();
    let time = t as libc::time_t;
    // SAFETY: an all-zero tm is a valid output buffer; localtime_r only writes it.
    let mut out: libc::tm = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::localtime_r(&time, &mut out) };
    assert!(!result.is_null(), "localtime_r failed for epoch {t}");
    Tm::from_libc(&out)
}

/// C `gmtime (&t)`: `t` broken down in UTC.
///
/// # Panics
/// If the C library cannot represent `t` (the C++ code would dereference the
/// returned null pointer).
pub fn gmtime(t: i64) -> Tm {
    let time = t as libc::time_t;
    // SAFETY: an all-zero tm is a valid output buffer; gmtime_r only writes it.
    let mut out: libc::tm = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::gmtime_r(&time, &mut out) };
    assert!(!result.is_null(), "gmtime_r failed for epoch {t}");
    Tm::from_libc(&out)
}

/// C `mktime (&t)`: interprets `t` as local time, normalizes it in place
/// (including `tm_wday`, `tm_yday`, `tm_isdst`) and returns the epoch, or -1.
pub fn mktime(t: &mut Tm) -> i64 {
    let mut c = t.to_libc();
    // SAFETY: `c` is a valid, exclusively borrowed struct tm.
    let result = unsafe { libc::mktime(&mut c) } as i64;
    *t = Tm::from_libc(&c);
    result
}

/// C `timegm (&t)`: interprets `t` as UTC, normalizes it in place and returns
/// the epoch, or -1.
pub fn timegm(t: &mut Tm) -> i64 {
    let mut c = t.to_libc();
    // SAFETY: `c` is a valid, exclusively borrowed struct tm.
    let result = unsafe { libc::timegm(&mut c) } as i64;
    *t = Tm::from_libc(&c);
    result
}

/// C `strftime (buf, sizeof (buf), format, &t)` with a 256-byte buffer; the
/// result is empty if the output does not fit (as C leaves the buffer
/// unspecified and returns 0). `tm_zone` is null, so `%Z` is not meaningful.
pub fn strftime(format: &str, t: &Tm) -> String {
    let Ok(format) = CString::new(format) else {
        return String::new();
    };
    let c = t.to_libc();
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer and its length match; format is NUL-terminated.
    let written = unsafe {
        libc::strftime(buffer.as_mut_ptr() as *mut libc::c_char, buffer.len(), format.as_ptr(), &c)
    };
    String::from_utf8_lossy(&buffer[..written]).into_owned()
}

#[cfg(test)]
mod tests {
    //! Tests that set `TZ` hold [`crate::test_util::env_lock`] and restore it
    //! with [`crate::test_util::EnvGuard`] before releasing the lock.
    use super::*;
    use crate::test_util::{env_lock, EnvGuard};

    fn with_tz<F: FnOnce()>(tz: &str, f: F) {
        let _lock = env_lock();
        let env = EnvGuard::save(&["TZ"]);
        std::env::set_var("TZ", tz);
        tzset();
        f();
        drop(env);
        tzset();
    }

    #[test]
    fn gmtime_breaks_down_utc() {
        let t = gmtime(1_000_000_000);
        assert_eq!((t.tm_year, t.tm_mon, t.tm_mday), (101, 8, 9));
        assert_eq!((t.tm_hour, t.tm_min, t.tm_sec), (1, 46, 40));
        assert_eq!(t.tm_wday, 0);
        assert_eq!(t.tm_yday, 251);
    }

    #[test]
    fn timegm_inverts_gmtime_and_normalizes() {
        let mut t = gmtime(2_147_483_650);
        assert_eq!(timegm(&mut t), 2_147_483_650);

        // 2009-02-30 normalizes to 2009-03-02.
        let mut t = Tm { tm_year: 109, tm_mon: 1, tm_mday: 30, ..Tm::default() };
        let epoch = timegm(&mut t);
        assert_eq!((t.tm_mon, t.tm_mday), (2, 2));
        assert_eq!(epoch, 1_235_952_000);
    }

    #[test]
    fn localtime_and_mktime_follow_tz() {
        with_tz("UTC", || {
            let t = localtime(1_000_000_000);
            assert_eq!((t.tm_hour, t.tm_min), (1, 46));
            assert_eq!(t.tm_gmtoff, 0);
        });
        with_tz("America/New_York", || {
            let t = localtime(1_000_000_000);
            assert_eq!((t.tm_mday, t.tm_hour, t.tm_min), (8, 21, 46));
            assert_eq!(t.tm_isdst, 1);
            assert_eq!(t.tm_gmtoff, -4 * 3600);

            let mut t = Tm { tm_year: 110, tm_mon: 0, tm_mday: 1, tm_isdst: -1, ..Tm::default() };
            assert_eq!(mktime(&mut t), 1_262_322_000);
            assert_eq!(t.tm_isdst, 0);
            assert_eq!(t.tm_wday, 5);
        });
    }

    #[test]
    fn strftime_week_numbers() {
        // 2015-10-28 (Wednesday): %U 43, %V 44.
        let t = gmtime(1_446_036_900);
        assert_eq!(strftime("%U", &t), "43");
        assert_eq!(strftime("%V", &t), "44");
        assert_eq!(strftime("%Y-%m-%d", &t), "2015-10-28");
    }

    #[test]
    fn time_now_is_recent() {
        assert!(time_now() > 1_600_000_000);
    }
}
