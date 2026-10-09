//! Rust translation of `libshared src/Timer.cpp` (everything declared in
//! `Timer.h`): a stopwatch over a monotonic high-resolution clock.
//!
//! A [`Timer`] starts running when created, as in C++. `total_*` report the
//! time from the last [`start`](Timer::start) to the [`stop`](Timer::stop),
//! or to now while still running. Like the C++
//! `std::chrono::duration_cast<...>(...).count ()` converted to `double`,
//! each total is a whole number of its unit (truncated) returned as `f64`.
//!
//! The pinned libshared `Timer` has exactly `start`, `stop`, `total_s`,
//! `total_ms`, `total_us` and `total_ns`; it has no `subtract` or `now`.
//! `std::chrono::high_resolution_clock` becomes [`std::time::Instant`]
//! (monotonic; libstdc++'s `high_resolution_clock` is `system_clock`, so in
//! C++ a wall-clock step could make a total jump, which cannot happen here).
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.

use std::time::{Duration, Instant};

/// C++ `Timer`.
#[derive(Debug, Clone, Copy)]
pub struct Timer {
    start: Instant,
    /// C++ `_end`; `None` is the C++ default-constructed (epoch) time point,
    /// which is only ever present while the timer is running.
    end: Option<Instant>,
    running: bool,
}

impl Timer {
    /// C++ `Timer::Timer`: a timer that is already running.
    pub fn new() -> Self {
        Timer {
            start: Instant::now(),
            end: None,
            running: true,
        }
    }

    /// C++ `Timer::start`: (re)starts timing from now.
    pub fn start(&mut self) {
        self.start = Instant::now();
        self.end = None;
        self.running = true;
    }

    /// C++ `Timer::stop`: records the end time if running; a second stop
    /// keeps the first end time.
    pub fn stop(&mut self) {
        if self.running {
            self.end = Some(Instant::now());
            self.running = false;
        }
    }

    /// The elapsed time up to the end point (now, while running).
    fn elapsed(&self) -> Duration {
        let endpoint = if self.running {
            Instant::now()
        } else {
            self.end.unwrap_or(self.start)
        };

        endpoint.saturating_duration_since(self.start)
    }

    /// C++ `Timer::total_s`: whole seconds.
    pub fn total_s(&self) -> f64 {
        self.elapsed().as_secs() as f64
    }

    /// C++ `Timer::total_ms`: whole milliseconds.
    pub fn total_ms(&self) -> f64 {
        self.elapsed().as_millis() as f64
    }

    /// C++ `Timer::total_us`: whole microseconds.
    pub fn total_us(&self) -> f64 {
        self.elapsed().as_micros() as f64
    }

    /// C++ `Timer::total_ns`: whole nanoseconds.
    pub fn total_ns(&self) -> f64 {
        self.elapsed().as_nanos() as f64
    }
}

impl Default for Timer {
    /// Same as [`Timer::new`]: running from now.
    fn default() -> Self {
        Timer::new()
    }
}

#[cfg(test)]
mod tests {
    //! Port of libshared `test/timer.t.cpp` (7 assertions; its `t.diag`
    //! output lines are printed), then Rust-only tests of stop/start
    //! semantics.

    use super::*;
    use crate::shared::format::format;

    #[test]
    fn timer_t_cpp() {
        // Start this timer early to allow for non-trivial elapsed time.
        let mut t0 = Timer::new();
        t0.start();

        let mut t1 = Timer::new();
        t1.stop();
        assert!(t1.total_s() >= 0.0, "Timer: possibliy non-zero s if not started");
        assert!(t1.total_ms() >= 0.0, "Timer: possibliy non-zero ms if not started");
        assert!(t1.total_us() >= 0.0, "Timer: possibliy non-zero us if not started");
        assert!(t1.total_ns() >= 0.0, "Timer: possibliy non-zero ns if not started");

        t0.stop();
        assert!(t0.total_ms() >= t0.total_s(), "Timer: more ms than s");
        assert!(t0.total_us() >= t0.total_ms(), "Timer: more us than ms");
        assert!(t0.total_ns() >= t0.total_us(), "Timer: more ns than us");

        println!("{}", format("Total {1} s", &[&t0.total_s()]));
        println!("{}", format("Total {1} ms", &[&t0.total_ms()]));
        println!("{}", format("Total {1} μs", &[&t0.total_us()]));
        println!("{}", format("Total {1} ns", &[&t0.total_ns()]));

        let mut t2 = Timer::new();
        t2.start();
        println!("{}", format("Running total {1} s", &[&t2.total_s()]));
        println!("{}", format("Running total {1} ms", &[&t2.total_ms()]));
        println!("{}", format("Running total {1} us", &[&t2.total_us()]));
        println!("{}", format("Running total {1} ns", &[&t2.total_ns()]));
    }

    #[test]
    fn stopped_timer_is_frozen() {
        let mut t = Timer::new();
        std::thread::sleep(Duration::from_millis(2));
        t.stop();
        let frozen = t.total_ns();
        assert!(t.total_ms() >= 2.0, "at least the slept time");
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(t.total_ns(), frozen, "no change after stop");
        t.stop();
        assert_eq!(t.total_ns(), frozen, "a second stop keeps the first end");
    }

    #[test]
    fn running_timer_advances_and_restarts() {
        let mut t = Timer::default();
        std::thread::sleep(Duration::from_millis(2));
        let first = t.total_ns();
        assert!(t.total_ns() >= first, "still running");
        assert!(first >= 2_000_000.0, "at least the slept time");
        t.start();
        assert!(t.total_ns() < first, "start resets the start point");
    }

    #[test]
    fn totals_are_truncated_whole_units() {
        let mut t = Timer::new();
        t.stop();
        for total in [t.total_s(), t.total_ms(), t.total_us(), t.total_ns()] {
            assert_eq!(total, total.trunc());
        }
        assert_eq!(t.total_s(), 0.0, "well under a second");
    }
}
