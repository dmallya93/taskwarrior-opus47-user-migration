//! Rust translation of `src/Operation.cpp` (declarations in `src/Operation.h`).
//!
//! C++ `Operation` wraps a `const tc::Operation*` into the cxx bridge's
//! `rust::Vec<tc::Operation>`; the getters are the bridge's (`src/taskchampion-cpp/src/lib.rs`)
//! mapping onto [`taskchampion::Operation`]. Here [`Operation`] owns a clone
//! of the taskchampion operation, so a `Vec<Operation>` does not borrow the
//! `Operations` it came from (the undo command in milestone 7 displays the
//! operations and then hands the original `Operations` to
//! `commit_reversed_operations`).
//!
//! As in the bridge, calling a getter that does not apply to the operation's
//! kind (e.g. [`Operation::get_property`] on a `Create`) panics with the
//! bridge's message; C++ would abort the process at that point.

use std::cmp::Ordering;

use taskchampion::Operation as TcOperation;
use taskchampion::Operations as TcOperations;

/// Representation of a TaskChampion operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    op: TcOperation,
}

impl From<TcOperation> for Operation {
    fn from(op: TcOperation) -> Self {
        Operation { op }
    }
}

impl Operation {
    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Operation (const tc::Operation&)`.
    pub fn new(op: &TcOperation) -> Self {
        Operation { op: op.clone() }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Operation::operations`: create a vector of Operations given the
    /// result of `Replica::get_undo_operations` or
    /// `Replica::get_task_operations`, in the same order.
    pub fn operations(operations: &TcOperations) -> Vec<Operation> {
        operations.iter().map(Operation::new).collect()
    }

    /// The wrapped taskchampion operation.
    pub fn inner(&self) -> &TcOperation {
        &self.op
    }

    /// Check if this is a Create operation.
    pub fn is_create(&self) -> bool {
        matches!(&self.op, TcOperation::Create { .. })
    }

    /// Check if this is an Update operation.
    pub fn is_update(&self) -> bool {
        matches!(&self.op, TcOperation::Update { .. })
    }

    /// Check if this is a Delete operation.
    pub fn is_delete(&self) -> bool {
        matches!(&self.op, TcOperation::Delete { .. })
    }

    /// Check if this is an UndoPoint operation.
    pub fn is_undo_point(&self) -> bool {
        matches!(&self.op, TcOperation::UndoPoint)
    }

    /// The operation's uuid, lowercase hyphenated (C++
    /// `op->get_uuid ().to_string ()`).
    ///
    /// Only valid for create, update, and delete operations.
    pub fn get_uuid(&self) -> String {
        match &self.op {
            TcOperation::Create { uuid, .. } => uuid,
            TcOperation::Update { uuid, .. } => uuid,
            TcOperation::Delete { uuid, .. } => uuid,
            _ => panic!("operation has no uuid"),
        }
        .to_string()
    }

    /// The `old_task` of this delete operation as (property, value) pairs.
    ///
    /// Only valid for delete operations.
    ///
    /// Deviation: the bridge returns the pairs in `HashMap` iteration order,
    /// which is unspecified; here they are sorted by property name so the
    /// result is deterministic. (No C++ caller depends on the order.)
    pub fn get_old_task(&self) -> Vec<(String, String)> {
        match &self.op {
            TcOperation::Delete { old_task, .. } => {
                let mut pairs: Vec<(String, String)> = old_task
                    .iter()
                    .map(|(p, v)| (p.clone(), v.clone()))
                    .collect();
                pairs.sort();
                pairs
            }
            _ => panic!("operation is not a delete"),
        }
    }

    /// The `property` of this update operation.
    ///
    /// Only valid for update operations.
    pub fn get_property(&self) -> String {
        match &self.op {
            TcOperation::Update { property, .. } => property.clone(),
            _ => panic!("operation is not an update"),
        }
    }

    /// The `value` of this update operation (`None` when the property is
    /// removed).
    ///
    /// Only valid for update operations.
    pub fn get_value(&self) -> Option<String> {
        match &self.op {
            TcOperation::Update { value, .. } => value.clone(),
            _ => panic!("operation is not an update"),
        }
    }

