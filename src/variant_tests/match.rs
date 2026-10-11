//! Port of Taskwarrior `test/variant_match_test.cpp` (120 assertions, as planned by its
//! `UnitTest t (120)`), with the C++ messages.
//!
//! The C++ `Task task;` is the placeholder [`Task`] (no annotations, no
//! tags); the annotation scan is covered by `annotations_and_tags` in
//! `match.rs`.

use super::*;

/// Assertions executed by variant_match_test.cpp.
const PLANNED: usize = 120;

#[allow(clippy::approx_constant)] // The C++ tests use the literals 3.14 and 3.14159.
fn suite(t: &mut UnitTest) -> Result<()> {
    let task = Task::default();

    let vs0 = Variant::from_string("untrue"); // ~ true
    let vs1 = Variant::from_int(8421); // ~ 42
    let vs2 = Variant::from_real(3.14159); // ~ 3.14
    let vs3 = Variant::from_string("foolish"); // ~ foo

    let v0 = Variant::from_bool(true);
    let v1 = Variant::from_int(42);
    let v2 = Variant::from_real(3.14);
    let v3 = Variant::from_string("foo");
    let v4 = Variant::from_time(1234567890, Type::Date)?;
    let v5 = Variant::from_time(1200, Type::Duration)?;

    // Interesting cases.
    let vs00 = Variant::from_bool(vs0.op_match(&v0, &task)?);
    t.is(vs00.type_(), Type::Boolean, "untrue ~ true --> boolean");
    t.is(vs00.get_bool(), true, "untrue ~ true --> true");

    let vs01 = Variant::from_bool(vs0.op_match(&v1, &task)?);
    t.is(vs01.type_(), Type::Boolean, "untrue ~ 42 --> boolean");
    t.is(vs01.get_bool(), false, "untrue ~ 42 --> false");

    let vs02 = Variant::from_bool(vs0.op_match(&v2, &task)?);
    t.is(vs02.type_(), Type::Boolean, "untrue ~ 3.14 --> boolean");
    t.is(vs02.get_bool(), false, "untrue ~ 3.14 --> false");

    let vs03 = Variant::from_bool(vs0.op_match(&v3, &task)?);
    t.is(vs03.type_(), Type::Boolean, "untrue ~ 'foo' --> boolean");
    t.is(vs03.get_bool(), false, "untrue ~ 'foo' --> false");

    let vs04 = Variant::from_bool(vs0.op_match(&v4, &task)?);
    t.is(
        vs04.type_(),
        Type::Boolean,
        "untrue ~ 1234567890 --> boolean",
    );
    t.is(vs04.get_bool(), false, "untrue ~ 1234567890 --> false");

    let vs05 = Variant::from_bool(vs0.op_match(&v5, &task)?);
    t.is(vs05.type_(), Type::Boolean, "untrue ~ 1200 --> boolean");
    t.is(vs05.get_bool(), false, "untrue ~ 1200 --> false");

    let vs10 = Variant::from_bool(vs1.op_match(&v0, &task)?);
    t.is(vs10.type_(), Type::Boolean, "8421 ~ true --> boolean");
    t.is(vs10.get_bool(), false, "8421 ~ true --> false");

    let vs11 = Variant::from_bool(vs1.op_match(&v1, &task)?);
    t.is(vs11.type_(), Type::Boolean, "8421 ~ 42 --> boolean");
    t.is(vs11.get_bool(), true, "8421 ~ 42 --> true");

    let vs12 = Variant::from_bool(vs1.op_match(&v2, &task)?);
    t.is(vs12.type_(), Type::Boolean, "8421 ~ 3.14 --> boolean");
    t.is(vs12.get_bool(), false, "8421 ~ 3.14 --> false");

    let vs13 = Variant::from_bool(vs1.op_match(&v3, &task)?);
    t.is(vs13.type_(), Type::Boolean, "8421 ~ 'foo' --> boolean");
    t.is(vs13.get_bool(), false, "8421 ~ 'foo' --> false");

    let vs14 = Variant::from_bool(vs1.op_match(&v4, &task)?);
    t.is(vs14.type_(), Type::Boolean, "8421 ~ 1234567890 --> boolean");
    t.is(vs14.get_bool(), false, "8421 ~ 1234567890 --> false");

    let vs15 = Variant::from_bool(vs1.op_match(&v5, &task)?);
    t.is(vs15.type_(), Type::Boolean, "8421 ~ 1200 --> boolean");
    t.is(vs15.get_bool(), false, "8421 ~ 1200 --> false");

    let vs20 = Variant::from_bool(vs2.op_match(&v0, &task)?);
    t.is(vs20.type_(), Type::Boolean, "3.14159 ~ true --> boolean");
    t.is(vs20.get_bool(), false, "3.14159 ~ true --> false");

    let vs21 = Variant::from_bool(vs2.op_match(&v1, &task)?);
    t.is(vs21.type_(), Type::Boolean, "3.14159 ~ 42 --> boolean");
    t.is(vs21.get_bool(), false, "3.14159 ~ 42 --> false");

    let vs22 = Variant::from_bool(vs2.op_match(&v2, &task)?);
    t.is(vs22.type_(), Type::Boolean, "3.14159 ~ 3.14 --> boolean");
    t.is(vs22.get_bool(), true, "3.14159 ~ 3.14 --> true");

    let vs23 = Variant::from_bool(vs2.op_match(&v3, &task)?);
    t.is(vs23.type_(), Type::Boolean, "3.14159 ~ 'foo' --> boolean");
    t.is(vs23.get_bool(), false, "3.14159 ~ 'foo' --> false");

    let vs24 = Variant::from_bool(vs2.op_match(&v4, &task)?);
    t.is(
        vs24.type_(),
        Type::Boolean,
        "3.14159 ~ 1234567890 --> boolean",
    );
    t.is(vs24.get_bool(), false, "3.14159 ~ 1234567890 --> false");

    let vs25 = Variant::from_bool(vs2.op_match(&v5, &task)?);
    t.is(vs25.type_(), Type::Boolean, "3.14159 ~ 1200 --> boolean");
    t.is(vs25.get_bool(), false, "3.14159 ~ 1200 --> false");

    let vs30 = Variant::from_bool(vs3.op_match(&v0, &task)?);
    t.is(vs30.type_(), Type::Boolean, "foolish ~ true --> boolean");
    t.is(vs30.get_bool(), false, "foolish ~ true --> false");

    let vs31 = Variant::from_bool(vs3.op_match(&v1, &task)?);
    t.is(vs31.type_(), Type::Boolean, "foolish ~ 42 --> boolean");
    t.is(vs31.get_bool(), false, "foolish ~ 42 --> false");

    let vs32 = Variant::from_bool(vs3.op_match(&v2, &task)?);
    t.is(vs32.type_(), Type::Boolean, "foolish ~ 3.14 --> boolean");
    t.is(vs32.get_bool(), false, "foolish ~ 3.14 --> false");

    let vs33 = Variant::from_bool(vs3.op_match(&v3, &task)?);
    t.is(vs33.type_(), Type::Boolean, "foolish ~ 'foo' --> boolean");
    t.is(vs33.get_bool(), true, "foolish ~ 'foo' --> true");

    let vs34 = Variant::from_bool(vs3.op_match(&v4, &task)?);
    t.is(
        vs34.type_(),
        Type::Boolean,
        "foolish ~ 1234567890 --> boolean",
    );
    t.is(vs34.get_bool(), false, "foolish ~ 1234567890 --> false");

    let vs35 = Variant::from_bool(vs3.op_match(&v5, &task)?);
    t.is(vs35.type_(), Type::Boolean, "foolish ~ 1200 --> boolean");
    t.is(vs35.get_bool(), false, "foolish ~ 1200 --> false");

    // Exhaustive comparisons.
    let v00 = Variant::from_bool(v0.op_match(&v0, &task)?);
    t.is(v00.type_(), Type::Boolean, "true ~ true --> boolean");
    t.is(v00.get_bool(), true, "true ~ true --> true");

    let v01 = Variant::from_bool(v0.op_match(&v1, &task)?);
    t.is(v01.type_(), Type::Boolean, "true ~ 42 --> boolean");
    t.is(v01.get_bool(), false, "true ~ 42 --> false");

    let v02 = Variant::from_bool(v0.op_match(&v2, &task)?);
    t.is(v02.type_(), Type::Boolean, "true ~ 3.14 --> boolean");
    t.is(v02.get_bool(), false, "true ~ 3.14 --> false");

    let v03 = Variant::from_bool(v0.op_match(&v3, &task)?);
    t.is(v03.type_(), Type::Boolean, "true ~ 'foo' --> boolean");
    t.is(v03.get_bool(), false, "true ~ 'foo' --> false");

    let v04 = Variant::from_bool(v0.op_match(&v4, &task)?);
    t.is(v04.type_(), Type::Boolean, "true ~ 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "true ~ 1234567890 --> false");

    let v05 = Variant::from_bool(v0.op_match(&v5, &task)?);
    t.is(v05.type_(), Type::Boolean, "true ~ 1200 --> boolean");
    t.is(v05.get_bool(), false, "true ~ 1200 --> false");

    let v10 = Variant::from_bool(v1.op_match(&v0, &task)?);
    t.is(v10.type_(), Type::Boolean, "42 ~ true --> boolean");
    t.is(v10.get_bool(), false, "42 ~ true --> false");

    let v11 = Variant::from_bool(v1.op_match(&v1, &task)?);
    t.is(v11.type_(), Type::Boolean, "42 ~ 42 --> boolean");
    t.is(v11.get_bool(), true, "42 ~ 42 --> true");

    let v12 = Variant::from_bool(v1.op_match(&v2, &task)?);
    t.is(v12.type_(), Type::Boolean, "42 ~ 3.14 --> boolean");
    t.is(v12.get_bool(), false, "42 ~ 3.14 --> false");

    let v13 = Variant::from_bool(v1.op_match(&v3, &task)?);
    t.is(v13.type_(), Type::Boolean, "42 ~ 'foo' --> boolean");
    t.is(v13.get_bool(), false, "42 ~ 'foo' --> false");

    // As in the C++, the two checks below test v04 rather than v14.
    let _v14 = Variant::from_bool(v1.op_match(&v4, &task)?);
    t.is(v04.type_(), Type::Boolean, "42 ~ 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "42 ~ 1234567890 --> false");

    let v15 = Variant::from_bool(v1.op_match(&v5, &task)?);
    t.is(v15.type_(), Type::Boolean, "42 ~ 1200 --> boolean");
    t.is(v15.get_bool(), false, "42 ~ 1200 --> false");

    let v20 = Variant::from_bool(v2.op_match(&v0, &task)?);
    t.is(v20.type_(), Type::Boolean, "3.14 ~ true --> boolean");
    t.is(v20.get_bool(), false, "3.14 ~ true --> false");

    let v21 = Variant::from_bool(v2.op_match(&v1, &task)?);
    t.is(v21.type_(), Type::Boolean, "3.14 ~ 42 --> boolean");
    t.is(v21.get_bool(), false, "3.14 ~ 42 --> false");

    let v22 = Variant::from_bool(v2.op_match(&v2, &task)?);
    t.is(v22.type_(), Type::Boolean, "3.14 ~ 3.14 --> boolean");
    t.is(v22.get_bool(), true, "3.14 ~ 3.14 --> true");

    let v23 = Variant::from_bool(v2.op_match(&v3, &task)?);
    t.is(v23.type_(), Type::Boolean, "3.14 ~ 'foo' --> boolean");
    t.is(v23.get_bool(), false, "3.14 ~ 'foo' --> false");

    let v24 = Variant::from_bool(v2.op_match(&v4, &task)?);
    t.is(v24.type_(), Type::Boolean, "3.14 ~ 1234567890 --> boolean");
    t.is(v24.get_bool(), false, "3.14 ~ 1234567890 --> false");

    let v25 = Variant::from_bool(v2.op_match(&v5, &task)?);
    t.is(v25.type_(), Type::Boolean, "3.14 ~ 1200 --> boolean");
    t.is(v25.get_bool(), false, "3.14 ~ 1200 --> false");

    let v30 = Variant::from_bool(v3.op_match(&v0, &task)?);
    t.is(v30.type_(), Type::Boolean, "'foo' ~ true --> boolean");
    t.is(v30.get_bool(), false, "'foo' ~ true --> false");

    let v31 = Variant::from_bool(v3.op_match(&v1, &task)?);
    t.is(v31.type_(), Type::Boolean, "'foo' ~ 42 --> boolean");
    t.is(v31.get_bool(), false, "'foo' ~ 42 --> false");

    let v32 = Variant::from_bool(v3.op_match(&v2, &task)?);
    t.is(v32.type_(), Type::Boolean, "'foo' ~ 3.14 --> boolean");
    t.is(v32.get_bool(), false, "'foo' ~ 3.14 --> false");

    let v33 = Variant::from_bool(v3.op_match(&v3, &task)?);
    t.is(v33.type_(), Type::Boolean, "'foo' ~ 'foo' --> boolean");
    t.is(v33.get_bool(), true, "'foo' ~ 'foo' --> true");

    let v34 = Variant::from_bool(v3.op_match(&v4, &task)?);
    t.is(v34.type_(), Type::Boolean, "'foo' ~ 1234567890 --> boolean");
    t.is(v34.get_bool(), false, "'foo' ~ 1234567890 --> false");

    let v35 = Variant::from_bool(v3.op_match(&v5, &task)?);
    t.is(v35.type_(), Type::Boolean, "'foo' ~ 1200 --> boolean");
    t.is(v35.get_bool(), false, "'foo' ~ 1200 --> false");

    let v40 = Variant::from_bool(v4.op_match(&v0, &task)?);
    t.is(v40.type_(), Type::Boolean, "1234567890 ~ true --> boolean");
    t.is(v40.get_bool(), false, "1234567890 ~ true --> false");

    let v41 = Variant::from_bool(v4.op_match(&v1, &task)?);
    t.is(v41.type_(), Type::Boolean, "1234567890 ~ 42 --> boolean");
    t.is(v41.get_bool(), false, "1234567890 ~ 42 --> false");

    let v42 = Variant::from_bool(v4.op_match(&v2, &task)?);
    t.is(v42.type_(), Type::Boolean, "1234567890 ~ 3.14 --> boolean");
    t.is(v42.get_bool(), false, "1234567890 ~ 3.14 --> false");

    let v43 = Variant::from_bool(v4.op_match(&v3, &task)?);
    t.is(v43.type_(), Type::Boolean, "1234567890 ~ 'foo' --> boolean");
    t.is(v43.get_bool(), false, "1234567890 ~ 'foo' --> false");

    let v44 = Variant::from_bool(v4.op_match(&v4, &task)?);
    t.is(
        v44.type_(),
        Type::Boolean,
        "1234567890 ~ 1234567890 --> boolean",
    );
    t.is(v44.get_bool(), true, "1234567890 ~ 1234567890 --> true");

    let v45 = Variant::from_bool(v4.op_match(&v5, &task)?);
    t.is(v45.type_(), Type::Boolean, "1234567890 ~ 1200 --> boolean");
    t.is(v45.get_bool(), false, "1234567890 ~ 1200 --> false");

    let v50 = Variant::from_bool(v5.op_match(&v0, &task)?);
    t.is(v50.type_(), Type::Boolean, "1200 ~ true --> boolean");
    t.is(v50.get_bool(), false, "1200 ~ true --> false");

    let v51 = Variant::from_bool(v5.op_match(&v1, &task)?);
    t.is(v51.type_(), Type::Boolean, "1200 ~ 42 --> boolean");
    t.is(v51.get_bool(), false, "1200 ~ 42 --> false");

    let v52 = Variant::from_bool(v5.op_match(&v2, &task)?);
    t.is(v52.type_(), Type::Boolean, "1200 ~ 3.14 --> boolean");
    t.is(v52.get_bool(), false, "1200 ~ 3.14 --> false");

    let v53 = Variant::from_bool(v5.op_match(&v3, &task)?);
    t.is(v53.type_(), Type::Boolean, "1200 ~ 'foo' --> boolean");
    t.is(v53.get_bool(), false, "1200 ~ 'foo' --> false");

    // As in the C++, the two checks below test v04 rather than v54.
    let _v54 = Variant::from_bool(v5.op_match(&v4, &task)?);
    t.is(v04.type_(), Type::Boolean, "1200 ~ 1234567890 --> boolean");
    t.is(v04.get_bool(), false, "1200 ~ 1234567890 --> false");

    let v55 = Variant::from_bool(v5.op_match(&v5, &task)?);
    t.is(v55.type_(), Type::Boolean, "1200 ~ 1200 --> boolean");
    t.is(v55.get_bool(), true, "1200 ~ 1200 --> true");

    Ok(())
}

#[test]
fn variant_match_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}

/// A test-only [`VariantTask`] with annotations and tags (the placeholder
/// [`Task`] has neither until milestone 4).
struct MockTask {
    annotations: BTreeMap<String, String>,
    tags: Vec<String>,
}

impl MockTask {
    fn new(annotations: &[&str], tags: &[&str]) -> Self {
        MockTask {
            annotations: annotations
                .iter()
                .enumerate()
                .map(|(i, a)| (format!("annotation_{}", 1234567890 + i), a.to_string()))
                .collect(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }
}

impl VariantTask for MockTask {
    fn get_annotations(&self) -> BTreeMap<String, String> {
        self.annotations.clone()
    }

    fn has_tag(&self, tag: &str) -> crate::Result<bool> {
        Ok(self.tags.iter().any(|t| t == tag))
    }
}

fn description(text: &str) -> Variant {
    let mut v = Variant::from_string(text);
    v.set_source("description");
    v
}

#[test]
fn annotations_and_tags() {
    let _u = Upstream::setup(None);
    let task = MockTask::new(&["first note", "Second NOTE here"], &["home", "next"]);
    let s = Variant::from_string;

    for regex in [true, false] {
        Variant::set_search_using_regex(regex);
        Variant::set_search_case_sensitive(true);

        // The annotations are searched only for a "description" left operand.
        assert!(
            description("plain").op_match(&s("first"), &task).unwrap(),
            "regex={regex}"
        );
        assert!(
            description("plain").op_match(&s("NOTE"), &task).unwrap(),
            "regex={regex}"
        );
        assert!(
            !description("plain").op_match(&s("absent"), &task).unwrap(),
            "regex={regex}"
        );
        assert!(
            description("plain")
                .op_nomatch(&s("absent"), &task)
                .unwrap(),
            "regex={regex}"
        );
        assert!(
            !s("plain").op_match(&s("first"), &task).unwrap(),
            "regex={regex}"
        );
        assert!(
            s("plain").op_nomatch(&s("first"), &task).unwrap(),
            "regex={regex}"
        );
        let mut project = Variant::from_string("plain");
        project.set_source("project");
        assert!(
            !project.op_match(&s("first"), &task).unwrap(),
            "regex={regex}"
        );

        // Case sensitivity applies to the annotations too.
        assert!(
            !description("plain").op_match(&s("second"), &task).unwrap(),
            "regex={regex}"
        );
        Variant::set_search_case_sensitive(false);
        assert!(
            description("plain").op_match(&s("second"), &task).unwrap(),
            "regex={regex}"
        );
        assert!(
            description("PLAIN").op_match(&s("'plain'"), &task).unwrap(),
            "regex={regex}"
        );
    }

    // Regex syntax applies to the annotations with searchUsingRegex only.
    Variant::set_search_case_sensitive(true);
    Variant::set_search_using_regex(true);
    assert!(description("plain")
        .op_match(&s("^Second.*here$"), &task)
        .unwrap());
    Variant::set_search_using_regex(false);
    assert!(!description("plain").op_match(&s("f.rst"), &task).unwrap());

    // The placeholder Task has no annotations.
    Variant::set_search_using_regex(true);
    assert!(!description("plain")
        .op_match(&s("first"), &Task::default())
        .unwrap());

    // hastag / notag: the right operand is cast to string and dequoted.
    let v = Variant::default();
    assert!(v.op_hastag(&s("home"), &task).unwrap());
    assert!(v.op_hastag(&s("'next'"), &task).unwrap());
    assert!(!v.op_hastag(&s("work"), &task).unwrap());
    assert!(!v.op_notag(&s("home"), &task).unwrap());
    assert!(v.op_notag(&s("work"), &task).unwrap());
    assert!(!v.op_hastag(&Variant::from_int(1), &task).unwrap());
    assert!(MockTask::new(&[], &["42"]).has_tag("42").unwrap());
    assert!(v
        .op_hastag(&Variant::from_int(42), &MockTask::new(&[], &["42"]))
        .unwrap());
    assert!(!v.op_hastag(&s("home"), &Task::default()).unwrap());
    assert!(v.op_notag(&s("home"), &Task::default()).unwrap());
}

#[test]
fn substring_search_anchors() {
    let _u = Upstream::setup(None);
    Variant::set_search_using_regex(false);
    let task = Task::default();
    let s = Variant::from_string;
    let m = |left: &str, pattern: &str| s(left).op_match(&s(pattern), &task).unwrap();

    assert!(m("foobar", "oba"));
    assert!(m("foobar", "^foo"));
    assert!(!m("foobar", "^bar"));
    assert!(m("foobar", "bar$"));
    assert!(!m("foobar", "foo$"));
    assert!(m("foobar", "^foobar"));
    assert!(m("foobar", "foobar$"));
    // Regex metacharacters are literal.
    assert!(!m("foobar", "f.o"));
    assert!(m("f.o", "f.o"));
    // The pattern is dequoted twice: once as a string Variant, once as the
    // pattern.
    assert!(m("foobar", "'\"bar\"'"));
    // An empty pattern is found at 0.
    assert!(m("foobar", ""));
    assert!(m("", ""));
    // C++ size_t arithmetic: a not-found '$' search matches when the pattern
    // is two bytes longer than the left operand (npos == npos).
    assert!(m("ab", "wxy$"));
    assert!(!m("ab", "wx$"));
    assert!(!m("ab", "wxyz$"));
    // Case-insensitive search.
    assert!(!m("FooBar", "^foo"));
    Variant::set_search_case_sensitive(false);
    assert!(m("FooBar", "^foo"));
    assert!(m("FooBar", "BAR$"));
    assert!(m("FooBar", "oob"));
}

#[test]
fn regex_search() {
    let _u = Upstream::setup(None);
    Variant::set_search_using_regex(true);
    let task = Task::default();
    let s = Variant::from_string;
    let m = |left: &str, pattern: &str| s(left).op_match(&s(pattern), &task).unwrap();

    assert!(m("foobar", "f.o"));
    assert!(m("foobar", "^foo"));
    assert!(!m("foobar", "^bar"));
    assert!(m("foobar", "o+b"));
    assert!(m("foobar", ""));
    assert!(!m("FooBar", "foo"));
    Variant::set_search_case_sensitive(false);
    assert!(m("FooBar", "foo"));
}

/// C++ `RX` throws `std::runtime_error` for an invalid pattern; only
/// `catch (...)` catches it ("Unknown error. Please report.", status 3 in
/// `Context::run`, -2 in `main`, see their tests). It must arrive as
/// [`TwError::Runtime`], also when only the annotation scan would follow.
#[test]
fn invalid_regex_is_a_runtime_error() {
    let _u = Upstream::setup(None);
    Variant::set_search_using_regex(true);
    let task = MockTask::new(&["note"], &[]);

    for pattern in ["(", "'('", "[z-a]"] {
        let err = description("x")
            .op_match(&Variant::from_string(pattern), &task)
            .unwrap_err();
        assert!(matches!(err, TwError::Runtime(_)), "{pattern}: {err:?}");
        let err = Variant::from_int(1)
            .op_nomatch(&Variant::from_string(pattern), &task)
            .unwrap_err();
        assert!(matches!(err, TwError::Runtime(_)), "{pattern}: {err:?}");
    }
    let err = description("x")
        .op_match(&Variant::from_string("("), &task)
        .unwrap_err();
    assert_eq!(err.to_string(), "Parenthesis is not closed.");

    // Without regex search the same pattern is a plain substring.
    Variant::set_search_using_regex(false);
    assert!(Variant::from_string("a(b")
        .op_match(&Variant::from_string("("), &task)
        .unwrap());
}
