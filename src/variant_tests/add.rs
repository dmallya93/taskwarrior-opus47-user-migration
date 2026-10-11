//! Port of Taskwarrior `test/variant_add_test.cpp` (80 assertions, as planned by its `UnitTest t (80)`), with the C++ messages.

use super::*;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
const EPSILON: f64 = 0.001;

/// Assertions executed by variant_add_test.cpp.
const PLANNED: usize = 80;

/// C++ `(int) s[i]` on a `std::string`: the byte at `i`, `'\0'` at (and,
/// instead of undefined behaviour, past) the end.
fn byte_at(s: &str, i: usize) -> i32 {
    s.as_bytes().get(i).copied().map_or(0, i32::from)
}

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
fn suite(t: &mut UnitTest) -> Result<()> {
    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;

    // boolean + boolean  -> ERROR
    match v0.op_add(&v0) {
        Ok(_v00) => t.fail("true + true --> error"),
        Err(_) => t.pass("true + true --> error"),
    }

    // boolean + integer  -> integer
    let v01 = v0.op_add(&v1)?;
    t.is(v01.type_(), Type::Integer, "true + 42 --> integer");
    t.is(v01.get_integer(), 43, "true + 42 --> 43");

    // boolean + real     -> real
    let v02 = v0.op_add(&v2)?;
    t.is(v02.type_(), Type::Real, "true + 3.14 --> real");
    t.is_tol(v02.get_real(), 4.14, EPSILON, "true + 3.14 --> 4.14");

    // boolean + string   -> string
    let v03 = v0.op_add(&v3)?;
    t.is(v03.type_(), Type::String, "true + foo --> string");
    t.is(v03.get_string(), "truefoo", "true + foo --> truefoo");

    // boolean + date     -> date
    let v04 = v0.op_add(&v4)?;
    t.is(v04.type_(), Type::Date, "true + 1234567890 --> date");
    // C++ `t.is (time_t, "1234567891", ...)` resolves to `is (bool, bool, ...)`:
    // both arguments convert to bool (non-zero date, non-null pointer).
    t.is(
        v04.get_date() != 0,
        true,
        "true + 1234567890 --> 1234567891",
    );

    // boolean + duration -> duration
    let v05 = v0.op_add(&v5)?;
    t.is(v05.type_(), Type::Duration, "true + 1200 --> duration");
    // As above, `is (bool, bool, ...)`.
    t.is(v05.get_duration() != 0, true, "true + 1200 --> 1201");

    // integer + boolean  -> integer
    let v10 = v1.op_add(&v0)?;
    t.is(v10.type_(), Type::Integer, "42 + true --> integer");
    t.is(v10.get_integer(), 43, "42 + true --> 43");

    // integer + integer  -> integer
    let v11 = v1.op_add(&v1)?;
    t.is(v11.type_(), Type::Integer, "42 + 42 --> integer");
    t.is(v11.get_integer(), 84, "42 + 42 --> 84");

    // integer + real     -> real
    let v12 = v1.op_add(&v2)?;
    t.is(v12.type_(), Type::Real, "42 + 3.14 --> real");
    t.is_tol(v12.get_real(), 45.14, EPSILON, "42 + 3.14 --> 45.14");

    // integer + string   -> string
    let v13 = v1.op_add(&v3)?;
    t.is(v13.type_(), Type::String, "42 + foo --> string");
    t.is(v13.get_string(), "42foo", "42 + foo --> 42foo");

    // integer + date     -> date
    let v14 = v1.op_add(&v4)?;
    t.is(v14.type_(), Type::Date, "42 + 1234567890 --> date");
    t.is(v14.get_date(), 1234567932, "42 + 1234567890 --> 1234567932");

    // integer + duration -> duration
    let v15 = v1.op_add(&v5)?;
    t.is(v15.type_(), Type::Duration, "42 + 1200 --> duration");
    t.is(v15.get_duration(), 1242, "42 + 1200 --> 1242");

    // real + boolean  -> real
    let v20 = v2.op_add(&v0)?;
    t.is(v20.type_(), Type::Real, "3.14 + true --> real");
    t.is_tol(v20.get_real(), 4.14, EPSILON, "3.14 + true --> 4.14");

    // real + integer  -> real
    let v21 = v2.op_add(&v1)?;
    t.is(v21.type_(), Type::Real, "3.14 + 42 --> real");
    t.is_tol(v21.get_real(), 45.14, EPSILON, "3.14 + 42 --> 45.14");

    // real + real     -> real
    let v22 = v2.op_add(&v2)?;
    t.is(v22.type_(), Type::Real, "3.14 + 3.14 --> real");
    t.is_tol(v22.get_real(), 6.28, EPSILON, "3.14 + 3.14 --> 6.28");

    // real + string   -> string
    let v23 = v2.op_add(&v3)?;
    t.is(v23.type_(), Type::String, "3.14 + foo --> string");
    t.is(v23.get_string(), "3.14foo", "3.14 + foo --> 3.14foo");

    // real + date     -> date
    let v24 = v2.op_add(&v4)?;
    t.is(v24.type_(), Type::Date, "3.14 + 1234567890 --> date");
    t.is(
        v24.get_date(),
        1234567893,
        "3.14 + 1234567890 --> 1234567893",
    );

    // real + duration -> duration
    let v25 = v2.op_add(&v5)?;
    t.is(v25.type_(), Type::Duration, "3.14 + 1200 --> duration");
    t.is(v25.get_duration(), 1203, "3.14 + 1200 --> 1203");

    // string + boolean  -> string
    let v30 = v3.op_add(&v0)?;
    t.is(v30.type_(), Type::String, "foo + true --> string");
    t.is(v30.get_string(), "footrue", "foo + true --> footrue");

    // string + integer  -> string
    let v31 = v3.op_add(&v1)?;
    t.is(v31.type_(), Type::String, "foo + 42 --> string");
    t.is(v31.get_string(), "foo42", "foo + 42 --> foo42");

    // string + real     -> string
    let v32 = v3.op_add(&v2)?;
    t.is(v32.type_(), Type::String, "foo + 3.14 --> string");
    t.is(v32.get_string(), "foo3.14", "foo + 3.14 --> foo3.14");

    // string + string   -> string
    let v33 = v3.op_add(&v3)?;
    t.is(v33.type_(), Type::String, "foo + foo --> string");
    t.is(v33.get_string(), "foofoo", "foo + foo --> foofoo");

    // string + date     -> string
    let v34 = v3.op_add(&v4)?;
    t.is(v34.type_(), Type::String, "foo + 1234567890 --> string");
    let mut s = v34.get_string().to_string();
    t.is(
        byte_at(&s, 7),
        i32::from(b'-'),
        "foo + 1234567890 --> fooYYYY-MM-DDThh:mm:ss",
    );
    t.is(
        byte_at(&s, 10),
        i32::from(b'-'),
        "foo + 1234567890 --> fooYYYY-MM-DDThh:mm:ss",
    );
    t.is(
        byte_at(&s, 13),
        i32::from(b'T'),
        "foo + 1234567890 --> fooYYYY-MM-DDThh:mm:ss",
    );
    t.is(
        byte_at(&s, 16),
        i32::from(b':'),
        "foo + 1234567890 --> fooYYYY-MM-DDThh:mm:ss",
    );
    t.is(
        byte_at(&s, 19),
        i32::from(b':'),
        "foo + 1234567890 --> fooYYYY-MM-DDThh:mm:ss",
    );
    t.is(s.len(), 22, "foo + 1234567890 --> fooYYYY-MM-DDThh:mm:ss");

    // string + duration -> string
    let v35 = v3.op_add(&v5)?;
    t.is(v35.type_(), Type::String, "foo + 1200 --> string");
    t.is(v35.get_string(), "fooPT20M", "foo + 1200 --> fooPT20M");

    // date + boolean  -> date
    let v40 = v4.op_add(&v0)?;
    t.is(v40.type_(), Type::Date, "1234567890 + true --> date");
    t.is(
        v40.get_date(),
        1234567891,
        "1234567890 + true --> 1234567891",
    );

    // date + integer  -> date
    let v41 = v4.op_add(&v1)?;
    t.is(v41.type_(), Type::Date, "1234567890 + 42 --> date");
    t.is(v41.get_date(), 1234567932, "1234567890 + 42 --> 1234567932");

    // date + real     -> date
    let v42 = v4.op_add(&v2)?;
    t.is(v42.type_(), Type::Date, "1234567890 + 3.14 --> date");
    t.is(
        v42.get_date(),
        1234567893,
        "1234567890 + 3.14 --> 1234567893",
    );

    // date + string   -> string
    let v43 = v4.op_add(&v3)?;
    t.is(v43.type_(), Type::String, "1234567890 + foo --> string");
    s = v43.get_string().to_string();
    t.is(
        byte_at(&s, 4),
        i32::from(b'-'),
        "1234567890 + foo --> YYYY-MM-DDThh:mm:ssfoo",
    );
    t.is(
        byte_at(&s, 7),
        i32::from(b'-'),
        "1234567890 + foo --> YYYY-MM-DDThh:mm:ssfoo",
    );
    t.is(
        byte_at(&s, 10),
        i32::from(b'T'),
        "1234567890 + foo --> YYYY-MM-DDThh:mm:ssfoo",
    );
    t.is(
        byte_at(&s, 13),
        i32::from(b':'),
        "1234567890 + foo --> YYYY-MM-DDThh:mm:ssfoo",
    );
    t.is(
        byte_at(&s, 16),
        i32::from(b':'),
        "1234567890 + foo --> YYYY-MM-DDThh:mm:ssfoo",
    );
    t.is(s.len(), 22, "1234567890 + foo --> YYYY-MM-DDThh:mm:ssfoo");

    // date + date     -> ERROR
    match v4.op_add(&v4) {
        Ok(_v44) => t.fail("1234567890 + 1234567890 --> error"),
        Err(_) => t.pass("1234567890 + 1234567890 --> error"),
    }

    // date + duration -> date
    let v45 = v4.op_add(&v5)?;
    t.is(v45.type_(), Type::Date, "1234567890 + 1200 --> date");
    t.is(
        v45.get_date(),
        1234569090,
        "1234567890 + 1200 --> 1234569090",
    );

    // duration + boolean  -> duration
    let v50 = v5.op_add(&v0)?;
    t.is(v50.type_(), Type::Duration, "1200 + true --> duration");
    t.is(v50.get_duration(), 1201, "1200 + true --> 1201");

    // duration + integer  -> duration
    let v51 = v5.op_add(&v1)?;
    t.is(v51.type_(), Type::Duration, "1200 + 42 --> duration");
    t.is(v51.get_duration(), 1242, "1200 + 42 --> 1242");

    // duration + real     -> duration
    let v52 = v5.op_add(&v2)?;
    t.is(v52.type_(), Type::Duration, "1200 + 3.14 --> duration");
    t.is(v52.get_duration(), 1203, "1200 + 3.14 --> 1203");

    // duration + string   -> string
    let v53 = v5.op_add(&v3)?;
    t.is(v53.type_(), Type::String, "1200 + foo --> string");
    t.is(v53.get_string(), "PT20Mfoo", "1200 + foo --> PT20Mfoo");

    // duration + date     -> date
    let v54 = v5.op_add(&v4)?;
    t.is(v54.type_(), Type::Date, "1200 + 1234567890 --> date");
    t.is(
        v54.get_date(),
        1234569090,
        "1200 + 1234567890 --> 1234569090",
    );

    // duration + duration -> duration
    let v55 = v5.op_add(&v5)?;
    t.is(v55.type_(), Type::Duration, "1200 + 1200 --> duration");
    t.is(v55.get_duration(), 2400, "1200 + 1200 --> 2400");

    Ok(())
}

#[test]
fn variant_add_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
