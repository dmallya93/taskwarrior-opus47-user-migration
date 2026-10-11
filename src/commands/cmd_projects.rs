//! Rust translation of `src/commands/CmdProjects.cpp`.
//!
//! Each struct below is one C++ class from `CmdProjects.cpp`, carrying the exact
//! metadata of its C++ constructor.

use std::collections::BTreeMap;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::filter::Filter;
use crate::shared::format::format;
use crate::shared::table::Table;
use crate::sort::sort_projects;
use crate::task::{Status, Task};
use crate::util::{
    extract_parents_default, indent_project, optional_blank_line, set_header_underline,
};

/// The C++ `pending_tasks ()`, plus `completed_tasks ()` when
/// `list.all.projects` is on: the input set of both project commands.
fn project_candidates(context: &mut Context) -> Result<Vec<Task>> {
    // Get all the tasks.
    let mut tasks: Vec<Task> = context.tdb2.pending_tasks()?.to_vec();

    if context.config.get_boolean("list.all.projects") {
        for task in context.tdb2.completed_tasks()? {
            tasks.push(task.clone());
        }
    }
    Ok(tasks)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdProjects` (`projects`).
pub struct CmdProjects {
    meta: CommandMeta,
}

impl CmdProjects {
    /// C++ `CmdProjects::CmdProjects`.
    pub fn new() -> Self {
        CmdProjects {
            meta: CommandMeta {
                keyword: "projects".into(),
                usage: "task <filter> projects".into(),
                description: "Shows all project names used".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
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

impl Default for CmdProjects {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdProjects {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdProjects::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut rc = 0;

        // Get all the tasks.
        let tasks = project_candidates(context)?;

        // Apply the filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset_of(context, &tasks, &mut filtered)?;
        let mut quantity = filtered.len() as i32;

        let mut out = String::new();

        // Scan all the tasks for their project name, building a map using project
        // names as keys.
        let mut unique: BTreeMap<String, i32> = BTreeMap::new();
        let mut no_project = false;
        for task in &filtered {
            if task.get_status()? == Status::Deleted {
                quantity -= 1;
                continue;
            }

            // Increase the count for the project the task belongs to and all
            // its super-projects
            let project = task.get("project");

            let mut projects = extract_parents_default(&project);
            projects.push(project.clone());

            for parent in projects {
                *unique.entry(parent).or_insert(0) += 1;
            }

            if project.is_empty() {
                no_project = true;
            }
        }

        if !unique.is_empty() {
            // Render a list of project names from the map.
            let mut view = Table::new();
            view.width(context.get_width());
            view.add("Project");
            view.add_with("Tasks", false, true);
            view.with_color(context.color());
            set_header_underline(context, &mut view)?;

            // create sorted list of table entries
            let mut sorted_view: Vec<(String, i32)> = Vec::new();
            sort_projects(&mut sorted_view, &unique);

            // construct view from sorted list
            for (name, count) in &sorted_view {
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
                view.set_int(row, 1, *count);
            }

            let mut number_projects = unique.len() as i32;
            if no_project {
                number_projects -= 1;
            }

            out += optional_blank_line(context);
            out += &view.render();
            out += optional_blank_line(context);
            out += &(if number_projects == 1 {
                format("{1} project", &[&number_projects])
            } else {
                format("{1} projects", &[&number_projects])
            });
            out.push(' ');
            out += &(if quantity == 1 {
                format("({1} task)", &[&quantity])
            } else {
                format("({1} tasks)", &[&quantity])
            });
            out.push('\n');
        } else {
            out += "No projects.\n";
            rc = 1;
        }

        *output = out;
        Ok(rc)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionProjects` (`_projects`).
pub struct CmdCompletionProjects {
    meta: CommandMeta,
}

impl CmdCompletionProjects {
    /// C++ `CmdCompletionProjects::CmdCompletionProjects`.
    pub fn new() -> Self {
        CmdCompletionProjects {
            meta: CommandMeta {
                keyword: "_projects".into(),
                usage: "task <filter> _projects".into(),
                description: "Shows only a list of all project names used".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionProjects {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionProjects {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionProjects::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Get all the tasks.
        let tasks = project_candidates(context)?;

        // Apply the filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset_of(context, &tasks, &mut filtered)?;

        // Scan all the tasks for their project name, building a map using project
        // names as keys.
        let mut unique: BTreeMap<String, i32> = BTreeMap::new();
        for task in &filtered {
            unique.insert(task.get("project"), 0);
        }

        for project in unique.keys() {
            if !project.is_empty() {
                *output += project;
                output.push('\n');
            }
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    fn run(command: &dyn Command, seed: &[&str], rc_lines: &str, args: &[&str]) -> (i32, String) {
        let mut f = CommandFixture::new(seed, &format!("defaultwidth=80\n{rc_lines}"), args);
        let mut output = String::new();
        let rc = command.execute(&mut f.ctx, &mut output).unwrap();
        (rc, output)
    }

    const SEED: [&str; 5] = [
        r#"{"description":"x","project":"a.b"}"#,
        r#"{"description":"y","project":"a"}"#,
        r#"{"description":"z"}"#,
        r#"{"description":"c","project":"c","status":"completed","end":"20150102T000000Z"}"#,
        r#"{"description":"d","project":"d","status":"deleted","end":"20150102T000000Z"}"#,
    ];

    #[test]
    fn projects_tree_counts_and_footer() {
        let (rc, out) = run(&CmdProjects::new(), &SEED, "", &["projects"]);
        assert_eq!(rc, 0);
        let expected = "\
Project Tasks
------- -----
(none)      1
a           2
  b         1
2 projects (3 tasks)
";
        assert_eq!(out, expected);
    }

    #[test]
    fn projects_list_all_includes_completed() {
        let (rc, out) = run(&CmdProjects::new(), &SEED, "list.all.projects=1\n", &["projects"]);
        assert_eq!(rc, 0);
        assert!(out.contains("\nc           1\n"), "{out}");
        // The deleted task is skipped and not counted.
        assert!(out.ends_with("\n3 projects (4 tasks)\n"), "{out}");
    }

    #[test]
    fn projects_singular_footer() {
        let seed = [r#"{"description":"x","project":"solo"}"#];
        let (rc, out) = run(&CmdProjects::new(), &seed, "", &["projects"]);
        assert_eq!(rc, 0);
        assert!(out.ends_with("\n1 project (1 task)\n"), "{out}");
    }

    #[test]
    fn projects_filter_and_none() {
        let (rc, out) = run(&CmdProjects::new(), &SEED, "", &["project:nomatch", "projects"]);
        assert_eq!((rc, out.as_str()), (1, "No projects.\n"));
    }

    #[test]
    fn completion_projects_lists_names_without_counts() {
        let (rc, out) = run(&CmdCompletionProjects::new(), &SEED, "", &["_projects"]);
        assert_eq!((rc, out.as_str()), (0, "a\na.b\n"));

        let (rc, out) =
            run(&CmdCompletionProjects::new(), &SEED, "list.all.projects=1\n", &["_projects"]);
        assert_eq!((rc, out.as_str()), (0, "a\na.b\nc\nd\n"));
        // (As in C++, `_projects` does not skip the deleted task `d`.)
    }
}
