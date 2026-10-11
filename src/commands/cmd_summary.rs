//! Rust translation of `src/commands/CmdSummary.cpp`.
//!
//! Each struct below is one C++ class from `CmdSummary.cpp`, carrying the exact
//! metadata of its C++ constructor.

use std::collections::{BTreeMap, HashMap};

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::filter::Filter;
use crate::shared::color::Color;
use crate::shared::duration::Duration;
use crate::shared::format::format;
use crate::shared::localtime::time_now;
use crate::shared::table::Table;
use crate::sort::sort_projects_bool;
use crate::task::{Status, Task};
use crate::util::{
    extract_parents_default, indent_project, optional_blank_line, set_header_underline,
};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdSummary` (`summary`).
pub struct CmdSummary {
    meta: CommandMeta,
}

impl CmdSummary {
    /// C++ `CmdSummary::CmdSummary`.
    pub fn new() -> Self {
        CmdSummary {
            meta: CommandMeta {
                keyword: "summary".into(),
                usage: "task <filter> summary".into(),
                description: "Shows a report of task status by project".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: false,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdSummary {
    fn default() -> Self {
        Self::new()
    }
}

/// C++ `std::map<std::string, T>::operator[]` read: the value, inserting
/// the default when the key is absent.
fn at<T: Copy + Default>(map: &mut HashMap<String, T>, key: &str) -> T {
    *map.entry(key.to_string()).or_default()
}

impl Command for CmdSummary {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdSummary::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut rc = 0;
        let show_all_projects = context.config.get_boolean("summary.all.projects");

        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        // Generate unique list of project names from all pending tasks.
        let mut all_projects: BTreeMap<String, bool> = BTreeMap::new();
        for task in &filtered {
            if show_all_projects || task.get_status()? == Status::Pending {
                all_projects.insert(task.get("project"), false);
            }
        }

        // Initialize counts, sum.
        let mut count_pending: HashMap<String, i32> = HashMap::new();
        let mut count_completed: HashMap<String, i32> = HashMap::new();
        let mut sum_entry: HashMap<String, f64> = HashMap::new();
        let mut counter: HashMap<String, i32> = HashMap::new();
        let now: i64 = time_now();

        // Initialize counters.
        for project in all_projects.keys() {
            count_pending.insert(project.clone(), 0);
            count_completed.insert(project.clone(), 0);
            sum_entry.insert(project.clone(), 0.0);
            counter.insert(project.clone(), 0);
        }

        // Count the various tasks.
        for task in &filtered {
            let project = task.get("project");
            let mut projects = extract_parents_default(&project);
            projects.push(project);

            for parent in &projects {
                *counter.entry(parent.clone()).or_insert(0) += 1;
            }

            let status = task.get_status()?;
            if status == Status::Pending || status == Status::Waiting {
                for parent in &projects {
                    *count_pending.entry(parent.clone()).or_insert(0) += 1;

                    let entry = crate::dom::strtoll(&task.get("entry"));
                    if entry != 0 {
                        let sum = at(&mut sum_entry, parent);
                        sum_entry.insert(parent.clone(), sum + (now - entry) as f64);
                    }
                }
            } else if status == Status::Completed {
                for parent in &projects {
                    *count_completed.entry(parent.clone()).or_insert(0) += 1;

                    let entry = crate::dom::strtoll(&task.get("entry"));
                    let end = crate::dom::strtoll(&task.get("end"));
                    if entry != 0 && end != 0 {
                        let sum = at(&mut sum_entry, parent);
                        sum_entry.insert(parent.clone(), sum + (end - entry) as f64);
                    }
                }
            }
        }

        // Create a table for output.
        let mut view = Table::new();
        view.width(context.get_width());
        view.add("Project");
        view.add_with("Remaining", false, true);
        view.add_with("Avg age", false, true);
        view.add_with("Complete", false, true);
        view.add_with("0%                        100%", true, false);
        set_header_underline(context, &mut view)?;

        let mut bar_color = Color::new();
        let mut bg_color = Color::new();
        let with_color = context.color();
        view.with_color(with_color);

        if context.color() {
            bar_color = Color::parse(&context.config.get("color.summary.bar"))?;
            bg_color = Color::parse(&context.config.get("color.summary.background"))?;
        }

        // sort projects into sorted list
        let mut sorted_projects: Vec<(String, i32)> = Vec::new();
        sort_projects_bool(&mut sorted_projects, &all_projects);

        let bar_width: i32 = 30;
        // construct view from sorted list
        for (name, _) in &sorted_projects {
            let row = view.add_row();
            view.set(
                row,
                0,
                &(if name.is_empty() {
                    "(none)".to_string()
                } else {
                    indent_project(name, "  ", '.')
                }),
            );

            view.set_int(row, 1, at(&mut count_pending, name));
            let count = at(&mut counter, name);
            if count != 0 {
                let average = (at(&mut sum_entry, name) / f64::from(count)) as i32;
                view.set(row, 2, &Duration::from_seconds(i64::from(average)).format_vague(false));
            }

            let c = at(&mut count_completed, name);
            let p = at(&mut count_pending, name);
            let mut completed_bar = 0;
            if c + p != 0 {
                completed_bar = (c * bar_width) / (c + p);
            }

            let mut bar = String::new();
            if context.color() {
                bar += &bar_color.colorize(&" ".repeat(completed_bar as usize));
                bar += &bg_color.colorize(&" ".repeat((bar_width - completed_bar) as usize));
            } else {
                bar += &"=".repeat(completed_bar as usize);
                bar += &" ".repeat((bar_width - completed_bar) as usize);
            }
            view.set(row, 4, &bar);

            let mut percent = "0%".to_string();
            if c + p != 0 {
                percent = format!("{}%", 100 * c / (c + p));
            }
            view.set(row, 3, &percent);
        }

        let mut out = String::new();
        if view.rows() != 0 {
            out += optional_blank_line(context);
            out += &view.render();
            out += optional_blank_line(context);

            out += &format("{1} projects\n", &[&view.rows()]);
        } else {
            out += "No projects.\n";
            rc = 1;
        }

        *output = out;
        Ok(rc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    fn run(seed: &[&str], rc_lines: &str, args: &[&str]) -> (i32, String) {
        let mut f = CommandFixture::new(seed, &format!("defaultwidth=80\n{rc_lines}"), args);
        let mut output = String::new();
        let rc = CmdSummary::new().execute(&mut f.ctx, &mut output).unwrap();
        (rc, output)
    }

    /// Completed tasks only, so the average ages do not depend on "now".
    const SEED: [&str; 3] = [
        r#"{"description":"x","project":"a.b","status":"completed","entry":"20150101T000000Z","end":"20150111T000000Z"}"#,
        r#"{"description":"y","project":"a","status":"completed","entry":"20150101T000000Z","end":"20150103T000000Z"}"#,
        r#"{"description":"z","status":"completed","entry":"20150101T000000Z","end":"20150102T000000Z"}"#,
    ];

    #[test]
    fn no_pending_projects() {
        let (rc, out) = run(&SEED, "", &["summary"]);
        assert_eq!((rc, out.as_str()), (1, "No projects.\n"));
    }

    #[test]
    fn all_projects_tree_with_parents() {
        let (rc, out) = run(&SEED, "summary.all.projects=1\n", &["summary"]);
        assert_eq!(rc, 0);
        let expected = "\
Project Remaining Avg age Complete 0%                        100%
------- --------- ------- -------- ------------------------------
(none)          0      1d     100% ==============================
a               0      6d     100% ==============================
  b             0     10d     100% ==============================
3 projects
";
        assert_eq!(out, expected);
    }

    #[test]
    fn missing_parent_is_listed_and_bars_scale() {
        let seed = [
            r#"{"description":"p","project":"m.n"}"#,
            r#"{"description":"c","project":"m.n","status":"completed","entry":"20150101T000000Z","end":"20150102T000000Z"}"#,
            r#"{"description":"d","project":"m.n","status":"completed","entry":"20150101T000000Z","end":"20150102T000000Z"}"#,
        ];
        let (rc, out) = run(&seed, "", &["summary"]);
        assert_eq!(rc, 0);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 5, "{out}");
        assert!(lines[2].starts_with("m               1 "), "{out}");
        assert!(lines[2].ends_with(" 66% ===================="), "{out}");
        assert!(lines[3].starts_with("  n             1 "), "{out}");
        assert_eq!(lines[4], "2 projects");
    }
}
