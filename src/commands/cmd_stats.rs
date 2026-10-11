//! Rust translation of `src/commands/CmdStats.cpp`.
//!
//! Each struct below is one C++ class from `CmdStats.cpp`, carrying the exact
//! metadata of its C++ constructor.

use std::collections::BTreeMap;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::filter::Filter;
use crate::shared::color::Color;
use crate::shared::datetime::Datetime;
use crate::shared::duration::Duration;
use crate::shared::format::{format, printf_g};
use crate::shared::localtime::time_now;
use crate::shared::table::Table;
use crate::task::{Status, Task};
use crate::util::{optional_blank_line, set_header_underline};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdStats` (`stats`).
pub struct CmdStats {
    meta: CommandMeta,
}

impl CmdStats {
    /// C++ `CmdStats::CmdStats`.
    pub fn new() -> Self {
        CmdStats {
            meta: CommandMeta {
                keyword: "stats".into(),
                usage: "task <filter> stats".into(),
                description: "Shows task database statistics".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Adds a `label` / `value` row to `view`.
fn add_row(view: &mut Table, label: &str, value: &str) {
    let row = view.add_row();
    view.set(row, 0, label);
    view.set(row, 1, value);
}

/// Adds a `label` / int `value` row to `view` (C++ `Table::set (row, col, int)`).
fn add_row_int(view: &mut Table, label: &str, value: i32) {
    let row = view.add_row();
    view.set(row, 0, label);
    view.set_int(row, 1, value);
}

impl Command for CmdStats {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdStats::execute`.
    ///
    /// The C++ `float daysPending` accumulator is kept as `f32`: each `+=`
    /// adds a `double` and narrows back to `float`, and the average divides
    /// and multiplies in `float`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let rc = 0;
        let mut out = String::new();

        let dateformat = context.config.get("dateformat");

        // Count the possible reverts.
        let undo_count = context.tdb2.num_reverts_possible()?;

        // Count the backlog transactions.
        let num_local_changes = context.tdb2.num_local_changes()?;

        // Get all the tasks.
        let mut filter = Filter::new();
        let all = context.tdb2.all_tasks()?;
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset_of(context, &all, &mut filtered)?;

        let now = Datetime::new();
        let mut earliest: i64 = time_now();
        let mut latest: i64 = 1;
        let mut total_t: i32 = 0;
        let mut deleted_t: i32 = 0;
        let mut pending_t: i32 = 0;
        let mut completed_t: i32 = 0;
        let mut waiting_t: i32 = 0;
        let mut tagged_t: i32 = 0;
        let mut annotations_t: i32 = 0;
        let mut recurring_t: i32 = 0;
        let mut blocking_t: i32 = 0;
        let mut blocked_t: i32 = 0;
        let mut days_pending: f32 = 0.0;
        let mut desc_length: i32 = 0;
        let mut all_tags: BTreeMap<String, i32> = BTreeMap::new();
        let mut all_projects: BTreeMap<String, i32> = BTreeMap::new();

        for task in &filtered {
            total_t += 1;

            let status = task.get_status()?;
            match status {
                Status::Deleted => deleted_t += 1,
                Status::Pending => pending_t += 1,
                Status::Completed => completed_t += 1,
                Status::Recurring => recurring_t += 1,
                Status::Waiting => waiting_t += 1,
            }

            if task.is_blocked {
                blocked_t += 1;
            }
            if task.is_blocking {
                blocking_t += 1;
            }

            let entry = crate::dom::strtoll(&task.get("entry"));
            if entry < earliest {
                earliest = entry;
            }
            if entry > latest {
                latest = entry;
            }

            if status == Status::Completed {
                let end = crate::dom::strtoll(&task.get("end"));
                days_pending = (f64::from(days_pending) + (end - entry) as f64 / 86400.0) as f32;
            }

            if status == Status::Pending {
                days_pending =
                    (f64::from(days_pending) + (now.to_epoch() - entry) as f64 / 86400.0) as f32;
            }

            desc_length += task.get("description").len() as i32;
            annotations_t += task.get_annotations().len() as i32;

            let tags = task.get_tags();
            if !tags.is_empty() {
                tagged_t += 1;
            }

            for tag in tags {
                all_tags.insert(tag, 0);
            }

            let project = task.get("project");
            if !project.is_empty() {
                all_projects.insert(project, 0);
            }
        }

        // Create a table for output.
        let mut view = Table::new();
        view.width(context.get_width());
        let with_color = context.color();
        view.with_color(with_color);
        view.intra_padding(2);
        view.add("Category");
        view.add("Data");
        set_header_underline(context, &mut view)?;

        add_row_int(&mut view, "Pending", pending_t);
        add_row_int(&mut view, "Waiting", waiting_t);
        add_row_int(&mut view, "Recurring", recurring_t);
        add_row_int(&mut view, "Completed", completed_t);
        add_row_int(&mut view, "Deleted", deleted_t);
        add_row_int(&mut view, "Total", total_t);
        add_row_int(&mut view, "Annotations", annotations_t);
        add_row_int(&mut view, "Unique tags", all_tags.len() as i32);
        add_row_int(&mut view, "Projects", all_projects.len() as i32);
        add_row_int(&mut view, "Blocked tasks", blocked_t);
        add_row_int(&mut view, "Blocking tasks", blocking_t);
        add_row_int(&mut view, "Undo transactions", undo_count);
        add_row_int(&mut view, "Sync backlog transactions", num_local_changes);

        if total_t != 0 {
            // std::setprecision (3) << double: the `%.3g` form.
            let value = printf_g(100.0 * f64::from(tagged_t) / f64::from(total_t), 3) + "%";
            add_row(&mut view, "Tasks tagged", &value);
        }

        if !filtered.is_empty() {
            let e = Datetime::from_epoch(earliest);
            add_row(&mut view, "Oldest task", &e.to_string(&dateformat));

            let l = Datetime::from_epoch(latest);
            add_row(&mut view, "Newest task", &l.to_string(&dateformat));

            add_row(
                &mut view,
                "Task used for",
                &Duration::from_seconds(latest - earliest).format_vague(false),
            );
        }

        if total_t != 0 {
            add_row(
                &mut view,
                "Task added every",
                &Duration::from_seconds((latest - earliest) / i64::from(total_t))
                    .format_vague(false),
            );
        }

        if completed_t != 0 {
            add_row(
                &mut view,
                "Task completed every",
                &Duration::from_seconds((latest - earliest) / i64::from(completed_t))
                    .format_vague(false),
            );
        }

        if deleted_t != 0 {
            add_row(
                &mut view,
                "Task deleted every",
                &Duration::from_seconds((latest - earliest) / i64::from(deleted_t))
                    .format_vague(false),
            );
        }

        if pending_t != 0 || completed_t != 0 {
            let seconds = ((days_pending / (pending_t + completed_t) as f32) * 86400.0) as i32;
            add_row(
                &mut view,
                "Average time pending",
                &Duration::from_seconds(i64::from(seconds)).format_vague(false),
            );
        }

        if total_t != 0 {
            add_row(
                &mut view,
                "Average desc length",
                &format("{1} characters", &[&(desc_length / total_t)]),
            );
        }

        // If an alternating row color is specified, notify the table.
        if context.color() {
            let alternate = Color::parse(&context.config.get("color.alternate"))?;
            if alternate.nontrivial() {
                view.color_odd(alternate);
                view.intra_color_odd(alternate);
                view.extra_color_odd(alternate);
            }
        }

        out += optional_blank_line(context);
        out += &view.render();
        out += optional_blank_line(context);

        *output = out;
        Ok(rc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    fn run(seed: &[&str], args: &[&str]) -> (i32, Vec<(String, String)>) {
        let mut f = CommandFixture::new(seed, "defaultwidth=80\ndateformat=Y-M-D\n", args);
        let mut output = String::new();
        let rc = CmdStats::new().execute(&mut f.ctx, &mut output).unwrap();
        let rows = output
            .lines()
            .skip(2)
            .map(|l| {
                let (k, v) = l.split_at(27.min(l.len()));
                (k.trim_end().to_string(), v.to_string())
            })
            .collect();
        (rc, rows)
    }

    fn value<'a>(rows: &'a [(String, String)], key: &str) -> &'a str {
        rows.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("{key} missing in {rows:?}"))
    }

    #[test]
    fn counts_and_averages() {
        let seed = [
            r#"{"description":"abcd","project":"p","tags":["t1","t2"],"status":"completed","entry":"20150101T000000Z","end":"20150111T000000Z"}"#,
            r#"{"description":"ab","project":"p.q","status":"deleted","entry":"20150102T000000Z","end":"20150103T000000Z"}"#,
            r#"{"description":"abcdef","tags":["t2"],"status":"completed","entry":"20150105T000000Z","end":"20150106T000000Z","annotation_1420070400":"note"}"#,
        ];
        let (rc, rows) = run(&seed, &["stats"]);
        assert_eq!(rc, 0);
        assert_eq!(value(&rows, "Pending"), "0");
        assert_eq!(value(&rows, "Completed"), "2");
        assert_eq!(value(&rows, "Deleted"), "1");
        assert_eq!(value(&rows, "Total"), "3");
        assert_eq!(value(&rows, "Annotations"), "1");
        assert_eq!(value(&rows, "Unique tags"), "2");
        assert_eq!(value(&rows, "Projects"), "2");
        assert_eq!(value(&rows, "Tasks tagged"), "66.7%");
        assert_eq!(value(&rows, "Task used for"), "4d");
        // 4 days / 3 tasks = 32h, shown vaguely as whole days.
        assert_eq!(value(&rows, "Task added every"), "1d");
        assert_eq!(value(&rows, "Task completed every"), "2d");
        assert_eq!(value(&rows, "Task deleted every"), "4d");
        // (10 + 1) days / 2 completed.
        assert_eq!(value(&rows, "Average time pending"), "5d");
        assert_eq!(value(&rows, "Average desc length"), "4 characters");
    }

    #[test]
    fn empty_database_has_no_derived_rows() {
        let (rc, rows) = run(&[], &["stats"]);
        assert_eq!(rc, 0);
        assert_eq!(rows.len(), 13, "{rows:?}");
        assert_eq!(value(&rows, "Total"), "0");
        assert_eq!(rows[12].0, "Sync backlog transactions");
    }
}
