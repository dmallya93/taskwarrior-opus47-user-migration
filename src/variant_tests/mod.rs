//! Unit tests for [`super::Variant`]: direct tests of the core API, plus the
//! ports of Taskwarrior's `test/variant_*_test.cpp` suites (one sub-module
//! per upstream file, assertion-count checked through [`UnitTest`]).

use super::*;
use crate::test_util::{env_lock, EnvGuard};
use std::fmt::Debug;
use std::sync::MutexGuard;

mod add;
mod and;
mod cast;
mod divide;
mod equal;
mod exp;
mod gt;
mod gte;
mod inequal;
mod lt;
mod lte;
mod r#match;
mod math;
mod modulo;
mod multiply;
mod nomatch;
mod not;
mod or;
mod partial;
mod subtract;
mod xor;

/// The C++ `UnitTest` (test/test.cpp), recording instead of printing.
pub(super) struct UnitTest {
    counter: usize,
    failures: Vec<String>,
}

impl UnitTest {
    pub(super) fn new() -> Self {
        UnitTest { counter: 0, failures: Vec::new() }
    }

    /// C++ `UnitTest::ok`.
    pub(super) fn ok(&mut self, expression: bool, name: &str) {
        self.counter += 1;
        if !expression {
            self.failures.push(format!("not ok {} - {}", self.counter, name));
        }
    }

    /// C++ `UnitTest::notok`.
    #[allow(dead_code)] // Used by the operator suites.
    pub(super) fn notok(&mut self, expression: bool, name: &str) {
        self.ok(!expression, name);
    }

    /// C++ `UnitTest::pass`.
    pub(super) fn pass(&mut self, name: &str) {
        self.ok(true, name);
    }

    /// C++ `UnitTest::fail`.
    pub(super) fn fail(&mut self, name: &str) {
        self.ok(false, name);
    }

    /// C++ `UnitTest::is` (exact comparison overloads).
    pub(super) fn is<A: PartialEq<E> + Debug, E: Debug>(&mut self, actual: A, expected: E, name: &str) {
        self.counter += 1;
        if actual != expected {
            self.failures.push(format!(
                "not ok {} - {}\n#   expected: {:?}\n#        got: {:?}",
                self.counter, name, expected, actual
            ));
        }
    }

    /// C++ `UnitTest::is (double actual, double expected, double tolerance, ...)`.
    pub(super) fn is_tol(&mut self, actual: f64, expected: f64, tolerance: f64, name: &str) {
        self.counter += 1;
        if (actual - expected).abs() > tolerance {
            self.failures.push(format!(
                "not ok {} - {}\n#   expected: {}\n#        got: {}",
                self.counter,
                name,
                stream_double(expected),
                stream_double(actual)
            ));
        }
    }

    /// C++ `UnitTest::diag`.
    pub(super) fn diag(&self, text: &str) {
        println!("# {text}");
    }

    /// Asserts that every assertion passed and that `planned` ran.
    pub(super) fn check(&self, planned: usize) {
        assert!(
            self.failures.is_empty(),
            "{} of {} assertions failed:\n{}",
            self.failures.len(),
            self.counter,
            self.failures.join("\n")
        );
        assert_eq!(self.counter, planned, "number of assertions executed");
    }
}

/// Holds [`env_lock`] with `TZ` saved (and optionally overridden), and
/// restores `TZ` and the `Variant` statics on drop.
pub(super) struct Upstream {
    date_format: String,
    case_sensitive: bool,
    regex: bool,
    iso: bool,
    env: Option<EnvGuard>,
    _lock: MutexGuard<'static, ()>,
}

impl Upstream {
    pub(super) fn setup(tz: Option<&str>) -> Self {
        let lock = env_lock();
        let env = EnvGuard::save(&["TZ"]);
        if let Some(tz) = tz {
            std::env::set_var("TZ", tz);
        }
        localtime::tzset();
        Upstream {
            date_format: Variant::date_format(),
            case_sensitive: Variant::search_case_sensitive(),
            regex: Variant::search_using_regex(),
            iso: Variant::iso_enabled(),
            env: Some(env),
            _lock: lock,
        }
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        Variant::set_date_format(&self.date_format);
        Variant::set_search_case_sensitive(self.case_sensitive);
        Variant::set_search_using_regex(self.regex);
        Variant::set_iso_enabled(self.iso);
        drop(self.env.take());
        localtime::tzset();
    }
}

