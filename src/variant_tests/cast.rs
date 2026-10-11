//! Port of Taskwarrior `test/variant_cast_test.cpp` (83 executed
//! assertions; its `UnitTest t (81)` plan is stale), with the C++ messages.

use super::*;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
const EPSILON: f64 = 0.001;

/// Assertions executed by variant_cast_test.cpp.
const PLANNED: usize = 83;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
fn suite(t: &mut UnitTest, now: i64) -> Result<()> {
    // Type::Boolean --> *
    let mut v00 = Variant::from_bool(true);
    v00.cast(Type::Boolean)?;
    t.ok(v00.type_() == Type::Boolean, "cast boolean --> boolean");
    t.ok(v00.get_bool(), "cast boolean --> boolean");

    let mut v01 = Variant::from_bool(true);
    v01.cast(Type::Integer)?;
    t.ok(v01.type_() == Type::Integer, "cast boolean --> integer");
    t.ok(v01.get_integer() == 1, "cast boolean --> integer");

    let mut v02 = Variant::from_bool(true);
    v02.cast(Type::Real)?;
    t.ok(v02.type_() == Type::Real, "cast boolean --> real");
    t.is_tol(v02.get_real(), 1.0, EPSILON, "cast boolean --> real");

    let mut v03 = Variant::from_bool(true);
    v03.cast(Type::String)?;
    t.ok(v03.type_() == Type::String, "cast boolean --> string");
    t.ok(v03.get_string() == "true", "cast boolean --> string");

    let mut v04 = Variant::from_bool(true);
    v04.cast(Type::Date)?;
    t.ok(v04.type_() == Type::Date, "cast boolean --> date");
    t.is(v04.get_date(), 1, "cast boolean --> date");

    let mut v05 = Variant::from_bool(true);
    v05.cast(Type::Duration)?;
    t.ok(v05.type_() == Type::Duration, "cast boolean --> duration");
    t.is(v05.get_duration(), 1, "cast boolean --> duration");

    // Type::Integer --> *
    let mut v10 = Variant::from_int(42);
    v10.cast(Type::Boolean)?;
    t.ok(v10.type_() == Type::Boolean, "cast integer --> boolean");
    t.ok(v10.get_bool(), "cast integer --> boolean");

    let mut v11 = Variant::from_int(42);
    v11.cast(Type::Integer)?;
    t.ok(v11.type_() == Type::Integer, "cast integer --> integer");
    t.ok(v11.get_integer() == 42, "cast integer --> integer");

    let mut v12 = Variant::from_int(42);
    v12.cast(Type::Real)?;
    t.ok(v12.type_() == Type::Real, "cast integer --> real");
    t.is_tol(v12.get_real(), 42.0, EPSILON, "cast integer --> real");

    let mut v13 = Variant::from_int(42);
    v13.cast(Type::String)?;
    t.is(v13.type_(), Type::String, "cast integer --> string");
    t.is(v13.get_string(), "42", "cast integer --> string");

    let mut v14 = Variant::from_int(42);
    v14.cast(Type::Date)?;
    t.ok(v14.type_() == Type::Date, "cast integer --> date");
    t.ok(v14.get_date() == 42, "cast integer --> date");

    let mut v15 = Variant::from_int(42);
    v15.cast(Type::Duration)?;
    t.is(v15.type_(), Type::Duration, "cast integer --> duration");
    t.is(v15.get_duration(), 42, "cast integer --> duration");

    // Type::Real --> *
    let mut v20 = Variant::from_real(3.14);
    v20.cast(Type::Boolean)?;
    t.ok(v20.type_() == Type::Boolean, "cast real --> boolean");
    t.ok(v20.get_bool(), "cast real --> boolean");

    let mut v21 = Variant::from_real(3.14);
    v21.cast(Type::Integer)?;
    t.ok(v21.type_() == Type::Integer, "cast real --> integer");
    t.ok(v21.get_integer() == 3, "cast real --> integer");

    let mut v22 = Variant::from_real(3.14);
    v22.cast(Type::Real)?;
    t.ok(v22.type_() == Type::Real, "cast real --> real");
    t.is_tol(v22.get_real(), 3.14, EPSILON, "cast real --> real");

    let mut v23 = Variant::from_real(3.14);
    v23.cast(Type::String)?;
    t.ok(v23.type_() == Type::String, "cast real --> string");
    t.ok(v23.get_string() == "3.14", "cast real --> string");

    let mut v24 = Variant::from_real(3.14);
    v24.cast(Type::Date)?;
    t.ok(v24.type_() == Type::Date, "cast real --> date");
    t.ok(v24.get_date() == 3, "cast real --> date");

    let mut v25 = Variant::from_real(3.14);
    v25.cast(Type::Duration)?;
    t.ok(v25.type_() == Type::Duration, "cast real --> duration");
    t.ok(v25.get_duration() == 3, "cast real --> duration");

    // Type::String --> *
    let mut v30 = Variant::from_string("foo");
    v30.cast(Type::Boolean)?;
    t.ok(v30.type_() == Type::Boolean, "cast string --> boolean");
    t.ok(v30.get_bool(), "cast string --> boolean");

    let mut v31 = Variant::from_string("42");
    v31.cast(Type::Integer)?;
    t.ok(v31.type_() == Type::Integer, "cast string --> integer");
    t.ok(v31.get_integer() == 42, "cast string --> integer");

    let mut v31h = Variant::from_string("0x20");
    v31h.cast(Type::Integer)?;
    t.ok(
        v31h.type_() == Type::Integer,
        "cast string(hex) --> integer",
    );
    t.ok(v31h.get_integer() == 32, "cast string(hex) --> integer");

    let mut v32 = Variant::from_string("3.14");
    v32.cast(Type::Real)?;
    t.ok(v32.type_() == Type::Real, "cast string --> real");
    t.is_tol(v32.get_real(), 3.14, EPSILON, "cast string --> real");

    let mut v33 = Variant::from_string("foo");
    v33.cast(Type::String)?;
    t.ok(v33.type_() == Type::String, "cast string --> string");
    t.ok(v33.get_string() == "foo", "cast string --> string");

    let mut v34 = Variant::from_string("2013-12-07T16:33:00-05:00");
    v34.cast(Type::Date)?;
    t.ok(v34.type_() == Type::Date, "cast string --> date");
    t.ok(v34.get_date() == 1386451980, "cast string --> date");

    let mut v35 = Variant::from_string("42 days");
    v35.cast(Type::Duration)?;
    t.ok(v35.type_() == Type::Duration, "cast string --> duration");
    t.is(v35.get_duration(), 3628800, "cast string --> duration");

    let mut v35b = Variant::from_string("P42D");
    v35b.cast(Type::Duration)?;
    t.ok(v35b.type_() == Type::Duration, "cast string --> duration");
    t.is(v35b.get_duration(), 3628800, "cast string --> duration");

    // Type::Date --> *
    let mut v40 = Variant::from_time(1234567890, Type::Date)?;
    v40.cast(Type::Boolean)?;
    t.ok(v40.type_() == Type::Boolean, "cast date --> boolean");
    t.ok(v40.get_bool(), "cast date --> boolean");

    let mut v41 = Variant::from_time(1234567890, Type::Date)?;
    v41.cast(Type::Integer)?;
    t.ok(v41.type_() == Type::Integer, "cast date --> integer");
    t.ok(v41.get_integer() == 1234567890, "cast date --> integer");

    let mut v42 = Variant::from_time(1234567890, Type::Date)?;
    v42.cast(Type::Real)?;
    t.ok(v42.type_() == Type::Real, "cast date --> real");
    t.is_tol(v42.get_real(), 1234567890.0, EPSILON, "cast date --> real");

    // YYYY-MM-DDThh:mm:ss
    //     ^  ^  ^  ^  ^
    let mut v43 = Variant::from_time(1234567890, Type::Date)?;
    v43.cast(Type::String)?;
    t.ok(v43.type_() == Type::String, "cast date --> string");
    let s = v43.get_string().as_bytes().to_vec();
    t.is(i32::from(s[4]), '-' as i32, "cast date --> string");
    t.is(i32::from(s[7]), '-' as i32, "cast date --> string");
    t.is(i32::from(s[10]), 'T' as i32, "cast date --> string");
    t.is(i32::from(s[13]), ':' as i32, "cast date --> string");
    t.is(i32::from(s[16]), ':' as i32, "cast date --> string");
    t.is(s.len() as i32, 19, "cast date --> string");

    let mut v44 = Variant::from_time(1234567890, Type::Date)?;
    v44.cast(Type::Date)?;
    t.ok(v44.type_() == Type::Date, "cast date --> date");
    t.ok(v44.get_date() == 1234567890, "cast date --> date");

    let mut v45 = Variant::from_time(1234567890, Type::Date)?;
    v45.cast(Type::Duration)?;
    t.ok(v45.type_() == Type::Duration, "cast date --> duration");
    t.ok(
        v45.get_duration() <= 1234567890 - now,
        "cast date --> duration",
    );
    // Assuming this unit test takes less than 10 min to execute
    t.ok(
        v45.get_duration() > 1234567890 - now - 600,
        "cast date --> duration",
    );

    // Type::Duration --> *
    let mut v50 = Variant::from_time(12345, Type::Duration)?;
    v50.cast(Type::Boolean)?;
    t.ok(v50.type_() == Type::Boolean, "cast duration --> boolean");
    t.ok(v50.get_bool(), "cast duration --> boolean");

    let mut v51 = Variant::from_time(12345, Type::Duration)?;
    v51.cast(Type::Integer)?;
    t.ok(v51.type_() == Type::Integer, "cast duration --> integer");
    t.ok(v51.get_integer() == 12345, "cast duration --> integer");

    let mut v52 = Variant::from_time(12345, Type::Duration)?;
    v52.cast(Type::Real)?;
    t.ok(v52.type_() == Type::Real, "cast duration --> real");
    t.is_tol(v52.get_real(), 12345.0, EPSILON, "cast duration --> real");

    let mut v53 = Variant::from_time(12345, Type::Duration)?;
    v53.cast(Type::String)?;
    t.ok(v53.type_() == Type::String, "cast duration --> string");
    t.is(v53.get_string(), "PT3H25M45S", "cast duration --> string");

    let mut v54 = Variant::from_time(12345, Type::Duration)?;
    v54.cast(Type::Date)?;
    t.ok(v54.type_() == Type::Date, "cast duration --> date");
    t.ok(v54.get_date() >= 12345 + now, "cast duration --> duration");
    t.ok(
        v54.get_date() < 12345 + now + 600,
        "cast duration --> duration",
    );

    let mut v55 = Variant::from_time(12345, Type::Duration)?;
    v55.cast(Type::Duration)?;
    t.ok(v55.type_() == Type::Duration, "cast duration --> duration");
    t.is(v55.get_duration(), 12345, "cast duration --> duration");

    Ok(())
}

#[test]
fn variant_cast_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    let now = localtime::time_now();

    if let Err(e) = suite(&mut t, now) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
