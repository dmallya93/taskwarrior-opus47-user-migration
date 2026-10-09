//! Port of Taskwarrior `test/variant_math_test.cpp` (1 assertion, as planned by its `UnitTest t (1)`), with the C++ messages.

use super::*;

const EPSILON: f64 = 0.001;

/// Assertions executed by variant_math_test.cpp.
const PLANNED: usize = 1;

fn suite(t: &mut UnitTest) -> Result<()> {
    let mut v0 = Variant::from_real(10.0);
    v0.sqrt()?;
    t.is_tol(v0.get_real(), 3.1622, EPSILON, "math sqrt 10 -> 3.1622");

    Ok(())
}

#[test]
fn variant_math_test() {
    let _u = Upstream::setup(None);
    let mut t = UnitTest::new();

    if let Err(e) = suite(&mut t) {
        t.diag(&e.to_string());
    }

    t.check(PLANNED);
}