#[test]
fn defaults_and_statics() {
    let _u = Upstream::setup(None);
    let v = Variant::default();
    assert_eq!(v.type_(), Type::Boolean);
    assert!(!v.get_bool());
    assert_eq!(v.get_integer(), 0);
    assert_eq!(v.get_real(), 0.0);
    assert_eq!(v.get_string(), "");
    assert_eq!(v.get_date(), 0);
    assert_eq!(v.get_duration(), 0);
    assert_eq!(v.source(), "");

    assert_eq!(Variant::date_format(), "");
    assert!(Variant::search_case_sensitive());
    assert!(Variant::search_using_regex());
    assert!(!Variant::iso_enabled());

    Variant::set_date_format("D.M.Y");
    Variant::set_search_case_sensitive(false);
    Variant::set_search_using_regex(false);
    Variant::set_iso_enabled(true);
    assert_eq!(Variant::date_format(), "D.M.Y");
    assert!(!Variant::search_case_sensitive());
    assert!(!Variant::search_using_regex());
    assert!(Variant::iso_enabled());
}

#[test]
fn constructors() {
    let v = Variant::from_bool(true);
    assert_eq!(v.type_(), Type::Boolean);
    assert!(v.get_bool());

    let v = Variant::from_int(-42);
    assert_eq!(v.type_(), Type::Integer);
    assert_eq!(v.get_integer(), -42);

    let v = Variant::from_real(3.5);
    assert_eq!(v.type_(), Type::Real);
    assert_eq!(v.get_real(), 3.5);

    let v = Variant::from_string("foo");
    assert_eq!(v.type_(), Type::String);
    assert_eq!(v.get_string(), "foo");

    let v = Variant::from_time(1234567890, Type::Date).unwrap();
    assert_eq!(v.type_(), Type::Date);
    assert_eq!(v.get_date(), 1234567890);
    assert_eq!(v.get_duration(), 0);

    let v = Variant::from_time(3600, Type::Duration).unwrap();
    assert_eq!(v.type_(), Type::Duration);
    assert_eq!(v.get_duration(), 3600);
    assert_eq!(v.get_date(), 0);

    for t in [Type::Boolean, Type::Integer, Type::Real, Type::String] {
        let e = Variant::from_time(1, t).unwrap_err();
        assert_eq!(e.to_string(), "Cannot instantiate this type with a time_t value.");
    }

    assert_eq!(Variant::from(true).type_(), Type::Boolean);
    assert_eq!(Variant::from(7).get_integer(), 7);
    assert_eq!(Variant::from(1.5).get_real(), 1.5);
    assert_eq!(Variant::from("a").get_string(), "a");
    assert_eq!(Variant::from(String::from("b")).get_string(), "b");

    let mut v = Variant::from_int(1);
    v.set_source("description");
    let copy = v.clone();
    assert_eq!(copy.source(), "description");
    assert_eq!(copy.get_integer(), 1);
}

#[test]
#[allow(clippy::approx_constant)] // The literal 3.14 is the formatting case under test.
fn display() {
    let _u = Upstream::setup(Some("UTC"));
    assert_eq!(Variant::from_bool(true).to_string(), "true");
    assert_eq!(Variant::from_bool(false).to_string(), "false");
    assert_eq!(Variant::from_int(-17).to_string(), "-17");
    assert_eq!(Variant::from_real(3.14).to_string(), "3.14");
    assert_eq!(Variant::from_real(1234567.0).to_string(), "1.23457e+06");
    assert_eq!(Variant::from_real(0.5).to_string(), "0.5");
    assert_eq!(Variant::from_real(2.0).to_string(), "2");
    assert_eq!(Variant::from_string("\"quoted\"").to_string(), "\"quoted\"");
    assert_eq!(Variant::from_time(1234567890, Type::Date).unwrap().to_string(), "2009-02-13T23:31:30");
    assert_eq!(Variant::from_time(12345, Type::Duration).unwrap().to_string(), "PT3H25M45S");
    assert_eq!(Variant::from_time(0, Type::Duration).unwrap().to_string(), "PT0S");
}

