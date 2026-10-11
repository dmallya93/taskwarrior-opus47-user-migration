//! Port of Taskwarrior `test/variant_divide_test.cpp` (46 assertions executed;
//! its `UnitTest t (44)` plan is stale), with the C++ messages.

use super::*;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
const EPSILON: f64 = 0.0001;

/// Assertions executed by variant_divide_test.cpp.
const PLANNED: usize = 46;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
fn suite(t: &mut UnitTest) -> Result<()> {
    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;

    // boolean / boolean  -> ERROR
    match v0.op_div(&v0) {
        Ok(_v00) => t.fail("true / true --> error"),
        Err(_) => t.pass("true / true --> error"),
    }

    // boolean / integer  -> ERROR
    match v0.op_div(&v1) {
        Ok(_v01) => t.fail("true / 42 --> error"),
        Err(_) => t.pass("true / 42 --> error"),
    }

    // boolean / real     -> ERROR
    match v0.op_div(&v2) {
        Ok(_v02) => t.fail("true / 3.14 --> error"),
        Err(_) => t.pass("true / 3.14 --> error"),
    }

    // boolean / string   -> ERROR
    match v0.op_div(&v3) {
        Ok(_v03) => t.fail("true / foo --> error"),
        Err(_) => t.pass("true / foo --> error"),
    }

    // boolean / date     -> ERROR
    match v0.op_div(&v4) {
        Ok(_v04) => t.fail("true / 1234567890 --> error"),
        Err(_) => t.pass("true / 1234567890 --> error"),
    }

    // boolean / duration -> ERROR
    match v0.op_div(&v5) {
        Ok(_v05) => t.fail("true / 1200 --> error"),
        Err(_) => t.pass("true / 1200 --> error"),
    }

    // integer / boolean  -> ERROR
    match v1.op_div(&v0) {
        Ok(_v10) => t.fail("42 / true --> error"),
        Err(_) => t.pass("42 / true --> error"),
    }

    // integer / integer  -> integer
    let v11 = v1.op_div(&v1)?;
    t.is(v11.type_(), Type::Integer, "42 / 42 --> integer");
    t.is(v11.get_integer(), 1, "42 / 42 --> 1");

    // integer / real     -> real
    let v12 = v1.op_div(&v2)?;
    t.is(v12.type_(), Type::Real, "42 / 3.14 --> real");
    t.is_tol(v12.get_real(), 13.3757, EPSILON, "42 / 3.14 --> 13.3757");

    // integer / string   -> ERROR
    match v1.op_div(&v3) {
        Ok(_v13) => t.fail("42 / foo --> error"),
        Err(_) => t.pass("42 / foo --> error"),
    }

    // integer / date     -> ERROR
    match v1.op_div(&v4) {
        Ok(_v14) => t.fail("42 / 1234567890 --> error"),
        Err(_) => t.pass("42 / 1234567890 --> error"),
    }

    // integer / duration -> duration
    let v15 = v1.op_div(&v5)?;
    t.is(v15.type_(), Type::Duration, "42 / 1200 --> duration");
    t.is(v15.get_duration(), 0, "42 / 1200 --> 0");

    // real / boolean  -> ERROR
    match v2.op_div(&v0) {
        Ok(_v20) => t.fail("3.14 / true --> error"),
        Err(_) => t.pass("3.14 / true --> error"),
    }

    // real / integer  -> real
    let v21 = v2.op_div(&v1)?;
    t.is(v21.type_(), Type::Real, "3.14 / 42 --> real");
    t.is_tol(v21.get_real(), 0.0747, EPSILON, "3.14 / 42 --> 0.0747");

    // real / real     -> real
    let v22 = v2.op_div(&v2)?;
    t.is(v22.type_(), Type::Real, "3.14 / 3.14 --> real");
    t.is_tol(v22.get_real(), 1.0, EPSILON, "3.14 / 3.14 --> 1.0");

    // real / string   -> error
    match v2.op_div(&v3) {
        Ok(_v23) => t.fail("3.14 / foo --> error"),
        Err(_) => t.pass("3.14 / foo --> error"),
    }

    // real / date     -> error
    match v2.op_div(&v4) {
        Ok(_v24) => t.fail("3.14 / 1234567890 --> error"),
        Err(_) => t.pass("3.14 / 1234567890 --> error"),
    }

    // real / duration -> duration
    let v25 = v2.op_div(&v5)?;
    t.is(v25.type_(), Type::Duration, "3.14 / 1200 --> duration");
    t.is(v25.get_duration(), 0, "3.14 / 1200 --> 0");

    // string / boolean  -> ERROR
    match v3.op_div(&v0) {
        Ok(_v30) => t.fail("foo / true --> error"),
        Err(_) => t.pass("foo / true --> error"),
    }

    // string / integer  -> ERROR
    match v3.op_div(&v1) {
        Ok(_v31) => t.fail("foo / 42 --> error"),
        Err(_) => t.pass("foo / 42 --> error"),
    }

    // string / real     -> ERROR
    match v3.op_div(&v2) {
        Ok(_v32) => t.fail("foo / 3.14 --> error"),
        Err(_) => t.pass("foo / 3.14 --> error"),
    }

    // string / string   -> ERROR
    match v3.op_div(&v3) {
        Ok(_v33) => t.fail("foo / foo --> error"),
        Err(_) => t.pass("foo / foo --> error"),
    }

    // string / date     -> ERROR
    match v3.op_div(&v4) {
        Ok(_v34) => t.fail("foo / 1234567890 --> error"),
        Err(_) => t.pass("foo / 1234567890 --> error"),
    }

    // string / duration -> ERROR
    match v3.op_div(&v5) {
        Ok(_v35) => t.fail("foo / 1200 --> error"),
        Err(_) => t.pass("foo / 1200 --> error"),
    }

    // date / boolean  -> ERROR
    match v4.op_div(&v0) {
        Ok(_v40) => t.fail("1234567890 / true --> error"),
        Err(_) => t.pass("1234567890 / true --> error"),
    }

    // date / integer  -> ERROR
    match v4.op_div(&v1) {
        Ok(_v41) => t.fail("1234567890 / 42 --> error"),
        Err(_) => t.pass("1234567890 / 42 --> error"),
    }

    // date / real     -> ERROR
    match v4.op_div(&v2) {
        Ok(_v42) => t.fail("1234567890 / 3.14 --> error"),
        Err(_) => t.pass("1234567890 / 3.14 --> error"),
    }

    // date / string   -> ERROR
    match v4.op_div(&v3) {
        Ok(_v43) => t.fail("1234567890 / foo --> error"),
        Err(_) => t.pass("1234567890 / foo --> error"),
    }

    // date / date     -> ERROR
    match v4.op_div(&v4) {
        Ok(_v44) => t.fail("1234567890 / 1234567890 --> error"),
        Err(_) => t.pass("1234567890 / 1234567890 --> error"),
    }

    // date / duration -> ERROR
    match v4.op_div(&v5) {
        Ok(_v45) => t.fail("1234567890 / 1200 --> error"),
        Err(_) => t.pass("1234567890 / 1200 --> error"),
    }

    // duration / boolean  -> ERROR
    match v5.op_div(&v0) {
        Ok(_v50) => t.fail("1200 / true --> error"),
        Err(_) => t.pass("1200 / true --> error"),
    }

    // duration / integer  -> duration
    let v51 = v5.op_div(&v1)?;
    t.is(v51.type_(), Type::Duration, "1200 / 42 --> duration");
    t.is(v51.get_duration(), 28, "1200 / 42 --> 28");

    // duration / real     -> duration
    let v52 = v5.op_div(&v2)?;
    t.is(v52.type_(), Type::Duration, "1200 / 3.14 --> duration");
    t.is(v52.get_duration(), 382, "1200 / 3.14 --> 382");

    // duration / string   -> string
    match v5.op_div(&v3) {
        Ok(_v53) => t.fail("1200 / foo --> error"),
        Err(_) => t.pass("1200 / foo --> error"),
    }

    // duration / date     -> date
    match v5.op_div(&v4) {
        Ok(_v54) => t.fail("1200 / 1234567890 --> error"),
        Err(_) => t.pass("1200 / 1234567890 --> error"),
    }

    // duration / duration -> duration
    let v55 = v5.op_div(&v5)?;
    t.is(v55.type_(), Type::Real, "1200 / 1200 --> real");
    t.is(v55.get_real(), 1.0, "1200 / 1200 --> 1.0");
    t.is_tol(
        v5.op_div(&v52)?.get_real(),
        3.14136,
        EPSILON,
        "1200 / 382 --> 3.14136",
    );

    Ok(())
}

#[test]
fn variant_divide_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
