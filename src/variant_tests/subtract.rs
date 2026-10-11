//! Port of Taskwarrior `test/variant_subtract_test.cpp` (56 executed assertions; its `UnitTest t (55)` plan is stale), with the C++ messages.

use super::*;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
const EPSILON: f64 = 0.001;

/// Assertions executed by variant_subtract_test.cpp.
const PLANNED: usize = 56;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
fn suite(t: &mut UnitTest) -> Result<()> {
    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;

    // boolean - boolean  -> ERROR
    match v0.op_sub(&v0) {
        Ok(_v00) => t.fail("true - true --> error"),
        Err(_) => t.pass("true - true --> error"),
    }

    // boolean - integer  -> ERROR
    match v0.op_sub(&v1) {
        Ok(_v01) => t.fail("true - 42 --> error"),
        Err(_) => t.pass("true - 42 --> error"),
    }

    // boolean - real     -> ERROR
    match v0.op_sub(&v2) {
        Ok(_v02) => t.fail("true - 3.14 --> error"),
        Err(_) => t.pass("true - 3.14 --> error"),
    }

    // boolean - string   -> ERROR
    match v0.op_sub(&v3) {
        Ok(_v03) => t.fail("true - foo --> error"),
        Err(_) => t.pass("true - foo --> error"),
    }

    // boolean - date     -> ERROR
    match v0.op_sub(&v4) {
        Ok(_v04) => t.fail("true - 1234567890 --> error"),
        Err(_) => t.pass("true - 1234567890 --> error"),
    }

    // boolean - duration -> ERROR
    match v0.op_sub(&v5) {
        Ok(_v05) => t.fail("true - 1200 --> error"),
        Err(_) => t.pass("true - 1200 --> error"),
    }

    // integer - boolean  -> integer
    let v10 = v1.op_sub(&v0)?;
    t.is(v10.type_(), Type::Integer, "42 - true --> integer");
    t.is(v10.get_integer(), 41, "42 - true --> 41");

    // integer - integer  -> integer
    let v11 = v1.op_sub(&v1)?;
    t.is(v11.type_(), Type::Integer, "42 - 42 --> integer");
    t.is(v11.get_integer(), 0, "42 - 42 --> 0");

    // integer - real     -> real
    let v12 = v1.op_sub(&v2)?;
    t.is(v12.type_(), Type::Real, "42 - 3.14 --> real");
    t.is_tol(v12.get_real(), 38.86, EPSILON, "42 - 3.14 --> 38.86");

    // integer - string   -> ERROR
    match v1.op_sub(&v3) {
        Ok(_v13) => t.fail("42 - foo --> error"),
        Err(_) => t.pass("42 - foo --> error"),
    }

    // integer - date     -> date
    let v1a = Variant::from_int(1300000000);
    let v14 = v1a.op_sub(&v4)?;
    t.is(v14.type_(), Type::Date, "1300000000 - 1234567890 --> date");
    t.is(
        v14.get_date(),
        65432110,
        "1300000000 - 1234567890 --> 65432110",
    );

    // integer - duration -> duration
    let v15 = v1a.op_sub(&v5)?;
    t.is(
        v15.type_(),
        Type::Duration,
        "1300000000 - 1200 --> duration",
    );
    t.is(
        v15.get_duration(),
        1299998800,
        "1300000000 - 1200 --> 1299998800",
    );

    // real - boolean  -> real
    let v20 = v2.op_sub(&v0)?;
    t.is(v20.type_(), Type::Real, "3.14 - true --> real");
    t.is_tol(v20.get_real(), 2.14, EPSILON, "3.14 - true --> 2.14");

    // real - integer  -> real
    let v21 = v2.op_sub(&v1)?;
    t.is(v21.type_(), Type::Real, "3.14 - 42 --> real");
    t.is_tol(v21.get_real(), -38.86, EPSILON, "3.14 - 42 --> -38.86");

    // real - real     -> real
    let v22 = v2.op_sub(&v2)?;
    t.is(v22.type_(), Type::Real, "3.14 - 3.14 --> real");
    t.is_tol(v22.get_real(), 0.0, EPSILON, "3.14 - 3.14 --> 0.0");

    // real - string   -> ERROR
    // As in the C++, this case tests v1 (42) rather than v2 (3.14).
    match v1.op_sub(&v3) {
        Ok(_v23) => t.fail("3.14 - foo --> error"),
        Err(_) => t.pass("3.14 - foo --> error"),
    }

    // real - date     -> real
    let v2a = Variant::from_real(1300000000.0);
    let v24 = v2a.op_sub(&v4)?;
    t.is(
        v24.type_(),
        Type::Real,
        "1300000000.0 - 1234567890 --> real",
    );
    t.is(
        v24.get_real(),
        65432110.0,
        "1300000000.0 - 1234567890 --> 65432110",
    );

    // real - duration -> real
    let v25 = v2a.op_sub(&v5)?;
    t.is(v25.type_(), Type::Real, "1300000000.0 - 1200 --> real");
    t.is(
        v25.get_real(),
        1299998800.0,
        "1300000000.0 - 1200 --> 1299998800",
    );

    // string - boolean  -> ERROR
    match v3.op_sub(&v0) {
        Ok(_v30) => t.fail("foo - foo --> error"),
        Err(_) => t.pass("foo - foo --> error"),
    }

    // string - integer  -> ERROR
    match v3.op_sub(&v1) {
        Ok(_v31) => t.fail("foo - 42 --> error"),
        Err(_) => t.pass("foo - 42 --> error"),
    }

    // string - real     -> ERROR
    match v3.op_sub(&v2) {
        Ok(_v32) => t.fail("foo - 3.14 --> error"),
        Err(_) => t.pass("foo - 3.14 --> error"),
    }

    // string - string   -> concatenated string
    let v33 = v3.op_sub(&v3)?;
    t.is(v33.type_(), Type::String, "foo - foo --> string");
    t.is(v33.get_string(), "foo-foo", "foo - foo --> foo-foo");

    // string - date     -> ERROR
    match v3.op_sub(&v4) {
        Ok(_v34) => t.fail("foo - 1234567890 --> error"),
        Err(_) => t.pass("foo - 1234567890 --> error"),
    }

    // string - duration -> ERROR
    match v3.op_sub(&v5) {
        Ok(_v35) => t.fail("foo - 1200 --> error"),
        Err(_) => t.pass("foo - 1200 --> error"),
    }

    // date - boolean  -> date
    let v40 = v4.op_sub(&v0)?;
    t.is(v40.type_(), Type::Date, "1234567890 - true --> date");
    t.is(
        v40.get_date(),
        1234567889,
        "1234567890 - true --> 1234567889",
    );

    // date - integer  -> date
    let v41 = v4.op_sub(&v1)?;
    t.is(v41.type_(), Type::Date, "1234567890 - 42 --> date");
    t.is(v41.get_date(), 1234567848, "1234567890 - 42 --> 1234567848");

    // date - real     -> date
    let v42 = v4.op_sub(&v2)?;
    t.is(v42.type_(), Type::Date, "1234567890 - 3.14 --> date");
    t.is(
        v42.get_date(),
        1234567887,
        "1234567890 - 3.14 --> 1234567887",
    );

    // date - string   -> string
    match v4.op_sub(&v3) {
        Ok(_v43) => t.fail("1234567890 - foo --> error"),
        Err(_) => t.pass("1234567890 - foo --> error"),
    }

    // date - date     -> duration
    let v44 = v4.op_sub(&v4)?;
    t.is(
        v44.type_(),
        Type::Duration,
        "1234567890 - 1234567890 --> duration",
    );
    t.is(v44.get_duration(), 0, "1234567890 - 1234567890 --> 0");

    // date - duration -> date
    let v45 = v4.op_sub(&v5)?;
    t.is(v45.type_(), Type::Date, "1234567890 - 1200 --> date");
    t.is(
        v45.get_date(),
        1234566690,
        "1234567890 - 1200 --> 1234566690",
    );

    // duration - boolean  -> duration
    let v50 = v5.op_sub(&v0)?;
    t.is(v50.type_(), Type::Duration, "1200 - true --> duration");
    t.is(v50.get_duration(), 1199, "1200 - true --> 1199");

    // duration - integer  -> duration
    let v51 = v5.op_sub(&v1)?;
    t.is(v51.type_(), Type::Duration, "1200 - 42 --> duration");
    t.is(v51.get_duration(), 1158, "1200 - 42 --> 1158");

    // duration - real     -> duration
    let v52 = v5.op_sub(&v2)?;
    t.is(v52.type_(), Type::Duration, "1200 - 3.14 --> duration");
    t.is(v52.get_duration(), 1197, "1200 - 3.14 --> 1197");

    // duration - string   -> ERROR
    match v5.op_sub(&v3) {
        Ok(_v53) => t.fail("1200 - foo --> error"),
        Err(_) => t.pass("1200 - foo --> error"),
    }

    // duration - date     -> ERROR
    match v5.op_sub(&v4) {
        Ok(_v54) => t.fail("1200 - 1234567890 --> error"),
        Err(_) => t.pass("1200 - 1234567890 --> error"),
    }

    // duration - duration -> duration
    let v55 = v5.op_sub(&v5)?;
    t.is(v55.type_(), Type::Duration, "1200 - 1200 --> duration");
    t.is(v55.get_duration(), 0, "1200 - 1200 --> 0");

    Ok(())
}

#[test]
fn variant_subtract_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
