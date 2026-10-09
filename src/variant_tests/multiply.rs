//! Port of Taskwarrior `test/variant_multiply_test.cpp` (54 assertions, as planned by its `UnitTest t (54)`), with the C++ messages.

use super::*;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
const EPSILON: f64 = 0.0001;

/// Assertions executed by variant_multiply_test.cpp.
const PLANNED: usize = 54;

/// C++ `s.substr (pos, n)` (`pos` within `s`): at most `n` bytes from `pos`.
fn substr(s: &str, pos: usize, n: usize) -> &str {
    &s[pos..s.len().min(pos + n)]
}

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
fn suite(t: &mut UnitTest) -> Result<()> {
    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;

    // boolean * boolean  -> ERROR
    match v0.op_mul(&v0) {
        Ok(_v00) => t.fail("true * true --> error"),
        Err(_) => t.pass("true * true --> error"),
    }

    // boolean * integer  -> integer
    let v01 = v0.op_mul(&v1)?;
    t.is(v01.type_(), Type::Integer, "true * 42 --> integer");
    t.is(v01.get_integer(), 42, "true * 42 --> 42");

    // boolean * real     -> real
    let v02 = v0.op_mul(&v2)?;
    t.is(v02.type_(), Type::Real, "true * 3.14 --> real");
    t.is_tol(v02.get_real(), 3.14, EPSILON, "true * 3.14 --> 3.14");

    // boolean * string   -> string
    let v03 = v0.op_mul(&v3)?;
    t.is(v03.type_(), Type::String, "true * foo --> real");
    t.is(v03.get_string(), "foo", "true * foo --> foo");

    // boolean * date     -> ERROR
    match v0.op_mul(&v4) {
        Ok(_v04) => t.fail("true * 1234567890 --> error"),
        Err(_) => t.pass("true * 1234567890 --> error"),
    }

    // boolean * duration -> duration
    let v05 = v0.op_mul(&v5)?;
    t.is(v05.type_(), Type::Duration, "true * 1200 --> duration");
    t.is(v05.get_duration(), 1200, "true * 1200 --> 1200");

    // integer * boolean  -> integer
    let v10 = v1.op_mul(&v0)?;
    t.is(v10.type_(), Type::Integer, "42 * true --> integer");
    t.is(v10.get_integer(), 42, "42 * true --> 42");

    // integer * integer  -> integer
    let v11 = v1.op_mul(&v1)?;
    t.is(v11.type_(), Type::Integer, "42 * 42 --> integer");
    t.is(v11.get_integer(), 1764, "42 * 42 --> 1764");

    // integer * real     -> real
    let v12 = v1.op_mul(&v2)?;
    t.is(v12.type_(), Type::Real, "42 * 3.14 --> real");
    t.is_tol(v12.get_real(), 131.88, EPSILON, "42 * 3.14 --> 131.88");

    // integer * string   -> string
    let v13 = v1.op_mul(&v3)?;
    t.is(v13.type_(), Type::String, "42 * foo --> string");
    t.is(substr(v13.get_string(), 0, 10), "foofoofoof", "42 * foo --> foofoofoofoo...");
    // integer * date     -> error
    match v1.op_mul(&v4) {
        Ok(_v14) => t.fail("42 * 1234567890 --> error"),
        Err(_) => t.pass("42 * 1234567890 --> error"),
    }

    // integer * duration -> duration
    let v15 = v1.op_mul(&v5)?;
    t.is(v15.type_(), Type::Duration, "42 * 1200 --> duration");
    t.is(v15.get_duration(), 50400, "42 * 1200 --> 50400");

    // real * boolean  -> real
    let v20 = v2.op_mul(&v0)?;
    t.is(v20.type_(), Type::Real, "3.14 * true --> real");
    t.is_tol(v20.get_real(), 3.14, EPSILON, "3.14 * true --> 3.14");

    // real * integer  -> real
    let v21 = v2.op_mul(&v1)?;
    t.is(v21.type_(), Type::Real, "3.14 * 42 --> real");
    t.is_tol(v21.get_real(), 131.88, EPSILON, "3.14 * 42 --> 131.88");

    // real * real     -> real
    let v22 = v2.op_mul(&v2)?;
    t.is(v22.type_(), Type::Real, "3.14 * 3.14 --> real");
    t.is_tol(v22.get_real(), 9.8596, EPSILON, "3.14 * 3.14 --> 9.8596");

    // real * string   -> error
    match v2.op_mul(&v3) {
        Ok(_v23) => t.fail("3.14 * foo --> error"),
        Err(_) => t.pass("3.14 * foo --> error"),
    }

    // real * date     -> error
    match v2.op_mul(&v4) {
        Ok(_v24) => t.fail("3.14 * 1234567890 --> error"),
        Err(_) => t.pass("3.14 * 1234567890 --> error"),
    }

    // real * duration -> duration
    let v25 = v2.op_mul(&v5)?;
    t.is(v25.type_(), Type::Duration, "3.14 * 1200 --> duration");
    t.is(v25.get_duration(), 3768, "3.14 * 1200 --> 3768");

    // string * boolean  -> string
    let v30 = v3.op_mul(&v0)?;
    t.is(v30.type_(), Type::String, "foo * true --> real");
    t.is(v30.get_string(), "foo", "foo * true --> foo");

    // string * integer  -> string
    let v31 = v3.op_mul(&v1)?;
    t.is(v31.type_(), Type::String, "foo * 42 --> string");
    t.is(substr(v31.get_string(), 0, 10), "foofoofoof", "foo * 42 --> foofoofoof...");

    // string * real     -> string
    match v3.op_mul(&v2) {
        Ok(_v32) => t.fail("foo * 3.14 --> error"),
        Err(_) => t.pass("foo * 3.14 --> error"),
    }

    // string * string   -> string
    match v3.op_mul(&v3) {
        Ok(_v33) => t.fail("foo * foo --> error"),
        Err(_) => t.pass("foo * foo --> error"),
    }

    // string * date     -> string
    match v3.op_mul(&v4) {
        Ok(_v34) => t.fail("foo * 1234567890 --> error"),
        Err(_) => t.pass("foo * 1234567890 --> error"),
    }

    // string * duration -> string
    match v3.op_mul(&v5) {
        Ok(_v35) => t.fail("foo * 1200 --> error"),
        Err(_) => t.pass("foo * 1200 --> error"),
    }

    // date * boolean  -> ERROR
    match v4.op_mul(&v0) {
        Ok(_v40) => t.fail("1234567890 * true --> error"),
        Err(_) => t.pass("1234567890 * true --> error"),
    }

    // date * integer  -> ERROR
    match v4.op_mul(&v1) {
        Ok(_v41) => t.fail("1234567890 * 42 --> error"),
        Err(_) => t.pass("1234567890 * 42 --> error"),
    }

    // date * real     -> ERROR
    match v4.op_mul(&v2) {
        Ok(_v42) => t.fail("1234567890 * 3.14 --> error"),
        Err(_) => t.pass("1234567890 * 3.14 --> error"),
    }

    // date * string   -> ERROR
    match v4.op_mul(&v3) {
        Ok(_v43) => t.fail("1234567890 * foo --> error"),
        Err(_) => t.pass("1234567890 * foo --> error"),
    }

    // date * date     -> ERROR
    match v4.op_mul(&v4) {
        Ok(_v44) => t.fail("1234567890 * 1234567890 --> error"),
        Err(_) => t.pass("1234567890 * 1234567890 --> error"),
    }

    // date * duration -> ERROR
    match v4.op_mul(&v5) {
        Ok(_v45) => t.fail("1234567890 * 1200 --> error"),
        Err(_) => t.pass("1234567890 * 1200 --> error"),
    }

    // duration * boolean  -> duration
    let v50 = v5.op_mul(&v0)?;
    t.is(v50.type_(), Type::Duration, "1200 * true --> duration");
    t.is(v50.get_duration(), 1200, "1200 * true --> 1200");

    // duration * integer  -> duration
    let v51 = v5.op_mul(&v1)?;
    t.is(v51.type_(), Type::Duration, "1200 * 42 --> duration");
    t.is(v51.get_duration(), 50400, "1200 * 42 --> 50400");

    // duration * real     -> duration
    let v52 = v5.op_mul(&v2)?;
    t.is(v52.type_(), Type::Duration, "1200 * 3.14 --> duration");
    t.is(v52.get_duration(), 3768, "1200 * 3.14 --> 3768");

    // duration * string   -> string
    match v5.op_mul(&v3) {
        Ok(_v53) => t.fail("1200 * foo --> error"),
        Err(_) => t.pass("1200 * foo --> error"),
    }

    // duration * date     -> date
    match v5.op_mul(&v4) {
        Ok(_v54) => t.fail("1200 * 1234567890 --> error"),
        Err(_) => t.pass("1200 * 1234567890 --> error"),
    }

    // duration * duration -> duration
    match v5.op_mul(&v5) {
        Ok(_v55) => t.fail("1200 * 1200 --> error"),
        Err(_) => t.pass("1200 * 1200 --> error"),
    }

    Ok(())
}

#[test]
fn variant_multiply_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