#[test]
fn trivial() {
    assert!(!Variant::from_bool(false).trivial());
    assert!(Variant::from_int(0).trivial());
    assert!(!Variant::from_int(1).trivial());
    assert!(Variant::from_real(0.0).trivial());
    assert!(!Variant::from_real(0.1).trivial());
    assert!(Variant::from_string("").trivial());
    assert!(!Variant::from_string("x").trivial());
    assert!(Variant::from_time(0, Type::Date).unwrap().trivial());
    assert!(Variant::from_time(0, Type::Duration).unwrap().trivial());
    assert!(!Variant::from_time(1, Type::Duration).unwrap().trivial());
}

#[test]
fn sqrt() {
    let mut v = Variant::from_int(16);
    v.sqrt().unwrap();
    assert_eq!(v.type_(), Type::Real);
    assert_eq!(v.get_real(), 4.0);

    let mut v = Variant::from_string("'2.25'");
    v.sqrt().unwrap();
    assert_eq!(v.get_real(), 1.5);

    let mut v = Variant::from_real(-1.0);
    assert_eq!(v.sqrt().unwrap_err().to_string(), "Cannot take the square root of a negative number.");
}

#[test]
fn cast_string_details() {
    let _u = Upstream::setup(Some("UTC"));

    // Boolean: empty, "0" and "0.0" are false; quotes are removed first.
    for (text, expected) in [("", false), ("0", false), ("0.0", false), ("'0'", false), ("00", true), ("x", true)] {
        let mut v = Variant::from_string(text);
        v.cast(Type::Boolean).unwrap();
        assert_eq!(v.get_bool(), expected, "{text:?}");
    }

    // Integer: strtol, base 16 only for a literal "0x" prefix.
    for (text, expected) in [(" 12abc", 12), ("-7", -7), ("0x1F", 31), ("0X1F", 0), ("abc", 0), ("99999999999999999999", i64::MAX)] {
        let mut v = Variant::from_string(text);
        v.cast(Type::Integer).unwrap();
        assert_eq!(v.get_integer(), expected, "{text:?}");
    }

    // Real: strtod.
    for (text, expected) in [("1e3", 1000.0), ("  -2.5x", -2.5), ("0x10", 16.0), ("foo", 0.0)] {
        let mut v = Variant::from_string(text);
        v.cast(Type::Real).unwrap();
        assert_eq!(v.get_real(), expected, "{text:?}");
    }

    // Duration: whole-string parse only.
    let mut v = Variant::from_string("1h");
    v.cast(Type::Duration).unwrap();
    assert_eq!(v.get_duration(), 3600);
    let mut v = Variant::from_string("1h trailing");
    v.cast(Type::Duration).unwrap();
    assert_eq!(v.get_duration(), 0);

    // Date: dateFormat, then a duration relative to now, else 0.
    Variant::set_date_format("");
    let mut v = Variant::from_string("2009-02-13T23:31:30Z");
    v.cast(Type::Date).unwrap();
    assert_eq!(v.get_date(), 1234567890);

    let before = localtime::time_now();
    let mut v = Variant::from_string("P1D");
    v.cast(Type::Date).unwrap();
    assert!(v.get_date() >= before + 86400 && v.get_date() <= localtime::time_now() + 86400);

    let mut v = Variant::from_string("not a date");
    v.cast(Type::Date).unwrap();
    assert_eq!(v.type_(), Type::Date);
    assert_eq!(v.get_date(), 0);

    Variant::set_date_format("D.M.Y");
    let mut v = Variant::from_string("13.02.2009");
    v.cast(Type::Date).unwrap();
    assert_eq!(v.get_date(), 1234483200);

    let mut v = Variant::from_string("'not a date'");
    let e = v.cast(Type::Date).unwrap_err();
    assert_eq!(e.to_string(), "'not a date' is not a valid date in the 'D.M.Y' format.");
    assert_eq!(v.type_(), Type::String);
    assert_eq!(v.get_string(), "not a date");
}

