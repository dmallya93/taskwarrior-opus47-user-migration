//! Port of Taskwarrior `test/variant_gte_test.cpp` (72 assertions, as
//! planned by its `UnitTest t (72)`), with the C++ messages.

use super::*;

/// Assertions executed by variant_gte_test.cpp.
const PLANNED: usize = 72;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
fn suite(t: &mut UnitTest) -> Result<()> {
    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;

    let v00 = Variant::from_bool(v0.op_gte(&v0)?);
    t.is(v00.type_(), Type::Boolean, "true >= true --> boolean");
    t.is(v00.get_bool(), true, "true >= true --> true");

    let v01 = Variant::from_bool(v0.op_gte(&v1)?);
    t.is(v01.type_(), Type::Boolean, "true >= 42 --> boolean");
    t.is(v01.get_bool(), false, "true >= 42 --> false");

    let v02 = Variant::from_bool(v0.op_gte(&v2)?);
    t.is(v02.type_(), Type::Boolean, "true >= 3.14 --> boolean");
    t.is(v02.get_bool(), false, "true >= 3.14 --> false");

    let v03 = Variant::from_bool(v0.op_gte(&v3)?);
    t.is(v03.type_(), Type::Boolean, "true >= 'foo' --> boolean");
    t.is(v03.get_bool(), true, "true >= 'foo' --> true");

    let v04 = Variant::from_bool(v0.op_gte(&v4)?);
    t.is(v04.type_(), Type::Boolean, "true >= 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "true >= 1234567890 --> false");

    let v05 = Variant::from_bool(v0.op_gte(&v5)?);
    t.is(v05.type_(), Type::Boolean, "true >= 1200 --> boolean");
    t.is(v05.get_bool(), false, "true >= 1200 --> false");

    let v10 = Variant::from_bool(v1.op_gte(&v0)?);
    t.is(v10.type_(), Type::Boolean, "42 >= true --> boolean");
    t.is(v10.get_bool(), true, "42 >= true --> true");

    let v11 = Variant::from_bool(v1.op_gte(&v1)?);
    t.is(v11.type_(), Type::Boolean, "42 >= 42 --> boolean");
    t.is(v11.get_bool(), true, "42 >= 42 --> true");

    let v12 = Variant::from_bool(v1.op_gte(&v2)?);
    t.is(v12.type_(), Type::Boolean, "42 >= 3.14 --> boolean");
    t.is(v12.get_bool(), true, "42 >= 3.14 --> true");

    let v13 = Variant::from_bool(v1.op_gte(&v3)?);
    t.is(v13.type_(), Type::Boolean, "42 >= 'foo' --> boolean");
    t.is(v13.get_bool(), false, "42 >= 'foo' --> false");

    // As in the C++, the two checks below test v04 rather than v14.
    let _v14 = Variant::from_bool(v1.op_gte(&v4)?);
    t.is(v04.type_(), Type::Boolean, "42 >= 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "42 >= 1234567890 --> false");

    let v15 = Variant::from_bool(v1.op_gte(&v5)?);
    t.is(v15.type_(), Type::Boolean, "42 >= 1200 --> boolean");
    t.is(v15.get_bool(), false, "42 >= 1200 --> false");

    let v20 = Variant::from_bool(v2.op_gte(&v0)?);
    t.is(v20.type_(), Type::Boolean, "3.14 >= true --> boolean");
    t.is(v20.get_bool(), true, "3.14 >= true --> true");

    let v21 = Variant::from_bool(v2.op_gte(&v1)?);
    t.is(v21.type_(), Type::Boolean, "3.14 >= 42 --> boolean");
    t.is(v21.get_bool(), false, "3.14 >= 42 --> false");

    let v22 = Variant::from_bool(v2.op_gte(&v2)?);
    t.is(v22.type_(), Type::Boolean, "3.14 >= 3.14 --> boolean");
    t.is(v22.get_bool(), true, "3.14 >= 3.14 --> true");

    let v23 = Variant::from_bool(v2.op_gte(&v3)?);
    t.is(v23.type_(), Type::Boolean, "3.14 >= 'foo' --> boolean");
    t.is(v23.get_bool(), false, "3.14 >= 'foo' --> false");

    let v24 = Variant::from_bool(v2.op_gte(&v4)?);
    t.is(v24.type_(), Type::Boolean, "3.14 >= 1234567890 --> boolean");
    t.is(v24.get_bool(), false, "3.14 >= 1234567890 --> false");

    let v25 = Variant::from_bool(v2.op_gte(&v5)?);
    t.is(v25.type_(), Type::Boolean, "3.14 >= 1200 --> boolean");
    t.is(v25.get_bool(), false, "3.14 >= 1200 --> false");

    let v30 = Variant::from_bool(v3.op_gte(&v0)?);
    t.is(v30.type_(), Type::Boolean, "'foo' >= true --> boolean");
    t.is(v30.get_bool(), false, "'foo' >= true --> false");

    let v31 = Variant::from_bool(v3.op_gte(&v1)?);
    t.is(v31.type_(), Type::Boolean, "'foo' >= 42 --> boolean");
    t.is(v31.get_bool(), true, "'foo' >= 42 --> true");

    let v32 = Variant::from_bool(v3.op_gte(&v2)?);
    t.is(v32.type_(), Type::Boolean, "'foo' >= 3.14 --> boolean");
    t.is(v32.get_bool(), true, "'foo' >= 3.14 --> true");

    let v33 = Variant::from_bool(v3.op_gte(&v3)?);
    t.is(v33.type_(), Type::Boolean, "'foo' >= 'foo' --> boolean");
    t.is(v33.get_bool(), true, "'foo' >= 'foo' --> true");

    let v34 = Variant::from_bool(v3.op_gte(&v4)?);
    t.is(
        v34.type_(),
        Type::Boolean,
        "'foo' >= 1234567890 --> boolean",
    );
    t.is(v34.get_bool(), false, "'foo' >= 1234567890 --> false");

    let v35 = Variant::from_bool(v3.op_gte(&v5)?);
    t.is(v35.type_(), Type::Boolean, "'foo' >= 1200 --> boolean");
    t.is(v35.get_bool(), false, "'foo' >= 1200 --> false");

    let v40 = Variant::from_bool(v4.op_gte(&v0)?);
    t.is(v40.type_(), Type::Boolean, "1234567890 >= true --> boolean");
    t.is(v40.get_bool(), true, "1234567890 >= true --> true");

    let v41 = Variant::from_bool(v4.op_gte(&v1)?);
    t.is(v41.type_(), Type::Boolean, "1234567890 >= 42 --> boolean");
    t.is(v41.get_bool(), true, "1234567890 >= 42 --> true");

    let v42 = Variant::from_bool(v4.op_gte(&v2)?);
    t.is(v42.type_(), Type::Boolean, "1234567890 >= 3.14 --> boolean");
    t.is(v42.get_bool(), true, "1234567890 >= 3.14 --> true");

    let v43 = Variant::from_bool(v4.op_gte(&v3)?);
    t.is(
        v43.type_(),
        Type::Boolean,
        "1234567890 >= 'foo' --> boolean",
    );
    t.is(v43.get_bool(), true, "1234567890 >= 'foo' --> true");

    let v44 = Variant::from_bool(v4.op_gte(&v4)?);
    t.is(
        v44.type_(),
        Type::Boolean,
        "1234567890 >= 1234567890 --> boolean",
    );
    t.is(v44.get_bool(), true, "1234567890 >= 1234567890 --> true");

    let v45 = Variant::from_bool(v4.op_gte(&v5)?);
    // 1234567890 corresponds to Fri Feb 13 06:31:30 PM EST 2009 hence 1200
    // (evaluated as now+1200s) be in future for any date after 2009-02-13
    t.is(v45.type_(), Type::Boolean, "1234567890 >= 1200 --> boolean");
    t.is(v45.get_bool(), false, "1234567890 >= 1200 --> false");

    let v50 = Variant::from_bool(v5.op_gte(&v0)?);
    t.is(v50.type_(), Type::Boolean, "1200 >= true --> boolean");
    t.is(v50.get_bool(), true, "1200 >= true --> true");

    let v51 = Variant::from_bool(v5.op_gte(&v1)?);
    t.is(v51.type_(), Type::Boolean, "1200 >= 42 --> boolean");
    t.is(v51.get_bool(), true, "1200 >= 42 --> true");

    let v52 = Variant::from_bool(v5.op_gte(&v2)?);
    t.is(v52.type_(), Type::Boolean, "1200 >= 3.14 --> boolean");
    t.is(v52.get_bool(), true, "1200 >= 3.14 --> true");

    let v53 = Variant::from_bool(v5.op_gte(&v3)?);
    t.is(v53.type_(), Type::Boolean, "1200 >= 'foo' --> boolean");
    t.is(v53.get_bool(), true, "1200 >= 'foo' --> true");

    let v54 = Variant::from_bool(v5.op_gte(&v4)?);
    // Same reasoning as v45
    t.is(v54.type_(), Type::Boolean, "1200 >= 1234567890 --> boolean");
    t.is(v54.get_bool(), true, "1200 >= 1234567890 --> true");

    let v55 = Variant::from_bool(v5.op_gte(&v5)?);
    t.is(v55.type_(), Type::Boolean, "1200 >= 1200 --> boolean");
    t.is(v55.get_bool(), true, "1200 >= 1200 --> true");

    Ok(())
}

#[test]
fn variant_gte_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
