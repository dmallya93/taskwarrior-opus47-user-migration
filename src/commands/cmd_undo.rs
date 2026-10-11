//! Rust translation of `src/commands/CmdUndo.cpp`.
//!
//! `task undo`: show the operations since the most recent undo point (one
//! per command invocation, added by `TDB2::maybe_add_undo_point` for the
//! first change of a run, or explicitly by `import-v2`), ask for
//! confirmation (`rc.confirmation`), then commit their reversal.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::operation::Operation;
use crate::shared::shared::{confirm, with_console};
use crate::shared::table::Table;
use crate::task::Task;

/// C++ `std::cout << text`, flushed so it interleaves with the prompt.
fn cout(text: &str) {
    with_console(|_, stdout| {
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    });
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUndo` (`undo`).
pub struct CmdUndo {
    meta: CommandMeta,
}

impl CmdUndo {
    /// C++ `CmdUndo::CmdUndo`.
    pub fn new() -> Self {
        CmdUndo {
            meta: CommandMeta {
                keyword: "undo".into(),
                usage: "task          undo".into(),
                description: "Reverts the most recent change to a task".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdUndo {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUndo {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUndo::execute`.
    ///
    /// Messages go straight to standard output, as the C++ writes them with
    /// `std::cout` (the output buffer stays empty). "Could not undo: other
    /// operations have occurred." has no trailing newline, as in C++.
    ///
    /// With nothing to undo, `confirm_revert` returns true and taskchampion's
    /// `commit_reversed_operations` returns false for an empty list, so both
    /// "No operations to undo." and "Could not undo: ..." are printed, as in
    /// C++ (both link taskchampion 3.1).
    fn execute(&self, context: &mut Context, _output: &mut String) -> Result<i32> {
        let undo_ops = context.tdb2.get_undo_operations()?;
        if confirm_revert(context, &Operation::operations(&undo_ops)) {
            // Note that commit_reversed_operations rebuilds the working set, so that
            // need not be done here.
            if !context.tdb2.commit_reversed_operations(undo_ops)? {
                cout("Could not undo: other operations have occurred.");
            }
        }
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUndo::confirm_revert`: list the operations to revert, grouped by
/// task in the order given, and ask for confirmation when `rc.confirmation`
/// is on. With nothing to revert, prints "No operations to undo." and
/// returns true.
fn confirm_revert(context: &mut Context, undo_ops: &[Operation]) -> bool {
    // Count non-undo operations
    let ops_count = undo_ops.iter().filter(|op| !op.is_undo_point()).count();
    if ops_count == 0 {
        cout("No operations to undo.\n");
        return true;
    }

    cout(&format!(
        "The following {ops_count} operations would be reverted:\n"
    ));

    let mut view = Table::new();
    if context.config.get_boolean("obfuscate") {
        view.obfuscate();
    }
    view.width(context.get_width());
    view.add("Uuid");
    view.add("Modification");

    let mut last_uuid = String::new();
    let mut mods = String::new();
    for op in undo_ops {
        if op.is_undo_point() {
            continue;
        }

        if last_uuid != op.get_uuid() {
            if !last_uuid.is_empty() {
                let row = view.add_row();
                view.set(row, 0, &last_uuid);
                view.set(row, 1, &mods);
            }
            last_uuid = op.get_uuid();
            mods.clear();
        }

        if op.is_create() {
            mods.push_str("Create task\n");
        } else if op.is_delete() {
            mods.push_str("Delete (purge) task");
        } else if op.is_update() {
            let property = op.get_property();
            let old_value = op.get_old_value();
            let value = op.get_value();
            if Task::is_tag_attr(&property) {
                if value.as_deref() == Some("x") {
                    mods.push_str(&format!("Add tag '{}'\n", Task::attr2tag(&property)));
                    continue;
                } else if value.is_none() && old_value.as_deref() == Some("x") {
                    mods.push_str(&format!("Remove tag '{}'\n", Task::attr2tag(&property)));
                    continue;
                }
            }
            match (&old_value, &value) {
                (Some(old_value), Some(value)) => mods.push_str(&format!(
                    "Update property '{property}' from '{old_value}' to '{value}'\n"
                )),
                (Some(old_value), None) => mods.push_str(&format!(
                    "Delete property '{property}' (was '{old_value}')\n"
                )),
                (None, Some(value)) => {
                    mods.push_str(&format!("Add property '{property}' with value '{value}'\n"))
                }
                (None, None) => {}
            }
        }
    }
    let row = view.add_row();
    view.set(row, 0, &last_uuid);
    view.set(row, 1, &mods);
    cout(&format!("{}\n", view.render()));

    !context.config.get_boolean("confirmation")
        || confirm(
            "The undo command is not reversible.  Are you sure you want to revert to the previous \
             state?",
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::shared::{begin_console_override, end_console_override};
    use crate::test_util::CommandFixture;
    use taskchampion::chrono::Utc;
    use taskchampion::{Operation as TcOperation, Operations as TcOperations, Uuid};

    const ONE: &str = "aaaaaaaa-0000-4000-8000-000000000001";
    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z"}"#,
    ];

    fn stored(f: &mut CommandFixture, uuid: &str) -> bool {
        f.ctx
            .tdb2
            .get_task_data(Uuid::parse_str(uuid).unwrap())
            .unwrap()
            .is_some()
    }

    /// Runs `undo` with the scripted console `input`, returning its stdout.
    fn undo(f: &mut CommandFixture, input: &str) -> String {
        begin_console_override(input);
        let rc = f.run(&CmdUndo::new()).unwrap();
        let shown = end_console_override();
        assert_eq!(rc, 0);
        shown
    }

    #[test]
    fn nothing_to_undo() {
        let mut f = CommandFixture::new(&[], "confirmation=off\n", &["undo"]);
        assert_eq!(
            undo(&mut f, ""),
            "No operations to undo.\nCould not undo: other operations have occurred."
        );
    }

    #[test]
    fn reverts_the_last_invocation() {
        let mut f = CommandFixture::new(SEED, "confirmation=off\n", &["undo"]);
        assert!(stored(&mut f, ONE));
        let shown = undo(&mut f, "");
        assert!(shown.starts_with("The following "), "{shown}");
        assert!(
            shown.contains(" operations would be reverted:\n"),
            "{shown}"
        );
        assert!(shown.contains(&format!("{ONE} Create task\n")), "{shown}");
        assert!(!shown.contains("Are you sure"), "{shown}");
        assert!(!stored(&mut f, ONE));

        // The seed was the only undo point.
        assert_eq!(
            undo(&mut f, ""),
            "No operations to undo.\nCould not undo: other operations have occurred."
        );
    }

    #[test]
    fn declined_confirmation_keeps_the_task() {
        let mut f = CommandFixture::new(SEED, "confirmation=on\n", &["undo"]);
        let shown = undo(&mut f, "no\n");
        assert!(
            shown.ends_with(
                "\nThe undo command is not reversible.  Are you sure you want to revert to the \
                 previous state? (yes/no) "
            ),
            "{shown}"
        );
        assert!(stored(&mut f, ONE));

        let shown = undo(&mut f, "yes\n");
        assert!(shown.ends_with("(yes/no) "), "{shown}");
        assert!(!stored(&mut f, ONE));
    }

    #[test]
    fn confirm_revert_lists_operations_grouped_by_uuid() {
        let mut f = CommandFixture::new(&[], "confirmation=off\n", &["undo"]);
        let u1 = Uuid::parse_str(ONE).unwrap();
        let u2 = Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000002").unwrap();
        let update =
            |uuid, property: &str, old: Option<&str>, value: Option<&str>| TcOperation::Update {
                uuid,
                property: property.into(),
                old_value: old.map(String::from),
                value: value.map(String::from),
                timestamp: Utc::now(),
            };
        let ops: TcOperations = vec![
            TcOperation::UndoPoint,
            TcOperation::Create { uuid: u1 },
            update(u1, "description", None, Some("one")),
            update(u1, "description", Some("one"), Some("uno")),
            update(u1, "priority", Some("H"), None),
            update(u1, "tag_foo", None, Some("x")),
            update(u1, "tag_bar", Some("x"), None),
            update(u1, "tag_baz", Some("y"), Some("z")),
            update(u1, "nothing", None, None),
            TcOperation::Delete {
                uuid: u2,
                old_task: Default::default(),
            },
            update(u2, "status", None, Some("pending")),
        ];

        begin_console_override("");
        assert!(confirm_revert(&mut f.ctx, &Operation::operations(&ops)));
        let shown = end_console_override();
        assert_eq!(
            shown,
            "The following 10 operations would be reverted:\n\
             Uuid                                 Modification\n\
             aaaaaaaa-0000-4000-8000-000000000001 Create task\n\
             \x20                                    Add property 'description' with value 'one'\n\
             \x20                                    Update property 'description' from 'one' to\n\
             \x20                                    'uno'\n\
             \x20                                    Delete property 'priority' (was 'H')\n\
             \x20                                    Add tag 'foo'\n\
             \x20                                    Remove tag 'bar'\n\
             \x20                                    Update property 'tag_baz' from 'y' to 'z'\n\
             aaaaaaaa-0000-4000-8000-000000000002 Delete (purge) taskAdd property 'status'\n\
             \x20                                    with value 'pending'\n\
             \n"
        );
    }
}