#[test]
fn cast_real_details() {
    let mut v = Variant::from_real(1234567.0);
    v.cast(Type::String).unwrap();
    assert_eq!(v.get_string(), "1.23457e+06");

    let mut v = Variant::from_real(-3.99);
    v.cast(Type::Integer).unwrap();
    assert_eq!(v.get_integer(), -3);

    let mut v = Variant::from_real(f64::NAN);
    v.cast(Type::Integer).unwrap();
    assert_eq!(v.get_integer(), i64::MIN);

    let mut v = Variant::from_real(1e10);
    v.cast(Type::Date).unwrap();
    assert_eq!(v.get_date(), i64::from(i32::MIN));

    let mut v = Variant::from_real(-2.5);
    v.cast(Type::Duration).unwrap();
    assert_eq!(v.get_duration(), -2);
}

#[test]
fn placeholder_task_has_no_annotations_or_tags() {
    let task = Task::default();
    assert!(task.get_annotations().is_empty());
    assert!(!task.has_tag("anything"));
}

#[test]
fn operators_custom_order() {
    let _u = Upstream::setup(Some("UTC"));
    let saved = Task::custom_order();

    let mut order = BTreeMap::new();
    order.insert("priority".to_string(), vec!["H".to_string(), "M".to_string(), "L".to_string()]);
    Task::set_custom_order(order);

    let mut h = Variant::from_string("H");
    h.set_source("priority");
    let mut l = Variant::from_string("L");
    l.set_source("priority");
    let mut unknown = Variant::from_string("X");
    unknown.set_source("priority");

    // Positions in the custom order: H (0) < M (1) < L (2) < not found (3).
    assert!(h.op_lt(&l).unwrap());
    assert!(!l.op_lt(&h).unwrap());
    assert!(h.op_lte(&l).unwrap());
    assert!(l.op_gt(&h).unwrap());
    assert!(l.op_gte(&h).unwrap());
    assert!(l.op_lt(&unknown).unwrap());
    assert!(!h.op_lt(&h).unwrap());
    assert!(h.op_lte(&h).unwrap());
    assert!(h.op_gte(&h).unwrap());
    assert!(!h.op_gt(&h).unwrap());
    // An empty string is found nowhere (position 3): no trivial shortcut.
    assert!(h.op_lt(&Variant::from_string("")).unwrap());

    // The lookup uses the left source only; without it, plain byte order.
    assert!(Variant::from_string("H").op_lt(&l).unwrap());
    assert!(!Variant::from_string("L").op_lt(&h).unwrap());

    Task::set_custom_order(saved);
}