    /// The `old_value` of this update operation (`None` when the property was
    /// not set).
    ///
    /// Only valid for update operations.
    pub fn get_old_value(&self) -> Option<String> {
        match &self.op {
            TcOperation::Update { old_value, .. } => old_value.clone(),
            _ => panic!("operation is not an update"),
        }
    }

    /// The timestamp of this update operation, in seconds since the epoch
    /// (`time_t`).
    ///
    /// Only valid for update operations.
    pub fn get_timestamp(&self) -> i64 {
        match &self.op {
            TcOperation::Update { timestamp, .. } => timestamp.timestamp(),
            _ => panic!("operation is not an update"),
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Operation::operator<`. Defines a partial order on Operations:
    ///  - Create < Update < Delete < UndoPoint
    ///  - Given two updates, sort by timestamp
    ///
    /// Two operations of the same kind (other than updates with different
    /// timestamps) are not less than each other.
    ///
    /// Note: ported exactly, including a C++ inconsistency with the comment
    /// above: an UndoPoint is less than every non-UndoPoint
    /// (`return !other.is_undo_point ()`), while every non-UndoPoint is also
    /// less than an UndoPoint. The only C++ sort (`CmdInfo`, over
    /// `get_task_operations`) never sees UndoPoints, since they carry no uuid.
    pub fn lt(&self, other: &Self) -> bool {
        if self.is_create() {
            !other.is_create()
        } else if self.is_update() {
            if other.is_create() {
                false
            } else if other.is_update() {
                self.get_timestamp() < other.get_timestamp()
            } else {
                true
            }
        } else if self.is_delete() {
            !(other.is_create() || other.is_update() || other.is_delete())
        } else if self.is_undo_point() {
            !other.is_undo_point()
        } else {
            false // not reachable
        }
    }

    /// The ordering induced by [`Operation::lt`] (a strict weak order), for
    /// `slice::sort_by` in place of C++ `std::sort`: `Less` if `self < other`,
    /// `Greater` if `other < self`, otherwise `Equal`.
    ///
    /// Note: `std::sort` is not stable while `sort_by` is; elements comparing
    /// `Equal` keep their input order here. Because `lt` is inconsistent
    /// between an UndoPoint and any other kind (see [`Operation::lt`]; this
    /// returns `Less` from both sides), only sort slices that contain no
    /// UndoPoints or only UndoPoints — exactly what C++ `CmdInfo` sorts.
    pub fn cmp_cpp(&self, other: &Self) -> Ordering {
        if self.lt(other) {
            Ordering::Less
        } else if other.lt(self) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taskchampion::chrono::{TimeZone, Utc};
    use taskchampion::{TaskData, Uuid};

    const UUID1: Uuid = Uuid::from_u128(0x1234_5678_9abc_4def_8123_4567_89ab_cdef);

    fn update(
        property: &str,
        value: Option<&str>,
        old_value: Option<&str>,
        timestamp: i64,
    ) -> Operation {
        Operation::from(TcOperation::Update {
            uuid: UUID1,
            property: property.into(),
            old_value: old_value.map(Into::into),
            value: value.map(Into::into),
            timestamp: Utc.timestamp_opt(timestamp, 0).unwrap(),
        })
    }

    /// Operations as produced by the taskchampion `TaskData` API, the way the
    /// bridge's `create_task`/`update`/`delete_task` produce them.
    fn task_lifecycle() -> TcOperations {
        let mut ops = TcOperations::new();
        ops.push(TcOperation::UndoPoint);
        let mut task = TaskData::create(UUID1, &mut ops);
        task.update("description", Some("hello".into()), &mut ops);
        task.update("status", Some("pending".into()), &mut ops);
        task.update("description", Some("bye".into()), &mut ops);
        task.update("status", None, &mut ops);
        task.delete(&mut ops);
        ops
    }

    #[test]
    fn operations_preserves_order_and_kinds() {
        let ops = Operation::operations(&task_lifecycle());
        assert_eq!(ops.len(), 7);
        let kinds: Vec<(bool, bool, bool, bool)> = ops
            .iter()
            .map(|o| {
                (
                    o.is_create(),
                    o.is_update(),
                    o.is_delete(),
                    o.is_undo_point(),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                (false, false, false, true),
                (true, false, false, false),
                (false, true, false, false),
                (false, true, false, false),
                (false, true, false, false),
                (false, true, false, false),
                (false, false, true, false),
            ]
        );
        assert_eq!(ops[1].inner(), &TcOperation::Create { uuid: UUID1 });
    }

    #[test]
    fn getters() {
        let ops = Operation::operations(&task_lifecycle());
        let uuid = "12345678-9abc-4def-8123-456789abcdef";
        assert_eq!(ops[1].get_uuid(), uuid);
        assert_eq!(ops[2].get_uuid(), uuid);
        assert_eq!(ops[6].get_uuid(), uuid);

        assert_eq!(ops[2].get_property(), "description");
        assert_eq!(ops[2].get_value(), Some("hello".to_string()));
        assert_eq!(ops[2].get_old_value(), None);

        assert_eq!(ops[4].get_property(), "description");
        assert_eq!(ops[4].get_value(), Some("bye".to_string()));
        assert_eq!(ops[4].get_old_value(), Some("hello".to_string()));

        assert_eq!(ops[5].get_property(), "status");
        assert_eq!(ops[5].get_value(), None);
        assert_eq!(ops[5].get_old_value(), Some("pending".to_string()));

        let now = Utc::now().timestamp();
        assert!((ops[2].get_timestamp() - now).abs() < 60);

        assert_eq!(
            ops[6].get_old_task(),
            vec![("description".to_string(), "bye".to_string())]
        );
    }

    #[test]
    fn old_task_is_sorted() {
        let old_task = [("z", "1"), ("a", "2"), ("m", "3")]
            .iter()
            .map(|(p, v)| (p.to_string(), v.to_string()))
            .collect();
        let op = Operation::from(TcOperation::Delete {
            uuid: UUID1,
            old_task,
        });
        assert_eq!(
            op.get_old_task(),
            vec![
                ("a".into(), "2".into()),
                ("m".into(), "3".into()),
                ("z".into(), "1".into())
            ]
        );
    }

    #[test]
    fn timestamp_is_seconds() {
        assert_eq!(
            update("x", Some("1"), None, 1_234_567_890).get_timestamp(),
            1_234_567_890
        );
    }

    #[test]
    #[should_panic(expected = "operation has no uuid")]
    fn undo_point_has_no_uuid() {
        Operation::from(TcOperation::UndoPoint).get_uuid();
    }

    #[test]
    #[should_panic(expected = "operation is not an update")]
    fn create_has_no_property() {
        Operation::from(TcOperation::Create { uuid: UUID1 }).get_property();
    }

    #[test]
    #[should_panic(expected = "operation is not a delete")]
    fn update_has_no_old_task() {
        update("x", None, None, 0).get_old_task();
    }

    #[test]
    fn partial_order_matches_cpp() {
        let create = Operation::from(TcOperation::Create { uuid: UUID1 });
        let early = update("a", None, None, 100);
        let late = update("a", None, None, 200);
        let delete = Operation::from(TcOperation::Delete {
            uuid: UUID1,
            old_task: Default::default(),
        });
        let undo = Operation::from(TcOperation::UndoPoint);

        // Expected `row.lt(column)`, from Operation.cpp.
        let all = [&create, &early, &late, &delete, &undo];
        let expected = [
            // create early  late   delete undo
            [false, true, true, true, true],    // create
            [false, false, true, true, true],   // early
            [false, false, false, true, true],  // late
            [false, false, false, false, true], // delete
            [true, true, true, true, false],    // undo (C++ quirk, see `lt`)
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                assert_eq!(a.lt(b), expected[i][j], "{i} < {j}");
            }
        }

        // Same kind, same timestamp: neither is less.
        let early2 = update("b", Some("x"), None, 100);
        assert!(!early.lt(&early2) && !early2.lt(&early));
        assert_eq!(early.cmp_cpp(&early2), Ordering::Equal);

        assert_eq!(undo.cmp_cpp(&create), Ordering::Less);
        assert_eq!(create.cmp_cpp(&undo), Ordering::Less);
        assert_eq!(undo.cmp_cpp(&undo.clone()), Ordering::Equal);

        // Sorting operations without UndoPoints, as CmdInfo does.
        let mut ops = vec![late.clone(), delete.clone(), early.clone(), create.clone()];
        ops.sort_by(Operation::cmp_cpp);
        assert_eq!(ops, vec![create, early, late, delete]);
    }
}
