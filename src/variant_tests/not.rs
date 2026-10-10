//! Port of Taskwarrior `test/variant_not_test.cpp` (14 assertions, as
//! planned by its `UnitTest t (14)`), with the C++ messages.

use super::*;

/// Assertions executed by variant_not_test.cpp.
const PLANNED: usize = 14;

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
    t.is(v_false.op_not(), true, "!false --> true");
    t.is(v_true.op_not(), false, "!true --> false");

    let v00 = Variant::from_bool(v0.op_not());
    t.is(v00.type_(), Type::Boolean, "! true --> boolean");
    t.is(v00.get_bool(), false, "! true --> false");

    let v01 = Variant::from_bool(v1.op_not());
    t.is(v01.type_(), Type::Boolean, "! 42 --> boolean");
    t.is(v01.get_bool(), false, "! 42 --> false");

    let v02 = Variant::from_bool(v2.op_not());
    t.is(v02.type_(), Type::Boolean, "! 3.14 --> boolean");
    t.is(v02.get_bool(), false, "! 3.14 --> false");

    let v03 = Variant::from_bool(v3.op_not());
    t.is(v03.type_(), Type::Boolean, "! foo --> boolean");
    t.is(v03.get_bool(), false, "! foo --> false");

    let v04 = Variant::from_bool(v4.op_not());
    t.is(v04.type_(), Type::Boolean, "! 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "! 1234567890 --> false");

    let v05 = Variant::from_bool(v5.op_not());
    t.is(v05.type_(), Type::Boolean, "! 1200 --> boolean");
    t.is(v05.get_bool(), false, "! 1200 --> false");

    Ok(())
}

#[test]
fn variant_not_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
