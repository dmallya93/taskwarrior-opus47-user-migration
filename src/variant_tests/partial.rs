//! Port of Taskwarrior `test/variant_partial_test.cpp` (76 assertions
//! executed; its `UnitTest t (72)` plan is stale), with the C++ messages.
//!
//! The same-day checks depend on the local time zone, so the suite runs once
//! per zone of the test strategy.

use super::*;

/// Assertions executed by variant_partial_test.cpp: 36 operand pairs and the
/// two date/string same-day cases, two assertions each.
const PLANNED: usize = 76;

/// The zones of the test strategy (wiki testing/test-strategy.md).
const ZONES: [&str; 6] = [
    "UTC",
    "America/New_York",
    "Europe/Berlin",
    "Asia/Kolkata",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
];

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
fn suite(t: &mut UnitTest) -> Result<()> {
    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;
    let v6 = Variant::from_time(1234522800, Type::Date)?; // 2009-02-13, 12.00pm UTC
    let v7 = Variant::from_string("2009-02-13");

    let v00 = Variant::from_bool(v0.op_partial(&v0)?);
    t.is(v00.type_(), Type::Boolean, "true == true --> boolean");
    t.is(v00.get_bool(), true, "true == true --> true");

    let v01 = Variant::from_bool(v0.op_partial(&v1)?);
    t.is(v01.type_(), Type::Boolean, "true == 42 --> boolean");
    t.is(v01.get_bool(), false, "true == 42 --> false");

    let v02 = Variant::from_bool(v0.op_partial(&v2)?);
    t.is(v02.type_(), Type::Boolean, "true == 3.14 --> boolean");
    t.is(v02.get_bool(), false, "true == 3.14 --> false");

    let v03 = Variant::from_bool(v0.op_partial(&v3)?);
    t.is(v03.type_(), Type::Boolean, "true == 'foo' --> boolean");
    t.is(v03.get_bool(), false, "true == 'foo' --> false");

    let v04 = Variant::from_bool(v0.op_partial(&v4)?);
    t.is(v04.type_(), Type::Boolean, "true == 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "true == 1234567890 --> false");

    let v05 = Variant::from_bool(v0.op_partial(&v5)?);
    t.is(v05.type_(), Type::Boolean, "true == 1200 --> boolean");
    t.is(v05.get_bool(), false, "true == 1200 --> false");

    let v10 = Variant::from_bool(v1.op_partial(&v0)?);
    t.is(v10.type_(), Type::Boolean, "42 == true --> boolean");
    t.is(v10.get_bool(), false, "42 == true --> false");

    let v11 = Variant::from_bool(v1.op_partial(&v1)?);
    t.is(v11.type_(), Type::Boolean, "42 == 42 --> boolean");
    t.is(v11.get_bool(), true, "42 == 42 --> true");

    let v12 = Variant::from_bool(v1.op_partial(&v2)?);
    t.is(v12.type_(), Type::Boolean, "42 == 3.14 --> boolean");
    t.is(v12.get_bool(), false, "42 == 3.14 --> false");

    let v13 = Variant::from_bool(v1.op_partial(&v3)?);
    t.is(v13.type_(), Type::Boolean, "42 == 'foo' --> boolean");
    t.is(v13.get_bool(), false, "42 == 'foo' --> false");

    // As in the C++, the two checks below test v04 rather than v14.
    let _v14 = Variant::from_bool(v1.op_partial(&v4)?);
    t.is(v04.type_(), Type::Boolean, "42 == 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "42 == 1234567890 --> false");

    let v15 = Variant::from_bool(v1.op_partial(&v5)?);
    t.is(v15.type_(), Type::Boolean, "42 == 1200 --> boolean");
    t.is(v15.get_bool(), false, "42 == 1200 --> false");

    let v20 = Variant::from_bool(v2.op_partial(&v0)?);
    t.is(v20.type_(), Type::Boolean, "3.14 == true --> boolean");
    t.is(v20.get_bool(), false, "3.14 == true --> false");

    let v21 = Variant::from_bool(v2.op_partial(&v1)?);
    t.is(v21.type_(), Type::Boolean, "3.14 == 42 --> boolean");
    t.is(v21.get_bool(), false, "3.14 == 42 --> false");

    let v22 = Variant::from_bool(v2.op_partial(&v2)?);
    t.is(v22.type_(), Type::Boolean, "3.14 == 3.14 --> boolean");
    t.is(v22.get_bool(), true, "3.14 == 3.14 --> true");

    let v23 = Variant::from_bool(v2.op_partial(&v3)?);
    t.is(v23.type_(), Type::Boolean, "3.14 == 'foo' --> boolean");
    t.is(v23.get_bool(), false, "3.14 == 'foo' --> false");

    let v24 = Variant::from_bool(v2.op_partial(&v4)?);
    t.is(v24.type_(), Type::Boolean, "3.14 == 1234567890 --> boolean");
    t.is(v24.get_bool(), false, "3.14 == 1234567890 --> false");

    let v25 = Variant::from_bool(v2.op_partial(&v5)?);
    t.is(v25.type_(), Type::Boolean, "3.14 == 1200 --> boolean");
    t.is(v25.get_bool(), false, "3.14 == 1200 --> false");

    let v30 = Variant::from_bool(v3.op_partial(&v0)?);
    t.is(v30.type_(), Type::Boolean, "'foo' == true --> boolean");
    t.is(v30.get_bool(), false, "'foo' == true --> false");

    let v31 = Variant::from_bool(v3.op_partial(&v1)?);
    t.is(v31.type_(), Type::Boolean, "'foo' == 42 --> boolean");
    t.is(v31.get_bool(), false, "'foo' == 42 --> false");

    let v32 = Variant::from_bool(v3.op_partial(&v2)?);
    t.is(v32.type_(), Type::Boolean, "'foo' == 3.14 --> boolean");
    t.is(v32.get_bool(), false, "'foo' == 3.14 --> false");

    let v33 = Variant::from_bool(v3.op_partial(&v3)?);
    t.is(v33.type_(), Type::Boolean, "'foo' == 'foo' --> boolean");
    t.is(v33.get_bool(), true, "'foo' == 'foo' --> true");

    let v34 = Variant::from_bool(v3.op_partial(&v4)?);
    t.is(
        v34.type_(),
        Type::Boolean,
        "'foo' == 1234567890 --> boolean",
    );
    t.is(v34.get_bool(), false, "'foo' == 1234567890 --> false");

    let v35 = Variant::from_bool(v3.op_partial(&v5)?);
    t.is(v35.type_(), Type::Boolean, "'foo' == 1200 --> boolean");
    t.is(v35.get_bool(), false, "'foo' == 1200 --> false");

    let v40 = Variant::from_bool(v4.op_partial(&v0)?);
    t.is(v40.type_(), Type::Boolean, "1234567890 == true --> boolean");
    t.is(v40.get_bool(), false, "1234567890 == true --> false");

    let v41 = Variant::from_bool(v4.op_partial(&v1)?);
    t.is(v41.type_(), Type::Boolean, "1234567890 == 42 --> boolean");
    t.is(v41.get_bool(), false, "1234567890 == 42 --> false");

    let v42 = Variant::from_bool(v4.op_partial(&v2)?);
    t.is(v42.type_(), Type::Boolean, "1234567890 == 3.14 --> boolean");
    t.is(v42.get_bool(), false, "1234567890 == 3.14 --> false");

    let v43 = Variant::from_bool(v4.op_partial(&v3)?);
    t.is(
        v43.type_(),
        Type::Boolean,
        "1234567890 == 'foo' --> boolean",
    );
    t.is(v43.get_bool(), false, "1234567890 == 'foo' --> false");

    let v44 = Variant::from_bool(v4.op_partial(&v4)?);
    t.is(
        v44.type_(),
        Type::Boolean,
        "1234567890 == 1234567890 --> boolean",
    );
    t.is(v44.get_bool(), true, "1234567890 == 1234567890 --> true");

    let v45 = Variant::from_bool(v4.op_partial(&v5)?);
    t.is(v45.type_(), Type::Boolean, "1234567890 == 1200 --> boolean");
    t.is(v45.get_bool(), false, "1234567890 == 1200 --> false");

    let v50 = Variant::from_bool(v5.op_partial(&v0)?);
    t.is(v50.type_(), Type::Boolean, "1200 == true --> boolean");
    t.is(v50.get_bool(), false, "1200 == true --> false");

    let v51 = Variant::from_bool(v5.op_partial(&v1)?);
    t.is(v51.type_(), Type::Boolean, "1200 == 42 --> boolean");
    t.is(v51.get_bool(), false, "1200 == 42 --> false");

    let v52 = Variant::from_bool(v5.op_partial(&v2)?);
    t.is(v52.type_(), Type::Boolean, "1200 == 3.14 --> boolean");
    t.is(v52.get_bool(), false, "1200 == 3.14 --> false");

    let v53 = Variant::from_bool(v5.op_partial(&v3)?);
    t.is(v53.type_(), Type::Boolean, "1200 == 'foo' --> boolean");
    t.is(v53.get_bool(), false, "1200 == 'foo' --> false");

    // As in the C++, the two checks below test v04 rather than v54.
    let _v54 = Variant::from_bool(v5.op_partial(&v4)?);
    t.is(v04.type_(), Type::Boolean, "1200 == 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "1200 == 1234567890 --> false");

    let v55 = Variant::from_bool(v5.op_partial(&v5)?);
    t.is(v55.type_(), Type::Boolean, "1200 == 1200 --> boolean");
    t.is(v55.get_bool(), true, "1200 == 1200 --> true");

    // Deviation: 1234522800 is 2009-02-13 11:00 UTC, which is local
    // 2009-02-13 only in zones between UTC-11 and UTC+12:59; the C++
    // expectation `true` is wrong elsewhere (e.g. Pacific/Chatham, where it
    // is 2009-02-14 00:45). The expectation is computed from the local date.
    let local = Datetime::from_epoch(1234522800);
    let same_day = (local.year(), local.month(), local.day()) == (2009, 2, 13);

    let v56 = Variant::from_bool(v6.op_partial(&v7)?);
    t.is(
        v56.type_(),
        Type::Boolean,
        "1234522800 == '2009-02-13' --> boolean",
    );
    t.is(
        v56.get_bool(),
        same_day,
        "1234522800 == '2009-02-13' --> true",
    );

    let v57 = Variant::from_bool(v7.op_partial(&v6)?);
    t.is(
        v57.type_(),
        Type::Boolean,
        "'2009-02-13' == 1234522800 --> boolean",
    );
    t.is(
        v57.get_bool(),
        same_day,
        "'2009-02-13' == 1234522800 --> true",
    );

    Ok(())
}

#[test]
fn variant_partial_test() {
    for zone in ZONES {
        let _u = Upstream::setup(Some(zone));
        let mut t = UnitTest::new();
        t.diag(zone);

        if let Err(e) = suite(&mut t) {
            t.diag(&e.to_string());
        }

        t.check(PLANNED);
    }
}

#[test]
fn partial_strings_and_dates() {
    let _u = Upstream::setup(Some("UTC"));
    let s = Variant::from_string;
    let p = |left: &Variant, right: &Variant| left.op_partial(right).unwrap();

    // string <partial> string --> leftmost.
    assert!(p(&s("foobar"), &s("foo")));
    assert!(p(&s("'foobar'"), &s("\"foo\"")));
    assert!(!p(&s("foo"), &s("foobar")));
    assert!(!p(&s("foobar"), &s("bar")));
    assert!(!p(&s(""), &s("x")));
    assert!(!p(&s("x"), &s("")));
    assert!(p(&s(""), &s("")));
    assert!(p(&s("2.75"), &Variant::from_real(2.75)));
    assert!(p(&s("421"), &Variant::from_int(42)));
    assert!(p(&s("truest"), &Variant::from_bool(true)));
    assert!(!s("foobar").op_nopartial(&s("foo")).unwrap());
    assert!(s("foo").op_nopartial(&s("foobar")).unwrap());

    // Status is always compared caseless (and in full).
    let mut status = s("Pending");
    status.set_source("status");
    assert!(p(&status, &s("pending")));
    assert!(!p(&status, &s("pend")));

    // date <partial> date --> same day check.
    let morning = Variant::from_time(1234483200, Type::Date).unwrap(); // 2009-02-13T00:00:00Z
    let evening = Variant::from_time(1234569599, Type::Date).unwrap(); // 2009-02-13T23:59:59Z
    let next = Variant::from_time(1234569600, Type::Date).unwrap(); // 2009-02-14T00:00:00Z
    assert!(p(&morning, &evening));
    assert!(!p(&evening, &next));
    assert!(morning.op_nopartial(&next).unwrap());
    assert!(p(&Variant::from_int(1234483200), &evening));
    assert!(p(&Variant::from_real(1234483200.0), &evening));
    assert!(p(&s("2009-02-13T12:00:00Z"), &morning));
    assert!(p(&morning, &s("2009-02-13")));
    // Trivial operands never match a date.
    assert!(!p(&s(""), &morning));
    assert!(!p(
        &Variant::from_time(0, Type::Date).unwrap(),
        &s("1970-01-01")
    ));

    // Durations compare exactly.
    let hour = Variant::from_time(3600, Type::Duration).unwrap();
    assert!(p(&hour, &s("1h")));
    assert!(p(&s("PT1H"), &hour));
    assert!(!p(&hour, &Variant::from_int(3599)));
}

#[test]
fn partial_cast_error() {
    let _u = Upstream::setup(Some("UTC"));
    Variant::set_date_format("D.M.Y");
    let date = Variant::from_time(1234483200, Type::Date).unwrap();

    let e = Variant::from_string("'not a date'")
        .op_partial(&date)
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "'not a date' is not a valid date in the 'D.M.Y' format."
    );
    let e = date
        .op_nopartial(&Variant::from_string("not a date"))
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "'not a date' is not a valid date in the 'D.M.Y' format."
    );
    assert!(Variant::from_string("13.02.2009")
        .op_partial(&date)
        .unwrap());
}