#[test]
fn operators_string_rules() {
    let _u = Upstream::setup(Some("UTC"));

    // Trivial strings never order, but equal strings short-circuit first.
    assert!(!Variant::from_string("").op_lt(&Variant::from_string("a")).unwrap());
    assert!(!Variant::from_string("a").op_gt(&Variant::from_string("")).unwrap());
    assert!(!Variant::from_string("").op_lte(&Variant::from_string("a")).unwrap());
    assert!(Variant::from_string("").op_lte(&Variant::from_string("")).unwrap());
    assert!(Variant::from_string("").op_gte(&Variant::from_string("")).unwrap());

    // std::string comparison is byte-wise; quotes are removed first.
    assert!(Variant::from_string("B").op_lt(&Variant::from_string("a")).unwrap());
    assert!(Variant::from_string("z").op_lt(&Variant::from_string("\u{e9}")).unwrap());
    assert!(Variant::from_string("'abc'").op_eq(&Variant::from_string("abc")).unwrap());

    // Status is always compared caseless (left source only).
    let mut status = Variant::from_string("Pending");
    status.set_source("status");
    assert!(status.op_eq(&Variant::from_string("pending")).unwrap());
    assert!(!status.op_ne(&Variant::from_string("PENDING")).unwrap());
    assert!(!Variant::from_string("Pending").op_eq(&Variant::from_string("pending")).unwrap());

    // String == duration and duration == * have no trivial shortcut.
    assert!(Variant::from_string("").op_eq(&Variant::from_time(0, Type::Duration).unwrap()).unwrap());
    // Date == trivial is false, even for equal values.
    let zero = Variant::from_time(0, Type::Date).unwrap();
    assert!(!zero.op_eq(&zero).unwrap());
    assert!(zero.op_ne(&zero).unwrap());

    // Logical operators dequote and cast strings to Boolean.
    assert!(!Variant::from_string("'0'").op_and(&Variant::from_bool(true)));
    assert!(Variant::from_string("'0'").op_or(&Variant::from_string("x")));
    assert!(Variant::from_string("\"0.0\"").op_not());
    assert!(Variant::from_string("").op_xor(&Variant::from_int(1)));
}

#[test]
fn operators_boolean_gt_matches_cpp() {
    // C++ operator> on two Booleans returns `!left && right`.
    let t = Variant::from_bool(true);
    let f = Variant::from_bool(false);
    assert!(!t.op_gt(&f).unwrap());
    assert!(f.op_gt(&t).unwrap());
    assert!(t.op_gte(&f).unwrap());
    assert!(!f.op_gte(&t).unwrap());
    assert!(f.op_lt(&t).unwrap());
    assert!(f.op_lte(&t).unwrap());
    assert!(!t.op_lte(&f).unwrap());
}

#[test]
fn operators_propagate_cast_errors() {
    let _u = Upstream::setup(Some("UTC"));
    Variant::set_date_format("D.M.Y");
    let date = Variant::from_time(1234567890, Type::Date).unwrap();
    let text = Variant::from_string("bogus");
    let message = "'bogus' is not a valid date in the 'D.M.Y' format.";

    assert_eq!(text.op_lt(&date).unwrap_err().to_string(), message);
    assert_eq!(text.op_lte(&date).unwrap_err().to_string(), message);
    assert_eq!(date.op_gt(&text).unwrap_err().to_string(), message);
    assert_eq!(date.op_gte(&text).unwrap_err().to_string(), message);
    assert_eq!(text.op_eq(&date).unwrap_err().to_string(), message);
    assert_eq!(date.op_ne(&text).unwrap_err().to_string(), message);

    let same = Variant::from_string("13.02.2009");
    assert!(same.op_lt(&date).unwrap());
    assert!(date.op_gt(&same).unwrap());
}

