//! Port of Taskwarrior `test/variant_exp_test.cpp` (38 assertions, as planned by its `UnitTest t (38)`), with the C++ messages.

use super::*;

/// Assertions executed by variant_exp_test.cpp.
const PLANNED: usize = 38;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
fn suite(t: &mut UnitTest) -> Result<()> {
    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;

    // boolean ^ boolean  -> ERROR
    match v0.op_exp(&v0) {
        Ok(_v00) => t.fail("true ^ true --> error"),
        Err(_) => t.pass("true ^ true --> error"),
    }

    // boolean ^ integer  -> ERROR
    match v0.op_exp(&v1) {
        Ok(_v01) => t.fail("true ^ 42 --> error"),
        Err(_) => t.pass("true ^ 42 --> error"),
    }

    // boolean ^ real     -> ERROR
    match v0.op_exp(&v2) {
        Ok(_v02) => t.fail("true ^ 3.14 --> error"),
        Err(_) => t.pass("true ^ 3.14 --> error"),
    }

    // boolean ^ string   -> ERROR
    match v0.op_exp(&v3) {
        Ok(_v03) => t.fail("true ^ foo --> error"),
        Err(_) => t.pass("true ^ foo --> error"),
    }

    // boolean ^ date     -> ERROR
    match v0.op_exp(&v4) {
        Ok(_v04) => t.fail("true ^ 1234567890 --> error"),
        Err(_) => t.pass("true ^ 1234567890 --> error"),
    }

    // boolean ^ duration -> ERROR
    match v0.op_exp(&v5) {
        Ok(_v05) => t.fail("true ^ 1200 --> error"),
        Err(_) => t.pass("true ^ 1200 --> error"),
    }

    // integer ^ boolean  -> ERROR
    match v1.op_exp(&v0) {
        Ok(_v10) => t.fail("42 ^ true --> error"),
        Err(_) => t.pass("42 ^ true --> error"),
    }

    // integer ^ integer  -> integer
    let v11 = v1.op_exp(&Variant::from_int(2))?;
    t.is(v11.type_(), Type::Integer, "42 ^ 2 --> integer");
    t.is(v11.get_integer(), 1764, "42 ^ 2 --> 1764");

    // integer ^ real     -> real
    match v1.op_exp(&v2) {
        Ok(_v12) => t.fail("42 ^ 3.14 --> error"),
        Err(_) => t.pass("42 ^ 3.14 --> error"),
    }

    // integer ^ string   -> ERROR
    match v1.op_exp(&v3) {
        Ok(_v13) => t.fail("42 ^ foo --> error"),
        Err(_) => t.pass("42 ^ foo --> error"),
    }

    // integer ^ date     -> ERROR
    match v1.op_exp(&v4) {
        Ok(_v14) => t.fail("42 ^ 1234567890 --> error"),
        Err(_) => t.pass("42 ^ 1234567890 --> error"),
    }

    // integer ^ duration -> ERROR
    match v1.op_exp(&v5) {
        Ok(_v15) => t.fail("42 ^ 1200 --> error"),
        Err(_) => t.pass("42 ^ 1200 --> error"),
    }

    // real ^ boolean  -> ERROR
    match v2.op_exp(&v0) {
        Ok(_v20) => t.fail("3.14 ^ true --> error"),
        Err(_) => t.pass("3.14 ^ true --> error"),
    }

    // real ^ integer  -> real
    let v21 = v2.op_exp(&Variant::from_int(2))?;
    t.is(v21.type_(), Type::Real, "3.14 ^ 2 --> real");
    t.is_tol(v21.get_real(), 9.8596, 0.001, "3.14 ^ 2 --> 9.8596");

    // real ^ real     -> ERROR
    match v2.op_exp(&v2) {
        Ok(_v22) => t.fail("3.14 ^ 3.14 --> error"),
        Err(_) => t.pass("3.14 ^ 3.14 --> error"),
    }

    // real ^ string   -> ERROR
    match v2.op_exp(&v3) {
        Ok(_v23) => t.fail("3.14 ^ foo --> error"),
        Err(_) => t.pass("3.14 ^ foo --> error"),
    }

    // real ^ date     -> ERROR
    match v2.op_exp(&v4) {
        Ok(_v24) => t.fail("3.14 ^ 1234567890 --> error"),
        Err(_) => t.pass("3.14 ^ 1234567890 --> error"),
    }

    // real ^ duration -> ERROR
    match v2.op_exp(&v5) {
        Ok(_v25) => t.fail("3.14 ^ 1200 --> error"),
        Err(_) => t.pass("3.14 ^ 1200 --> error"),
    }

    // string ^ boolean  -> ERROR
    match v3.op_exp(&v0) {
        Ok(_v30) => t.fail("foo ^ true --> error"),
        Err(_) => t.pass("foo ^ true --> error"),
    }

    // string ^ integer  -> ERROR
    match v3.op_exp(&v1) {
        Ok(_v31) => t.fail("foo ^ 42 --> error"),
        Err(_) => t.pass("foo ^ 42 --> error"),
    }

    // string ^ real     -> ERROR
    match v3.op_exp(&v2) {
        Ok(_v32) => t.fail("foo ^ 3.14 --> error"),
        Err(_) => t.pass("foo ^ 3.14 --> error"),
    }

    // string ^ string   -> ERROR
    match v3.op_exp(&v3) {
        Ok(_v33) => t.fail("foo ^ foo --> error"),
        Err(_) => t.pass("foo ^ foo --> error"),
    }

    // string ^ date     -> ERROR
    match v3.op_exp(&v4) {
        Ok(_v34) => t.fail("foo ^ 1234567890 --> error"),
        Err(_) => t.pass("foo ^ 1234567890 --> error"),
    }

    // string ^ duration -> ERROR
    match v3.op_exp(&v5) {
        Ok(_v35) => t.fail("foo ^ 1200 --> error"),
        Err(_) => t.pass("foo ^ 1200 --> error"),
    }

    // date ^ boolean  -> ERROR
    match v4.op_exp(&v0) {
        Ok(_v40) => t.fail("1234567890 ^ true --> error"),
        Err(_) => t.pass("1234567890 ^ true --> error"),
    }

    // date ^ integer  -> ERROR
    match v4.op_exp(&v1) {
        Ok(_v41) => t.fail("1234567890 ^ 42 --> error"),
        Err(_) => t.pass("1234567890 ^ 42 --> error"),
    }

    // date ^ real     -> ERROR
    match v4.op_exp(&v2) {
        Ok(_v42) => t.fail("1234567890 ^ 3.14 --> error"),
        Err(_) => t.pass("1234567890 ^ 3.14 --> error"),
    }

    // date ^ string   -> ERROR
    match v4.op_exp(&v3) {
        Ok(_v43) => t.fail("1234567890 ^ foo --> error"),
        Err(_) => t.pass("1234567890 ^ foo --> error"),
    }

    // date ^ date     -> ERROR
    match v4.op_exp(&v4) {
        Ok(_v44) => t.fail("1234567890 ^ 1234567890 --> error"),
        Err(_) => t.pass("1234567890 ^ 1234567890 --> error"),
    }

    // date ^ duration -> ERROR
    match v4.op_exp(&v5) {
        Ok(_v45) => t.fail("1234567890 ^ 1200 --> error"),
        Err(_) => t.pass("1234567890 ^ 1200 --> error"),
    }

    // duration ^ boolean  -> ERROR
    match v5.op_exp(&v0) {
        Ok(_v50) => t.fail("1200 ^ true --> error"),
        Err(_) => t.pass("1200 ^ true --> error"),
    }

    // duration ^ integer  -> ERROR
    match v5.op_exp(&v1) {
        Ok(_v51) => t.fail("1200 ^ 42 --> error"),
        Err(_) => t.pass("1200 ^ 42 --> error"),
    }

    // duration ^ real     -> ERROR
    match v5.op_exp(&v2) {
        Ok(_v52) => t.fail("1200 ^ 3.14 --> error"),
        Err(_) => t.pass("1200 ^ 3.14 --> error"),
    }

    // duration ^ string   -> ERROR
    match v5.op_exp(&v3) {
        Ok(_v53) => t.fail("1200 ^ foo --> error"),
        Err(_) => t.pass("1200 ^ foo --> error"),
    }

    // duration ^ date     -> ERROR
    match v5.op_exp(&v4) {
        Ok(_v54) => t.fail("1200 ^ 1234567890 --> error"),
        Err(_) => t.pass("1200 ^ 1234567890 --> error"),
    }

    // duration ^ duration -> ERROR
    match v5.op_exp(&v5) {
        Ok(_v55) => t.fail("1200 ^ 1200 --> error"),
        Err(_) => t.pass("1200 ^ 1200 --> error"),
    }

    Ok(())
}

#[test]
fn variant_exp_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
