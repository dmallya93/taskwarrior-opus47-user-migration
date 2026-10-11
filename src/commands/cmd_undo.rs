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
                (None, Some(value)) => mods.push_str(&format!(
                    "Add property '{property}' with value '{value}'\n"
                )),
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