#[test]
fn arithmetic_details() {
    let _u = Upstream::setup(Some("UTC"));
    let err = |r: Result<Variant>| r.unwrap_err().to_string();
    let b = Variant::from_bool(true);
    let i = Variant::from_int(2);
    let r = Variant::from_real(1.5);
    let s = Variant::from_string("x");
    let date = Variant::from_time(100, Type::Date).unwrap();
    let dur = Variant::from_time(10, Type::Duration).unwrap();

    // Exact error texts.
    assert_eq!(err(b.op_exp(&i)), "Cannot exponentiate Booleans");
    assert_eq!(err(i.op_exp(&r)), "Cannot exponentiate to a non-integer power");
    assert_eq!(err(r.op_exp(&s)), "Cannot exponentiate strings");
    assert_eq!(err(i.op_exp(&date)), "Cannot exponentiate dates");
    assert_eq!(err(dur.op_exp(&i)), "Cannot exponentiate durations");
    assert_eq!(err(b.op_sub(&i)), "Cannot subtract from a Boolean value");
    assert_eq!(err(i.op_sub(&s)), "Cannot subtract strings");
    assert_eq!(err(dur.op_sub(&date)), "Cannot subtract a date");
    assert_eq!(err(b.op_add(&b)), "Cannot add two Boolean values");
    assert_eq!(err(date.op_add(&date)), "Cannot add two date values");

    // Integer power goes through (int) pow (...): truncation, negative
    // exponents, and INT_MIN for an out-of-range result.
    assert_eq!(Variant::from_int(2).op_exp(&Variant::from_int(-1)).unwrap().get_integer(), 0);
    assert_eq!(Variant::from_int(-3).op_exp(&Variant::from_int(3)).unwrap().get_integer(), -27);
    assert_eq!(Variant::from_int(10).op_exp(&Variant::from_int(10)).unwrap().get_integer(), i64::from(i32::MIN));

    // Signed overflow wraps.
    let mut max = Variant::from_string("9223372036854775807");
    max.cast(Type::Integer).unwrap();
    assert_eq!(max.op_add(&Variant::from_int(1)).unwrap().get_integer(), i64::MIN);

    // Real + date: (unsigned)(int) _real widened to time_t.
    let v = Variant::from_real(-1.5).op_add(&date).unwrap();
    assert_eq!(v.type_(), Type::Date);
    assert_eq!(v.get_date(), 4_294_967_295 + 100);
    let v = Variant::from_real(-1.5).op_add(&dur).unwrap();
    assert_eq!(v.type_(), Type::Duration);
    assert_eq!(v.get_duration(), 4_294_967_295 + 10);

    // Duration + date and date - date.
    let v = dur.op_add(&date).unwrap();
    assert_eq!((v.type_(), v.get_date()), (Type::Date, 110));
    let v = date.op_sub(&Variant::from_time(40, Type::Date).unwrap()).unwrap();
    assert_eq!((v.type_(), v.get_duration()), (Type::Duration, 60));

    // += dequotes the right operand only; -= dequotes neither.
    assert_eq!(Variant::from_string("'a'").op_add(&Variant::from_string("'b'")).unwrap().get_string(), "'a'b");
    assert_eq!(Variant::from_string("'a'").op_sub(&Variant::from_string("'b'")).unwrap().get_string(), "'a'-'b'");
    assert_eq!(Variant::from_int(1).op_add(&Variant::from_string("\"z\"")).unwrap().get_string(), "1z");

    // Compound forms modify in place; the source is kept.
    let mut v = Variant::from_int(5);
    v.set_source("due");
    v.op_sub_assign(&Variant::from_int(7)).unwrap();
    assert_eq!((v.get_integer(), v.source()), (-2, "due"));
    v.op_exp_assign(&Variant::from_int(2)).unwrap();
    assert_eq!(v.get_integer(), 4);
    v.op_add_assign(&Variant::from_real(0.5)).unwrap();
    assert_eq!((v.type_(), v.get_real()), (Type::Real, 4.5));
}

