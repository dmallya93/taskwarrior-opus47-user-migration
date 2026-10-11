//! Rust translation of `src/commands/CmdImportV2.cpp`.
//!
//! `task import-v2`: read the Taskwarrior 2.x `pending.data` and
//! `completed.data` files from the data directory with [`Tf2`] and write
//! every task's attributes straight into the replica, printing ` add ` /
//! ` mod ` lines (or ` err  - Task with no UUID`).

use taskchampion::{Operations, TaskData};

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::shared::format::format;
use crate::shared::fs::File;
use crate::shared::shared::with_console;
use crate::tdb2::{add_undo_point, rt, uuid_from_string};
use crate::tf2::{Tf2, Tf2Task};

/// C++ `std::cout << text`, flushed so it interleaves with the other
/// standard output writers.
fn cout(text: &str) {
    with_console(|_, stdout| {
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    });
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdImportV2` (`import-v2`).
pub struct CmdImportV2 {
    meta: CommandMeta,
}

impl CmdImportV2 {
    /// C++ `CmdImportV2::CmdImportV2`.
    pub fn new() -> Self {
        CmdImportV2 {
            meta: CommandMeta {
                keyword: "import-v2".into(),
                usage: "task          import-v2".into(),
                description: "Imports Taskwarrior v2.x files".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Migration,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdImportV2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdImportV2 {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdImportV2::execute`.
    fn execute(&self, context: &mut Context, _output: &mut String) -> Result<i32> {
        let mut task_data: Vec<Tf2Task> = Vec::new();

        let location = context.data_dir.data.clone();
        let locking = context.config.get_boolean("locking");
        let pending_file = File::new(&format!("{location}/pending.data"));
        if pending_file.exists() {
            let mut pending_tf = Tf2::new();
            pending_tf.target(&pending_file.data);
            let pending_tasks = pending_tf.get_tasks(locking)?;
            task_data.extend_from_slice(pending_tasks);
            context.time_load_us += pending_tf.time_load_us;
        }
        let completed_file = File::new(&format!("{location}/completed.data"));
        if completed_file.exists() {
            let mut completed_tf = Tf2::new();
            completed_tf.target(&completed_file.data);
            let completed_tasks = completed_tf.get_tasks(locking)?;
            task_data.extend_from_slice(completed_tasks);
            context.time_load_us += completed_tf.time_load_us;
        }

        let count = Self::import(context, &task_data)?;

        context.footnote(&format(
            "Imported {1} tasks from `*.data` files. You may now delete these files.",
            &[&count],
        ));
        Ok(0)
    }
}

