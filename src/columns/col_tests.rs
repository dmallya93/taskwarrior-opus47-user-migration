//! Port of Taskwarrior 3.5.0 `test/col_test.cpp`: `ColumnID::measure` for
//! IDs of one to six digits.
//!
//! The C++ `UnitTest` harness is mirrored by [`UnitTest`]: each `test.is` of
//! col_test.cpp becomes one call with the identical message, and the run
//! checks the number of executed assertions against the C++ file's (12,
//! equal to its `UnitTest test (12)` plan).
use crate::columns::col_id::ColumnID;
use crate::columns::column::{Column, FakeRenderEnv};
use crate::task::Task;
use crate::test_util::{env_lock, EnvGuard};

/// col_test.cpp: `UnitTest test (12)`.
const COL_TEST_ASSERTIONS: usize = 12;

/// The C++ `UnitTest` (test/test.cpp), recording instead of printing.
struct UnitTest {
    counter: usize,
    failures: Vec<String>,
}

impl UnitTest {
    fn new() -> Self {
        UnitTest { counter: 0, failures: Vec::new() }
    }

    /// C++ `UnitTest::is (int actual, int expected, const std::string& name)`.
    fn is(&mut self, actual: i32, expected: i32, name: &str) {
        self.counter += 1;
        if actual != expected {
            self.failures.push(format!(
                "not ok {} - {}\n# expected: {}\n#      got: {}",
                self.counter, name, expected, actual
            ));
        }
    }

    /// Fails on any failed assertion, or when the number of executed
    /// assertions differs from the C++ file's.
    fn finish(self, expected: usize) {
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
        assert_eq!(self.counter, expected, "assertion count differs from the C++ test");
    }
}

/// col_test.cpp `TEST_NAME`, line by line.
#[test]
fn col_test() {
    let _lock = env_lock();
    let _env = EnvGuard::save(&["TASKDATA", "TASKRC"]);
    let mut test = UnitTest::new();

    // Ensure environment has no influence.
    std::env::remove_var("TASKDATA");
    std::env::remove_var("TASKRC");

    let column_id = ColumnID::new();
    let mut env = FakeRenderEnv::with_config(&[]);

    let mut t1 = Task::new();
    let cases: [(i32, i32); 6] = [(3, 1), (33, 2), (333, 3), (3333, 4), (33333, 5), (333333, 6)];
    for (id, width) in cases {
        t1.id = id;
        let (minimum, maximum) = column_id.measure(&mut t1, &mut env).unwrap();
        test.is(minimum as i32, width, &format!("id:{id} --> ColID::measure minimum {width}"));
        test.is(maximum as i32, width, &format!("id:{id} --> ColID::measure maximum {width}"));
    }

    test.finish(COL_TEST_ASSERTIONS);
}
