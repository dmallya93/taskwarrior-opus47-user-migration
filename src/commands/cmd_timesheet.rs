//! Rust translation of `src/commands/CmdTimesheet.cpp`.
//!
//! Each struct below is one C++ class from `CmdTimesheet.cpp`, carrying the exact
//! metadata of its C++ constructor.
//!
//! As in Taskwarrior 3.5.0 the report is a fixed `Table` (Wk, Date, Day, ID,
//! Action, Project, Due, Task); only `report.timesheet.filter` (the default
//! filter) and `report.timesheet.context` are read from the configuration.

use crate::cli2;
use crate::commands::cmd_info::cpp_sort;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::filter::Filter;
use crate::rules::auto_colorize;
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::datetime::Datetime;
use crate::shared::format::format;
use crate::shared::table::Table;
use crate::task::{Status, Task};
use crate::util::{optional_blank_line, set_header_underline};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdTimesheet` (`timesheet`).
///
/// The C++ class also overrides `uses_context` to honour
/// `report.timesheet.context` ([`Command::uses_context`]).
pub struct CmdTimesheet {
    meta: CommandMeta,
}

impl CmdTimesheet {
    /// C++ `CmdTimesheet::CmdTimesheet`.
    pub fn new() -> Self {
        CmdTimesheet {
            meta: CommandMeta {
                keyword: "timesheet".into(),
                usage: "task [filter] timesheet".into(),
                description: "Summary of completed and started tasks".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Report,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdTimesheet {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdTimesheet {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdTimesheet::uses_context`: honours `report.timesheet.context`.
    fn uses_context(&self, config: &Configuration) -> bool {
        let key = "report.timesheet.context";

        if config.has(key) {
            config.get_boolean(key)
        } else {
            self.meta.uses_context
        }
    }

    /// C++ `CmdTimesheet::execute`.
    ///
    /// The tasks are ordered with libstdc++'s (unstable) `std::sort` port, so
    /// tasks with equal `_key` values come out in the C++ order.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let rc = 0;

        // Detect a filter.
        let has_filter = context.cli2.args.iter().any(|a| a.has_tag("FILTER"));

        if !has_filter {
            let mut default_filter = context.config.get("report.timesheet.filter");
            if default_filter.is_empty() {
                default_filter = "(+PENDING and start.after:now-4wks) or (+COMPLETED and \
                                  end.after:now-4wks)"
                    .to_string();
            }
            cli2::with_cli2(context, |cli2, ctx| cli2.add_filter(ctx, &default_filter))?;
        }

        // Apply filter to get a set of tasks.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        // Subset the tasks to only those that are either completed or started.
        // The _key attribute is represents either the 'start' or 'end' date.
        let mut num_completed = 0;
        let mut num_started = 0;
        let mut shown: Vec<Task> = Vec::new();
        for mut task in filtered {
            if task.get_status()? == Status::Completed {
                let end = task.get("end");
                task.set("_key", &end);
                num_completed += 1;
            }

            if task.get_status()? == Status::Pending && task.has("start") {
                let start = task.get("start");
                task.set("_key", &start);
                num_started += 1;
            }

            shown.push(task);
        }

        // Sort tasks by _key.
        cpp_sort(&mut shown, |a: &Task, b: &Task| a.get("_key") < b.get("_key"));

        // Render the completed table.
        let mut table = Table::new();
        table.width(context.get_width());
        if context.config.get_boolean("obfuscate") {
            table.obfuscate();
        }
        table.add("Wk");
        table.add("Date");
        table.add("Day");
        table.add("ID");
        table.add("Action");
        table.add("Project");
        table.add("Due");
        table.add("Task");
        set_header_underline(context, &mut table)?;

        let dateformat = context.config.get("dateformat");

        let mut previous_week: i32 = -1;
        let mut previous_date = String::new();
        let mut previous_day = String::new();
        let mut week_counter: i32 = 0;
        for task in &shown {
            let key = Datetime::from_epoch(task.get_date("_key"));

            let label = if task.has("end") {
                "Completed"
            } else if task.has("start") {
                "Started"
            } else {
                ""
            };

            let week = key.week();
            let date = key.to_string(&dateformat);
            let due = if task.has("due") {
                Datetime::from_string(&task.get("due"), "")?.to_string(&dateformat)
            } else {
                String::new()
            };
            let day = Datetime::day_name_short(key.day_of_week());

            let mut task_color = Color::new();
            auto_colorize(context, task, &mut task_color)?;

            // Add a blank line between weeks.
            if week != previous_week && previous_week != -1 {
                let row = table.add_row_even();
                table.set(row, 0, " ");
            }

            // Keep track of unique week numbers.
            if week != previous_week {
                week_counter += 1;
            }

            // User-defined oddness.
            let row = if week_counter % 2 != 0 {
                table.add_row_odd()
            } else {
                table.add_row_even()
            };

            // If the data doesn't change, it doesn't get shown.
            let week_label = if week != previous_week {
                format("W{1}", &[&week])
            } else {
                String::new()
            };
            table.set(row, 0, &week_label);
            table.set(row, 1, if date != previous_date { &date } else { "" });
            table.set(row, 2, if day != previous_day { &day } else { "" });
            table.set(row, 3, &task.identifier(true));
            table.set(row, 4, label);
            table.set(row, 5, &task.get("project"));
            table.set(row, 6, &due);
            table.set_colored(row, 7, &task.get("description"), task_color);

            previous_week = week;
            previous_date = date;
            previous_day = day;
        }

        // Render the table.
        let mut out = String::new();
        if table.rows() != 0 {
            out += optional_blank_line(context);
            out += &table.render();
            out.push('\n');
        }

        if context.verbose("affected") {
            out += &format("{1} completed, {2} started.", &[&num_completed, &num_started]);
            out.push('\n');
        }

        *output = out;
        Ok(rc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    fn run(seed: &[&str], args: &[&str]) -> (i32, String) {
        let mut f = CommandFixture::new(
            seed,
            "defaultwidth=100\ndateformat=Y-M-D\nverbose=affected\nfontunderline=0\n",
            args,
        );
        let mut output = String::new();
        let rc = CmdTimesheet::new().execute(&mut f.ctx, &mut output).unwrap();
        (rc, output)
    }

    #[test]
    fn uses_context_follows_report_setting() {
        let mut config = Configuration::new();
        assert!(!CmdTimesheet::new().uses_context(&config));
        config.set("report.timesheet.context", "1");
        assert!(CmdTimesheet::new().uses_context(&config));
    }

    #[test]
    fn explicit_filter_groups_by_week() {
        let seed = [
            r#"{"description":"late","status":"completed","entry":"20150101T120000Z","end":"20150114T120000Z"}"#,
            r#"{"description":"early","status":"completed","entry":"20150101T120000Z","end":"20150106T120000Z"}"#,
            r#"{"description":"same","status":"completed","entry":"20150101T120000Z","end":"20150106T130000Z","project":"pr"}"#,
            r#"{"description":"gone","status":"deleted","entry":"20150101T120000Z","end":"20150106T120000Z"}"#,
        ];
        let (rc, out) = run(&seed, &["status:completed", "timesheet"]);
        assert_eq!(rc, 0);
        let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
        assert_eq!(lines[0], "Wk Date       Day ID       Action    Project Due Task", "{out}");
        assert_eq!(lines[1], "-- ---------- --- -------- --------- ------- --- -----", "{out}");
        assert!(lines[2].starts_with("W1 2015-01-06 Tue "), "{out}");
        assert!(lines[2].ends_with(" Completed             early"), "{out}");
        assert!(lines[3].starts_with("                  "), "{out}");
        assert!(lines[3].ends_with(" Completed pr          same"), "{out}");
        assert_eq!(lines[4], "");
        assert!(lines[5].starts_with("W2 2015-01-14 Wed "), "{out}");
        assert!(lines[5].ends_with(" late"), "{out}");
        // The table is followed by a blank line, then the affected footnote.
        assert_eq!(lines[6], "");
        assert_eq!(lines[7], "3 completed, 0 started.");
        assert_eq!(lines.len(), 8, "{out}");
    }

    #[test]
    fn default_filter_without_matches() {
        let seed =
            [r#"{"description":"old","status":"completed","entry":"20150101T120000Z","end":"20150114T120000Z"}"#];
        let (rc, out) = run(&seed, &["timesheet"]);
        assert_eq!((rc, out.as_str()), (0, "0 completed, 0 started.\n"));
    }
}