impl CmdImportV2 {
    /// C++ `int CmdImportV2::import (const std::vector<std::map<...>>&)`:
    /// creates (or updates) each task in the replica and commits all
    /// operations, after one undo point, at the end. Returns the number of
    /// tasks imported.
    ///
    /// Like C++, this bypasses `TDB2` (no validation, hooks or cache
    /// invalidation), and a task repeated in the input is looked up in the
    /// replica, not among the uncommitted operations.
    ///
    /// Deviation: C++ `tc::uuid_from_string` panics (aborting the process)
    /// on an invalid UUID; here it is the error `<uuid> is not a valid UUID`
    /// and nothing is committed.
    ///
    /// # Errors
    /// An invalid UUID or a replica failure.
    pub fn import(context: &mut Context, task_data: &[Tf2Task]) -> Result<i32> {
        let mut count = 0;
        let uuid_key = "uuid";
        let id_key = "id";
        let descr_key = "description";
        let replica = context.tdb2.replica()?;
        let mut ops = Operations::new();
        add_undo_point(&mut ops);

        for task in task_data {
            let Some(uuid_str) = task.get(uuid_key) else {
                cout(" err  - Task with no UUID\n");
                continue;
            };
            let uuid = uuid_from_string(uuid_str)?;

            let mut added_task = false;
            let maybe_task_data = rt().block_on(replica.get_task_data(uuid))?;
            let mut task_data = match maybe_task_data {
                Some(task_data) => task_data,
                None => {
                    added_task = true;
                    TaskData::create(uuid, &mut ops)
                }
            };

            for (name, value) in task {
                if name == uuid_key || name == id_key {
                    continue;
                }
                task_data.update(name, Some(value.clone()), &mut ops);
            }
            count += 1;

            let mut line = String::from(if added_task { " add " } else { " mod " });
            line.push_str(uuid_str);
            line.push(' ');
            match task.get(descr_key) {
                Some(description) => line.push_str(description),
                None => line.push_str("(no description)"),
            }
            line.push('\n');
            cout(&line);
        }

        rt().block_on(replica.commit_operations(ops))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::shared::{begin_console_override, end_console_override};
    use crate::test_util::CommandFixture;

    const ONE: &str = "aaaaaaaa-0000-4000-8000-000000000001";
    const NEW1: &str = "cccccccc-0000-4000-8000-000000000001";
    const NEW2: &str = "cccccccc-0000-4000-8000-000000000002";

    const SEED: &[&str] = &[
        r#"{"uuid":"aaaaaaaa-0000-4000-8000-000000000001","description":"one","status":"pending","entry":"20260101T000000Z"}"#,
    ];

    fn fixture() -> CommandFixture {
        CommandFixture::new(SEED, "", &["import-v2"])
    }

    fn write_data(f: &CommandFixture, name: &str, contents: &str) {
        std::fs::write(format!("{}/{name}", f.ctx.data_dir.data), contents).unwrap();
    }

    /// Runs `import-v2`; returns the result and the captured standard output.
    fn run(f: &mut CommandFixture) -> (Result<i32>, String) {
        begin_console_override("");
        let result = f.run(&CmdImportV2::new());
        (result, end_console_override())
    }

    #[test]
    fn imports_pending_then_completed() {
        let mut f = fixture();
        write_data(
            &f,
            "pending.data",
            &format!(
                "[description:\"bing\" due:\"1734480000\" entry:\"1734397061\" status:\"pending\" tags:\"a,b\" uuid:\"{NEW1}\"]\n"
            ),
        );
        write_data(
            &f,
            "completed.data",
            &format!(
                "[end:\"1734397065\" entry:\"1734397056\" id:\"7\" status:\"completed\" uuid:\"{NEW2}\"]\n[description:\"no uuid\"]"
            ),
        );
        let (rc, out) = run(&mut f);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(
            out,
            format!(" add {NEW1} bing\n add {NEW2} (no description)\n err  - Task with no UUID\n")
        );
        assert_eq!(
            f.ctx.footnotes,
            vec![
                "Imported 2 tasks from `*.data` files. You may now delete these files.".to_string()
            ]
        );

        let bing = f.task(NEW1);
        assert_eq!(bing.get_ref("description"), "bing");
        assert_eq!(bing.get_ref("due"), "1734480000");
        assert!(bing.has("tag_a") && bing.has("tag_b"));
        let done = f.task(NEW2);
        assert_eq!(done.get_ref("status"), "completed");
        assert_eq!(done.get_ref("end"), "1734397065");
        // `id` and `uuid` are not stored as attributes.
        assert!(!done.has("id"));
    }

    #[test]
    fn updates_an_existing_task() {
        let mut f = fixture();
        write_data(
            &f,
            "pending.data",
            &format!("[description:\"renamed\" project:\"p\" uuid:\"{ONE}\"]"),
        );
        let (rc, out) = run(&mut f);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, format!(" mod {ONE} renamed\n"));
        let one = f.task(ONE);
        assert_eq!(one.get_ref("description"), "renamed");
        assert_eq!(one.get_ref("project"), "p");
        assert_eq!(one.get_ref("entry"), "1767225600");
    }

    #[test]
    fn no_files_imports_nothing() {
        let mut f = fixture();
        let (rc, out) = run(&mut f);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "");
        assert_eq!(
            f.ctx.footnotes,
            vec![
                "Imported 0 tasks from `*.data` files. You may now delete these files.".to_string()
            ]
        );
    }

    #[test]
    fn parse_error_names_file_and_line() {
        let mut f = fixture();
        write_data(&f, "pending.data", &format!("[uuid:\"{NEW1}\"]\nbad"));
        let (rc, out) = run(&mut f);
        let path = format!("{}/pending.data", f.ctx.data_dir.data);
        assert_eq!(
            rc.unwrap_err().to_string(),
            format!("Record not recognized as format 4. in {path} at line 2")
        );
        assert_eq!(out, "");
        assert!(f.ctx.tdb2.get_by_uuid(NEW1).unwrap().is_none());
    }

    #[test]
    fn invalid_uuid_is_an_error() {
        let mut f = fixture();
        write_data(&f, "pending.data", "[uuid:\"nope\"]");
        let (rc, _) = run(&mut f);
        assert_eq!(rc.unwrap_err().to_string(), "nope is not a valid UUID");
    }
}
