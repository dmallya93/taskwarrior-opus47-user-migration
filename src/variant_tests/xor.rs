//! Port of Taskwarrior `test/variant_xor_test.cpp` (76 assertions, as
//! planned by its `UnitTest t (76)`), with the C++ messages.

use super::*;

/// Assertions executed by variant_xor_test.cpp.
const PLANNED: usize = 76;

#[allow(clippy::approx_constant)] // The C++ tests use the literal 3.14.
fn suite(t: &mut UnitTest) -> Result<()> {
    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;

    // Truth table.
    let v_false = Variant::from_bool(false);
    let v_true = Variant::from_bool(true);
    t.is(v_false.op_xor(&v_false), false, "false xor false --> false");
    t.is(v_false.op_xor(&v_true), true, "false xor true --> false");
    t.is(v_true.op_xor(&v_false), true, "true xor false --> false");
    t.is(v_true.op_xor(&v_true), false, "true xor true --> false");

    let v00 = Variant::from_bool(v0.op_xor(&v0));
    t.is(v00.type_(), Type::Boolean, "true xor true --> boolean");
    t.is(v00.get_bool(), false, "true xor true --> false");

    let v01 = Variant::from_bool(v0.op_xor(&v1));
    t.is(v01.type_(), Type::Boolean, "true xor 42 --> boolean");
    t.is(v01.get_bool(), false, "true xor 42 --> false");

    let v02 = Variant::from_bool(v0.op_xor(&v2));
    t.is(v02.type_(), Type::Boolean, "true xor 3.14 --> boolean");
    t.is(v02.get_bool(), false, "true xor 3.14 --> false");

    let v03 = Variant::from_bool(v0.op_xor(&v3));
    t.is(v03.type_(), Type::Boolean, "true xor 'foo' --> boolean");
    t.is(v03.get_bool(), false, "true xor 'foo' --> false");

    let v04 = Variant::from_bool(v0.op_xor(&v4));
    t.is(v04.type_(), Type::Boolean, "true xor 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "true xor 1234567890 --> false");

    let v05 = Variant::from_bool(v0.op_xor(&v5));
    t.is(v05.type_(), Type::Boolean, "true xor 1200 --> boolean");
    t.is(v05.get_bool(), false, "true xor 1200 --> false");

    let v10 = Variant::from_bool(v1.op_xor(&v0));
    t.is(v10.type_(), Type::Boolean, "42 xor true --> boolean");
    t.is(v10.get_bool(), false, "42 xor true --> false");

    let v11 = Variant::from_bool(v1.op_xor(&v1));
    t.is(v11.type_(), Type::Boolean, "42 xor 42 --> boolean");
    t.is(v11.get_bool(), false, "42 xor 42 --> false");

    let v12 = Variant::from_bool(v1.op_xor(&v2));
    t.is(v12.type_(), Type::Boolean, "42 xor 3.14 --> boolean");
    t.is(v12.get_bool(), false, "42 xor 3.14 --> false");

    let v13 = Variant::from_bool(v1.op_xor(&v3));
    t.is(v13.type_(), Type::Boolean, "42 xor 'foo' --> boolean");
    t.is(v13.get_bool(), false, "42 xor 'foo' --> false");

    // As in the C++, the two checks below test v04 rather than v14.
    let _v14 = Variant::from_bool(v1.op_xor(&v4));
    t.is(v04.type_(), Type::Boolean, "42 xor 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "42 xor 1234567890 --> false");

    let v15 = Variant::from_bool(v1.op_xor(&v5));
    t.is(v15.type_(), Type::Boolean, "42 xor 1200 --> boolean");
    t.is(v15.get_bool(), false, "42 xor 1200 --> false");

    let v20 = Variant::from_bool(v2.op_xor(&v0));
    t.is(v20.type_(), Type::Boolean, "3.14 xor true --> boolean");
    t.is(v20.get_bool(), false, "3.14 xor true --> false");

    let v21 = Variant::from_bool(v2.op_xor(&v1));
    t.is(v21.type_(), Type::Boolean, "3.14 xor 42 --> boolean");
    t.is(v21.get_bool(), false, "3.14 xor 42 --> false");

    let v22 = Variant::from_bool(v2.op_xor(&v2));
    t.is(v22.type_(), Type::Boolean, "3.14 xor 3.14 --> boolean");
    t.is(v22.get_bool(), false, "3.14 xor 3.14 --> false");

    let v23 = Variant::from_bool(v2.op_xor(&v3));
    t.is(v23.type_(), Type::Boolean, "3.14 xor 'foo' --> boolean");
    t.is(v23.get_bool(), false, "3.14 xor 'foo' --> false");

    let v24 = Variant::from_bool(v2.op_xor(&v4));
    t.is(v24.type_(), Type::Boolean, "3.14 xor 1234567890 --> boolean");
    t.is(v24.get_bool(), false, "3.14 xor 1234567890 --> false");

    let v25 = Variant::from_bool(v2.op_xor(&v5));
    t.is(v25.type_(), Type::Boolean, "3.14 xor 1200 --> boolean");
    t.is(v25.get_bool(), false, "3.14 xor 1200 --> false");

    let v30 = Variant::from_bool(v3.op_xor(&v0));
    t.is(v30.type_(), Type::Boolean, "'foo' xor true --> boolean");
    t.is(v30.get_bool(), false, "'foo' xor true --> false");

    let v31 = Variant::from_bool(v3.op_xor(&v1));
    t.is(v31.type_(), Type::Boolean, "'foo' xor 42 --> boolean");
    t.is(v31.get_bool(), false, "'foo' xor 42 --> false");

    let v32 = Variant::from_bool(v3.op_xor(&v2));
    t.is(v32.type_(), Type::Boolean, "'foo' xor 3.14 --> boolean");
    t.is(v32.get_bool(), false, "'foo' xor 3.14 --> false");

    let v33 = Variant::from_bool(v3.op_xor(&v3));
    t.is(v33.type_(), Type::Boolean, "'foo' xor 'foo' --> boolean");
    t.is(v33.get_bool(), false, "'foo' xor 'foo' --> false");

    let v34 = Variant::from_bool(v3.op_xor(&v4));
    t.is(v34.type_(), Type::Boolean, "'foo' xor 1234567890 --> boolean");
    t.is(v34.get_bool(), false, "'foo' xor 1234567890 --> false");

    let v35 = Variant::from_bool(v3.op_xor(&v5));
    t.is(v35.type_(), Type::Boolean, "'foo' xor 1200 --> boolean");
    t.is(v35.get_bool(), false, "'foo' xor 1200 --> false");

    let v40 = Variant::from_bool(v4.op_xor(&v0));
    t.is(v40.type_(), Type::Boolean, "1234567890 xor true --> boolean");
    t.is(v40.get_bool(), false, "1234567890 xor true --> false");

    let v41 = Variant::from_bool(v4.op_xor(&v1));
    t.is(v41.type_(), Type::Boolean, "1234567890 xor 42 --> boolean");
    t.is(v41.get_bool(), false, "1234567890 xor 42 --> false");

    let v42 = Variant::from_bool(v4.op_xor(&v2));
    t.is(v42.type_(), Type::Boolean, "1234567890 xor 3.14 --> boolean");
    t.is(v42.get_bool(), false, "1234567890 xor 3.14 --> false");

    let v43 = Variant::from_bool(v4.op_xor(&v3));
    t.is(v43.type_(), Type::Boolean, "1234567890 xor 'foo' --> boolean");
    t.is(v43.get_bool(), false, "1234567890 xor 'foo' --> false");

    let v44 = Variant::from_bool(v4.op_xor(&v4));
    t.is(v44.type_(), Type::Boolean, "1234567890 xor 1234567890 --> boolean");
    t.is(v44.get_bool(), false, "1234567890 xor 1234567890 --> false");

    let v45 = Variant::from_bool(v4.op_xor(&v5));
    t.is(v45.type_(), Type::Boolean, "1234567890 xor 1200 --> boolean");
    t.is(v45.get_bool(), false, "1234567890 xor 1200 --> false");

    let v50 = Variant::from_bool(v5.op_xor(&v0));
    t.is(v50.type_(), Type::Boolean, "1200 xor true --> boolean");
    t.is(v50.get_bool(), false, "1200 xor true --> false");

    let v51 = Variant::from_bool(v5.op_xor(&v1));
    t.is(v51.type_(), Type::Boolean, "1200 xor 42 --> boolean");
    t.is(v51.get_bool(), false, "1200 xor 42 --> false");

    let v52 = Variant::from_bool(v5.op_xor(&v2));
    t.is(v52.type_(), Type::Boolean, "1200 xor 3.14 --> boolean");
    t.is(v52.get_bool(), false, "1200 xor 3.14 --> false");

    let v53 = Variant::from_bool(v5.op_xor(&v3));
    t.is(v53.type_(), Type::Boolean, "1200 xor 'foo' --> boolean");
    t.is(v53.get_bool(), false, "1200 xor 'foo' --> false");

    // As in the C++, the two checks below test v04 rather than v54.
    let _v54 = Variant::from_bool(v5.op_xor(&v4));
    t.is(v04.type_(), Type::Boolean, "1200 xor 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "1200 xor 1234567890 --> false");

    let v55 = Variant::from_bool(v5.op_xor(&v5));
    t.is(v55.type_(), Type::Boolean, "1200 xor 1200 --> boolean");
    t.is(v55.get_bool(), false, "1200 xor 1200 --> false");

    Ok(())
}

#[test]
fn variant_xor_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
