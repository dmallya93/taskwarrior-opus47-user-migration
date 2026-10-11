//! Port of Taskwarrior `test/variant_nomatch_test.cpp` (120 assertions, as planned by its
//! `UnitTest t (120)`), with the C++ messages.
//!
//! The C++ `Task task;` is the placeholder [`Task`] (no annotations, no
//! tags); the annotation scan is covered by `annotations_and_tags` in
//! `match.rs`.

use super::*;

/// Assertions executed by variant_nomatch_test.cpp.
const PLANNED: usize = 120;

#[allow(clippy::approx_constant)] // The C++ tests use the literals 3.14 and 3.14159.
fn suite(t: &mut UnitTest) -> Result<()> {
    let task = Task::default();

    let vs0 = Variant::from_string("untrue"); // !~ true
    let vs1 = Variant::from_int(8421); // !~ 42
    let vs2 = Variant::from_real(3.14159); // !~ 3.14
    let vs3 = Variant::from_string("foolish"); // !~ foo

    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;

    // Interesting cases.
    let vs00 = Variant::from_bool(vs0.op_nomatch(&v0, &task)?);
    t.is(vs00.type_(), Type::Boolean, "untrue !~ true --> boolean");
    t.is(vs00.get_bool(), false, "untrue !~ true --> false");

    let vs01 = Variant::from_bool(vs0.op_nomatch(&v1, &task)?);
    t.is(vs01.type_(), Type::Boolean, "untrue !~ 42 --> boolean");
    t.is(vs01.get_bool(), true, "untrue !~ 42 --> true");

    let vs02 = Variant::from_bool(vs0.op_nomatch(&v2, &task)?);
    t.is(vs02.type_(), Type::Boolean, "untrue !~ 3.14 --> boolean");
    t.is(vs02.get_bool(), true, "untrue !~ 3.14 --> true");

    let vs03 = Variant::from_bool(vs0.op_nomatch(&v3, &task)?);
    t.is(vs03.type_(), Type::Boolean, "untrue !~ 'foo' --> boolean");
    t.is(vs03.get_bool(), true, "untrue !~ 'foo' --> true");

    let vs04 = Variant::from_bool(vs0.op_nomatch(&v4, &task)?);
    t.is(
        vs04.type_(),
        Type::Boolean,
        "untrue !~ 1234567890 --> boolean",
    );
    t.is(vs04.get_bool(), true, "untrue !~ 1234567890 --> true");

    let vs05 = Variant::from_bool(vs0.op_nomatch(&v5, &task)?);
    t.is(vs05.type_(), Type::Boolean, "untrue !~ 1200 --> boolean");
    t.is(vs05.get_bool(), true, "untrue !~ 1200 --> true");

    let vs10 = Variant::from_bool(vs1.op_nomatch(&v0, &task)?);
    t.is(vs10.type_(), Type::Boolean, "8421 !~ true --> boolean");
    t.is(vs10.get_bool(), true, "8421 !~ true --> true");

    let vs11 = Variant::from_bool(vs1.op_nomatch(&v1, &task)?);
    t.is(vs11.type_(), Type::Boolean, "8421 !~ 42 --> boolean");
    t.is(vs11.get_bool(), false, "8421 !~ 42 --> false");

    let vs12 = Variant::from_bool(vs1.op_nomatch(&v2, &task)?);
    t.is(vs12.type_(), Type::Boolean, "8421 !~ 3.14 --> boolean");
    t.is(vs12.get_bool(), true, "8421 !~ 3.14 --> true");

    let vs13 = Variant::from_bool(vs1.op_nomatch(&v3, &task)?);
    t.is(vs13.type_(), Type::Boolean, "8421 !~ 'foo' --> boolean");
    t.is(vs13.get_bool(), true, "8421 !~ 'foo' --> true");

    let vs14 = Variant::from_bool(vs1.op_nomatch(&v4, &task)?);
    t.is(
        vs14.type_(),
        Type::Boolean,
        "8421 !~ 1234567890 --> boolean",
    );
    t.is(vs14.get_bool(), true, "8421 !~ 1234567890 --> true");

    let vs15 = Variant::from_bool(vs1.op_nomatch(&v5, &task)?);
    t.is(vs15.type_(), Type::Boolean, "8421 !~ 1200 --> boolean");
    t.is(vs15.get_bool(), true, "8421 !~ 1200 --> true");

    let vs20 = Variant::from_bool(vs2.op_nomatch(&v0, &task)?);
    t.is(vs20.type_(), Type::Boolean, "3.14159 !~ true --> boolean");
    t.is(vs20.get_bool(), true, "3.14159 !~ true --> true");

    let vs21 = Variant::from_bool(vs2.op_nomatch(&v1, &task)?);
    t.is(vs21.type_(), Type::Boolean, "3.14159 !~ 42 --> boolean");
    t.is(vs21.get_bool(), true, "3.14159 !~ 42 --> true");

    let vs22 = Variant::from_bool(vs2.op_nomatch(&v2, &task)?);
    t.is(vs22.type_(), Type::Boolean, "3.14159 !~ 3.14 --> boolean");
    t.is(vs22.get_bool(), false, "3.14159 !~ 3.14 --> false");

    let vs23 = Variant::from_bool(vs2.op_nomatch(&v3, &task)?);
    t.is(vs23.type_(), Type::Boolean, "3.14159 !~ 'foo' --> boolean");
    t.is(vs23.get_bool(), true, "3.14159 !~ 'foo' --> true");

    let vs24 = Variant::from_bool(vs2.op_nomatch(&v4, &task)?);
    t.is(
        vs24.type_(),
        Type::Boolean,
        "3.14159 !~ 1234567890 --> boolean",
    );
    t.is(vs24.get_bool(), true, "3.14159 !~ 1234567890 --> true");

    let vs25 = Variant::from_bool(vs2.op_nomatch(&v5, &task)?);
    t.is(vs25.type_(), Type::Boolean, "3.14159 !~ 1200 --> boolean");
    t.is(vs25.get_bool(), true, "3.14159 !~ 1200 --> true");

    let vs30 = Variant::from_bool(vs3.op_nomatch(&v0, &task)?);
    t.is(vs30.type_(), Type::Boolean, "foolish !~ true --> boolean");
    t.is(vs30.get_bool(), true, "foolish !~ true --> true");

    let vs31 = Variant::from_bool(vs3.op_nomatch(&v1, &task)?);
    t.is(vs31.type_(), Type::Boolean, "foolish !~ 42 --> boolean");
    t.is(vs31.get_bool(), true, "foolish !~ 42 --> true");

    let vs32 = Variant::from_bool(vs3.op_nomatch(&v2, &task)?);
    t.is(vs32.type_(), Type::Boolean, "foolish !~ 3.14 --> boolean");
    t.is(vs32.get_bool(), true, "foolish !~ 3.14 --> true");

    let vs33 = Variant::from_bool(vs3.op_nomatch(&v3, &task)?);
    t.is(vs33.type_(), Type::Boolean, "foolish !~ 'foo' --> boolean");
    t.is(vs33.get_bool(), false, "foolish !~ 'foo' --> false");

    let vs34 = Variant::from_bool(vs3.op_nomatch(&v4, &task)?);
    t.is(
        vs34.type_(),
        Type::Boolean,
        "foolish !~ 1234567890 --> boolean",
    );
    t.is(vs34.get_bool(), true, "foolish !~ 1234567890 --> true");

    let vs35 = Variant::from_bool(vs3.op_nomatch(&v5, &task)?);
    t.is(vs35.type_(), Type::Boolean, "foolish !~ 1200 --> boolean");
    t.is(vs35.get_bool(), true, "foolish !~ 1200 --> true");

    // Exhaustive comparisons.
    let v00 = Variant::from_bool(v0.op_nomatch(&v0, &task)?);
    t.is(v00.type_(), Type::Boolean, "true !~ true --> boolean");
    t.is(v00.get_bool(), false, "true !~ true --> false");

    let v01 = Variant::from_bool(v0.op_nomatch(&v1, &task)?);
    t.is(v01.type_(), Type::Boolean, "true !~ 42 --> boolean");
    t.is(v01.get_bool(), true, "true !~ 42 --> true");

    let v02 = Variant::from_bool(v0.op_nomatch(&v2, &task)?);
    t.is(v02.type_(), Type::Boolean, "true !~ 3.14 --> boolean");
    t.is(v02.get_bool(), true, "true !~ 3.14 --> true");

    let v03 = Variant::from_bool(v0.op_nomatch(&v3, &task)?);
    t.is(v03.type_(), Type::Boolean, "true !~ 'foo' --> boolean");
    t.is(v03.get_bool(), true, "true !~ 'foo' --> true");

    let v04 = Variant::from_bool(v0.op_nomatch(&v4, &task)?);
    t.is(v04.type_(), Type::Boolean, "true !~ 1234567890 --> boolean");
    t.is(v04.get_bool(), true, "true !~ 1234567890 --> true");

    let v05 = Variant::from_bool(v0.op_nomatch(&v5, &task)?);
    t.is(v05.type_(), Type::Boolean, "true !~ 1200 --> boolean");
    t.is(v05.get_bool(), true, "true !~ 1200 --> true");

    let v10 = Variant::from_bool(v1.op_nomatch(&v0, &task)?);
    t.is(v10.type_(), Type::Boolean, "42 !~ true --> boolean");
    t.is(v10.get_bool(), true, "42 !~ true --> true");

    let v11 = Variant::from_bool(v1.op_nomatch(&v1, &task)?);
    t.is(v11.type_(), Type::Boolean, "42 !~ 42 --> boolean");
    t.is(v11.get_bool(), false, "42 !~ 42 --> false");

    let v12 = Variant::from_bool(v1.op_nomatch(&v2, &task)?);
    t.is(v12.type_(), Type::Boolean, "42 !~ 3.14 --> boolean");
    t.is(v12.get_bool(), true, "42 !~ 3.14 --> true");

    let v13 = Variant::from_bool(v1.op_nomatch(&v3, &task)?);
    t.is(v13.type_(), Type::Boolean, "42 !~ 'foo' --> boolean");
    t.is(v13.get_bool(), true, "42 !~ 'foo' --> true");

    // As in the C++, the two checks below test v04 rather than v14.
    let _v14 = Variant::from_bool(v1.op_nomatch(&v4, &task)?);
    t.is(v04.type_(), Type::Boolean, "42 !~ 1234567890 --> boolean");
    t.is(v04.get_bool(), true, "42 !~ 1234567890 --> true");

    let v15 = Variant::from_bool(v1.op_nomatch(&v5, &task)?);
    t.is(v15.type_(), Type::Boolean, "42 !~ 1200 --> boolean");
    t.is(v15.get_bool(), true, "42 !~ 1200 --> true");

    let v20 = Variant::from_bool(v2.op_nomatch(&v0, &task)?);
    t.is(v20.type_(), Type::Boolean, "3.14 !~ true --> boolean");
    t.is(v20.get_bool(), true, "3.14 !~ true --> true");

    let v21 = Variant::from_bool(v2.op_nomatch(&v1, &task)?);
    t.is(v21.type_(), Type::Boolean, "3.14 !~ 42 --> boolean");
    t.is(v21.get_bool(), true, "3.14 !~ 42 --> true");

    let v22 = Variant::from_bool(v2.op_nomatch(&v2, &task)?);
    t.is(v22.type_(), Type::Boolean, "3.14 !~ 3.14 --> boolean");
    t.is(v22.get_bool(), false, "3.14 !~ 3.14 --> false");

    let v23 = Variant::from_bool(v2.op_nomatch(&v3, &task)?);
    t.is(v23.type_(), Type::Boolean, "3.14 !~ 'foo' --> boolean");
    t.is(v23.get_bool(), true, "3.14 !~ 'foo' --> true");

    let v24 = Variant::from_bool(v2.op_nomatch(&v4, &task)?);
    t.is(v24.type_(), Type::Boolean, "3.14 !~ 1234567890 --> boolean");
    t.is(v24.get_bool(), true, "3.14 !~ 1234567890 --> true");

    let v25 = Variant::from_bool(v2.op_nomatch(&v5, &task)?);
    t.is(v25.type_(), Type::Boolean, "3.14 !~ 1200 --> boolean");
    t.is(v25.get_bool(), true, "3.14 !~ 1200 --> true");

    let v30 = Variant::from_bool(v3.op_nomatch(&v0, &task)?);
    t.is(v30.type_(), Type::Boolean, "'foo' !~ true --> boolean");
    t.is(v30.get_bool(), true, "'foo' !~ true --> true");

    let v31 = Variant::from_bool(v3.op_nomatch(&v1, &task)?);
    t.is(v31.type_(), Type::Boolean, "'foo' !~ 42 --> boolean");
    t.is(v31.get_bool(), true, "'foo' !~ 42 --> true");

    let v32 = Variant::from_bool(v3.op_nomatch(&v2, &task)?);
    t.is(v32.type_(), Type::Boolean, "'foo' !~ 3.14 --> boolean");
    t.is(v32.get_bool(), true, "'foo' !~ 3.14 --> true");

    let v33 = Variant::from_bool(v3.op_nomatch(&v3, &task)?);
    t.is(v33.type_(), Type::Boolean, "'foo' !~ 'foo' --> boolean");
    t.is(v33.get_bool(), false, "'foo' !~ 'foo' --> false");

    let v34 = Variant::from_bool(v3.op_nomatch(&v4, &task)?);
    t.is(
        v34.type_(),
        Type::Boolean,
        "'foo' !~ 1234567890 --> boolean",
    );
    t.is(v34.get_bool(), true, "'foo' !~ 1234567890 --> true");

    let v35 = Variant::from_bool(v3.op_nomatch(&v5, &task)?);
    t.is(v35.type_(), Type::Boolean, "'foo' !~ 1200 --> boolean");
    t.is(v35.get_bool(), true, "'foo' !~ 1200 --> true");

    let v40 = Variant::from_bool(v4.op_nomatch(&v0, &task)?);
    t.is(v40.type_(), Type::Boolean, "1234567890 !~ true --> boolean");
    t.is(v40.get_bool(), true, "1234567890 !~ true --> true");

    let v41 = Variant::from_bool(v4.op_nomatch(&v1, &task)?);
    t.is(v41.type_(), Type::Boolean, "1234567890 !~ 42 --> boolean");
    t.is(v41.get_bool(), true, "1234567890 !~ 42 --> true");

    let v42 = Variant::from_bool(v4.op_nomatch(&v2, &task)?);
    t.is(v42.type_(), Type::Boolean, "1234567890 !~ 3.14 --> boolean");
    t.is(v42.get_bool(), true, "1234567890 !~ 3.14 --> true");

    let v43 = Variant::from_bool(v4.op_nomatch(&v3, &task)?);
    t.is(
        v43.type_(),
        Type::Boolean,
        "1234567890 !~ 'foo' --> boolean",
    );
    t.is(v43.get_bool(), true, "1234567890 !~ 'foo' --> true");

    let v44 = Variant::from_bool(v4.op_nomatch(&v4, &task)?);
    t.is(
        v44.type_(),
        Type::Boolean,
        "1234567890 !~ 1234567890 --> boolean",
    );
    t.is(v44.get_bool(), false, "1234567890 !~ 1234567890 --> false");

    let v45 = Variant::from_bool(v4.op_nomatch(&v5, &task)?);
    t.is(v45.type_(), Type::Boolean, "1234567890 !~ 1200 --> boolean");
    t.is(v45.get_bool(), true, "1234567890 !~ 1200 --> true");

    let v50 = Variant::from_bool(v5.op_nomatch(&v0, &task)?);
    t.is(v50.type_(), Type::Boolean, "1200 !~ true --> boolean");
    t.is(v50.get_bool(), true, "1200 !~ true --> true");

    let v51 = Variant::from_bool(v5.op_nomatch(&v1, &task)?);
    t.is(v51.type_(), Type::Boolean, "1200 !~ 42 --> boolean");
    t.is(v51.get_bool(), true, "1200 !~ 42 --> true");

    let v52 = Variant::from_bool(v5.op_nomatch(&v2, &task)?);
    t.is(v52.type_(), Type::Boolean, "1200 !~ 3.14 --> boolean");
    t.is(v52.get_bool(), true, "1200 !~ 3.14 --> true");

    let v53 = Variant::from_bool(v5.op_nomatch(&v3, &task)?);
    t.is(v53.type_(), Type::Boolean, "1200 !~ 'foo' --> boolean");
    t.is(v53.get_bool(), true, "1200 !~ 'foo' --> true");

    // As in the C++, the two checks below test v04 rather than v54.
    let _v54 = Variant::from_bool(v5.op_nomatch(&v4, &task)?);
    t.is(v04.type_(), Type::Boolean, "1200 !~ 1234567890 --> boolean");
    t.is(v04.get_bool(), true, "1200 !~ 1234567890 --> true");

    let v55 = Variant::from_bool(v5.op_nomatch(&v5, &task)?);
    t.is(v55.type_(), Type::Boolean, "1200 !~ 1200 --> boolean");
    t.is(v55.get_bool(), false, "1200 !~ 1200 --> false");

    Ok(())
}

#[test]
fn variant_nomatch_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