#[test]
fn multiplicative_details() {
    let _u = Upstream::setup(Some("UTC"));
    let err = |r: Result<Variant>| r.unwrap_err().to_string();
    let b = Variant::from_bool(true);
    let f = Variant::from_bool(false);
    let i = Variant::from_int(2);
    let zero = Variant::from_int(0);
    let r = Variant::from_real(1.5);
    let rzero = Variant::from_real(0.0);
    let s = Variant::from_string("x");
    let date = Variant::from_time(100, Type::Date).unwrap();
    let dur = Variant::from_time(10, Type::Duration).unwrap();
    let dzero = Variant::from_time(0, Type::Duration).unwrap();

    // Exact error texts of operator*=.
    assert_eq!(err(b.op_mul(&b)), "Cannot multiply Boolean values");
    assert_eq!(err(b.op_mul(&date)), "Cannot multiply date values");
    assert_eq!(err(date.op_mul(&i)), "Cannot multiply date values");
    assert_eq!(err(r.op_mul(&s)), "Cannot multiply real numbers by strings");
    assert_eq!(err(s.op_mul(&r)), "Cannot multiply strings by real numbers");
    assert_eq!(err(s.op_mul(&s)), "Cannot multiply strings by strings");
    assert_eq!(err(s.op_mul(&date)), "Cannot multiply strings by dates");
    assert_eq!(err(s.op_mul(&dur)), "Cannot multiply strings by durations");
    assert_eq!(err(dur.op_mul(&s)), "Cannot multiply durations by strings");
    assert_eq!(err(dur.op_mul(&date)), "Cannot multiply durations by dates");
    assert_eq!(err(dur.op_mul(&dur)), "Cannot multiply durations by durations");

    // Exact error texts of operator/=.
    assert_eq!(err(b.op_div(&i)), "Cannot divide Boolean values");
    assert_eq!(err(i.op_div(&b)), "Cannot divide integers by Boolean values");
    assert_eq!(err(i.op_div(&zero)), "Cannot divide by zero");
    assert_eq!(err(i.op_div(&rzero)), "Cannot divide by zero");
    assert_eq!(err(i.op_div(&dzero)), "Cannot divide by zero");
    assert_eq!(err(r.op_div(&zero)), "Cannot divide by zero");
    assert_eq!(err(r.op_div(&rzero)), "Cannot divide by zero");
    assert_eq!(err(r.op_div(&dzero)), "Cannot divide by zero");
    assert_eq!(err(dur.op_div(&zero)), "Cannot divide by zero");
    assert_eq!(err(dur.op_div(&Variant::from_real(-0.0))), "Cannot divide by zero");
    assert_eq!(err(i.op_div(&s)), "Cannot divide integer by string");
    assert_eq!(err(i.op_div(&date)), "Cannot divide integer by date values");
    assert_eq!(err(r.op_div(&b)), "Cannot divide real by Boolean");
    assert_eq!(err(r.op_div(&s)), "Cannot divide real numbers by strings");
    assert_eq!(err(r.op_div(&date)), "Cannot divide real numbers by dates");
    assert_eq!(err(s.op_div(&i)), "Cannot divide real numbers by strings");
    assert_eq!(err(date.op_div(&i)), "Cannot divide real numbers by dates");
    assert_eq!(err(dur.op_div(&b)), "Cannot divide duration by Boolean");
    assert_eq!(err(dur.op_div(&s)), "Cannot divide durations by strings");
    assert_eq!(err(dur.op_div(&date)), "Cannot divide durations by dates");

    // Exact error texts of operator%=.
    assert_eq!(err(b.op_mod(&i)), "Cannot modulo Booleans");
    assert_eq!(err(i.op_mod(&b)), "Cannot modulo integer by Boolean");
    assert_eq!(err(i.op_mod(&zero)), "Cannot modulo zero");
    assert_eq!(err(i.op_mod(&rzero)), "Cannot modulo zero");
    assert_eq!(err(r.op_mod(&zero)), "Cannot modulo zero");
    assert_eq!(err(r.op_mod(&rzero)), "Cannot modulo zero");
    assert_eq!(err(i.op_mod(&s)), "Cannot modulo integer by string");
    assert_eq!(err(i.op_mod(&date)), "Cannot modulo integer by date values");
    assert_eq!(err(i.op_mod(&dur)), "Cannot modulo integer by duration values");
    assert_eq!(err(r.op_mod(&b)), "Cannot modulo real by Boolean");
    assert_eq!(err(r.op_mod(&s)), "Cannot modulo real numbers by strings");
    assert_eq!(err(r.op_mod(&date)), "Cannot modulo real numbers by dates");
    assert_eq!(err(r.op_mod(&dur)), "Cannot modulo real by duration values");
    assert_eq!(err(s.op_mod(&i)), "Cannot modulo string values");
    assert_eq!(err(date.op_mod(&i)), "Cannot modulo date values");
    assert_eq!(err(dur.op_mod(&i)), "Cannot modulo duration values");

    // String repetition; *= dequotes the right operand only.
    assert_eq!(Variant::from_int(3).op_mul(&Variant::from_string("'ab'")).unwrap().get_string(), "ababab");
    assert_eq!(Variant::from_string("'ab'").op_mul(&Variant::from_int(2)).unwrap().get_string(), "'ab''ab'");
    let v = Variant::from_int(0).op_mul(&s).unwrap();
    assert_eq!((v.type_(), v.get_string()), (Type::String, ""));
    assert_eq!(Variant::from_int(-2).op_mul(&s).unwrap().get_string(), "");
    assert_eq!(s.op_mul(&Variant::from_int(1)).unwrap().get_string(), "x");
    assert_eq!(s.op_mul(&zero).unwrap().get_string(), "x");
    assert_eq!(f.op_mul(&s).unwrap().get_string(), "");
    assert_eq!(b.op_mul(&Variant::from_string("\"y\"")).unwrap().get_string(), "y");
    assert_eq!(s.op_mul(&f).unwrap().get_string(), "");
    assert_eq!(s.op_mul(&b).unwrap().get_string(), "x");
    // (int) _integer truncates: 2^32 + 2 repeats twice.
    let mut big = Variant::from_string("4294967298");
    big.cast(Type::Integer).unwrap();
    assert_eq!(big.op_mul(&s).unwrap().get_string(), "xx");

    // (time_t)(unsigned)(int) of real products / quotients.
    let v = Variant::from_real(-1.5).op_mul(&dur).unwrap();
    assert_eq!((v.type_(), v.get_duration()), (Type::Duration, 4_294_967_295 - 14));
    assert_eq!(dur.op_mul(&Variant::from_real(-0.5)).unwrap().get_duration(), 4_294_967_295 - 4);
    assert_eq!(dur.op_mul(&Variant::from_real(1e10)).unwrap().get_duration(), 2_147_483_648);
    assert_eq!(dur.op_div(&Variant::from_real(-2.0)).unwrap().get_duration(), 4_294_967_291);
    let v = Variant::from_real(-20.0).op_div(&dur).unwrap();
    assert_eq!((v.type_(), v.get_duration()), (Type::Duration, 4_294_967_294));
    let v = Variant::from_int(-25).op_div(&dur).unwrap();
    assert_eq!((v.type_(), v.get_duration()), (Type::Duration, 4_294_967_294));

    // Integer division truncates toward zero; duration / duration is real
    // with no zero check.
    assert_eq!(Variant::from_int(-7).op_div(&i).unwrap().get_integer(), -3);
    assert_eq!(dur.op_div(&Variant::from_int(3)).unwrap().get_duration(), 3);
    let v = dur.op_div(&dzero).unwrap();
    assert_eq!(v.type_(), Type::Real);
    assert!(v.get_real().is_infinite());
    assert!(dzero.op_div(&dzero).unwrap().get_real().is_nan());

    // Remainders: integer % keeps the dividend's sign, reals use fmod.
    assert_eq!(Variant::from_int(-7).op_mod(&i).unwrap().get_integer(), -1);
    assert_eq!(Variant::from_real(-7.5).op_mod(&i).unwrap().get_real(), -1.5);
    let v = Variant::from_int(7).op_mod(&r).unwrap();
    assert_eq!((v.type_(), v.get_real()), (Type::Real, 1.0));

    // Signed overflow wraps.
    let mut min = Variant::from_string("-9223372036854775808");
    min.cast(Type::Integer).unwrap();
    let minus_one = Variant::from_int(-1);
    assert_eq!(min.op_mul(&minus_one).unwrap().get_integer(), i64::MIN);
    assert_eq!(min.op_div(&minus_one).unwrap().get_integer(), i64::MIN);
    assert_eq!(min.op_mod(&minus_one).unwrap().get_integer(), 0);

    // Compound forms modify in place; the source is kept.
    let mut v = Variant::from_int(6);
    v.set_source("urgency");
    v.op_mul_assign(&Variant::from_int(7)).unwrap();
    assert_eq!((v.get_integer(), v.source()), (42, "urgency"));
    v.op_div_assign(&Variant::from_int(4)).unwrap();
    assert_eq!(v.get_integer(), 10);
    v.op_mod_assign(&Variant::from_int(4)).unwrap();
    assert_eq!(v.get_integer(), 2);
}
