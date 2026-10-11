//! Rust translation of `src/commands/CmdTags.cpp`.
//!
//! Each struct below is one C++ class from `CmdTags.cpp`, carrying the exact
//! metadata of its C++ constructor.

use std::collections::BTreeMap;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::filter::Filter;
use crate::shared::color::Color;
use crate::shared::format::format;
use crate::shared::table::Table;
use crate::task::Task;
use crate::util::{optional_blank_line, set_header_underline};

/// The C++ `pending_tasks ()`, plus `completed_tasks ()` when the boolean
/// `setting` (`list.all.tags` / `complete.all.tags`) is on.
fn tag_candidates(context: &mut Context, setting: &str) -> Result<Vec<Task>> {
    // Get all the tasks.
    let mut tasks: Vec<Task> = context.tdb2.pending_tasks()?.to_vec();

    if context.config.get_boolean(setting) {
        for task in context.tdb2.completed_tasks()? {
            tasks.push(task.clone());
        }
    }
    Ok(tasks)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdTags` (`tags`).
pub struct CmdTags {
    meta: CommandMeta,
}

impl CmdTags {
    /// C++ `CmdTags::CmdTags`.
    pub fn new() -> Self {
        CmdTags {
            meta: CommandMeta {
                keyword: "tags".into(),
                usage: "task <filter> tags".into(),
                description: "Shows a list of all tags used".into(),
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

impl Default for CmdTags {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdTags {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdTags::execute`. As in C++, the "(N tasks)" footnote counts
    /// the unfiltered candidates, not the filtered ones.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut rc = 0;
        let mut out = String::new();

        // Get all the tasks.
        let tasks = tag_candidates(context, "list.all.tags")?;

        let quantity = tasks.len() as i32;

        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset_of(context, &tasks, &mut filtered)?;

        // Scan all the tasks for their project name, building a map using project
        // names as keys.
        let mut unique: BTreeMap<String, i32> = BTreeMap::new();
        for task in &filtered {
            for tag in task.get_tags() {
                *unique.entry(tag).or_insert(0) += 1;
            }
        }

        if !unique.is_empty() {
            // Render a list of tags names from the map.
            let mut view = Table::new();
            view.width(context.get_width());
            view.add("Tag");
            view.add_with("Count", false, true);
            set_header_underline(context, &mut view)?;

            let mut bold = Color::new();
            if context.color() {
                bold = Color::parse("bold")?;
            }

            for (tag, count) in &unique {
                // Highlight the special tags.
                let special = context.color()
                    && (tag == "nocolor" || tag == "nonag" || tag == "nocal" || tag == "next");

                let row = view.add_row();
                let color = if special { bold } else { Color::new() };
                view.set_colored(row, 0, tag, color);
                view.set_int_colored(row, 1, *count, color);
            }

            out += optional_blank_line(context);
            out += &view.render();
            out += optional_blank_line(context);

            if unique.len() == 1 {
                context.footnote("1 tag");
            } else {
                context.footnote(&format("{1} tags", &[&unique.len()]));
            }

            if quantity == 1 {
                context.footnote("(1 task)");
            } else {
                context.footnote(&format("({1} tasks)", &[&quantity]));
            }

            out.push('\n');
        } else {
            context.footnote("No tags.");
            rc = 1;
        }

        *output = out;
        Ok(rc)
    }
}

/// The built-in tags `CmdCompletionTags::execute` adds to the map, in the C++
/// order. `CHILD` and `PARENT` (2017-01-07) and `DUETODAY` (2016-03-29) are
/// deprecated since 2.6.0 but still listed.
const BUILTIN_TAGS: [&str; 36] = [
    "nocolor",
    "nonag",
    "nocal",
    "next",
    "ACTIVE",
    "ANNOTATED",
    "BLOCKED",
    "BLOCKING",
    "CHILD",
    "COMPLETED",
    "DELETED",
    "DUE",
    "DUETODAY",
    "INSTANCE",
    "LATEST",
    "MONTH",
    "ORPHAN",
    "OVERDUE",
    "PARENT",
    "PENDING",
    "PRIORITY",
    "PROJECT",
    "QUARTER",
    "READY",
    "SCHEDULED",
    "TAGGED",
    "TEMPLATE",
    "TODAY",
    "TOMORROW",
    "UDA",
    "UNBLOCKED",
    "UNTIL",
    "WAITING",
    "WEEK",
    "YEAR",
    "YESTERDAY",
];

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionTags` (`_tags`).
pub struct CmdCompletionTags {
    meta: CommandMeta,
}

impl CmdCompletionTags {
    /// C++ `CmdCompletionTags::CmdCompletionTags`.
    pub fn new() -> Self {
        CmdCompletionTags {
            meta: CommandMeta {
                keyword: "_tags".into(),
                usage: "task <filter> _tags".into(),
                description: "Shows only a list of all tags used, for autocompletion purposes"
                    .into(),
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

impl Default for CmdCompletionTags {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionTags {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionTags::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Get all the tasks.
        let tasks = tag_candidates(context, "complete.all.tags")?;

        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset_of(context, &tasks, &mut filtered)?;

        // Scan all the tasks for their tags, building a map using tag
        // names as keys.
        let mut unique: BTreeMap<String, i32> = BTreeMap::new();
        for task in &filtered {
            for tag in task.get_tags() {
                unique.insert(tag, 0);
            }
        }

        // Add built-in tags to map.
        for tag in BUILTIN_TAGS {
            unique.insert(tag.to_string(), 0);
        }

        // If you update the above list, update src/commands/CmdInfo.cpp and src/Task.cpp as well.

        let mut out = String::new();
        for tag in unique.keys() {
            out += tag;
            out.push('\n');
        }

        *output = out;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    fn run(
        command: &dyn Command,
        seed: &[&str],
        rc_lines: &str,
        args: &[&str],
    ) -> (i32, String, Vec<String>) {
        let mut f = CommandFixture::new(
            seed,
            &format!("defaultwidth=80\nfontunderline=0\n{rc_lines}"),
            args,
        );
        let mut output = String::new();
        let rc = command.execute(&mut f.ctx, &mut output).unwrap();
        (rc, output, f.ctx.footnotes.clone())
    }

    const SEED: [&str; 3] = [
        r#"{"description":"x","tags":["foo","next"]}"#,
        r#"{"description":"y","tags":["foo"]}"#,
        r#"{"description":"c","tags":["old"],"status":"completed","end":"20150102T000000Z"}"#,
    ];

    #[test]
    fn tags_table_and_footnotes() {
        let (rc, out, notes) = run(&CmdTags::new(), &SEED, "", &["tags"]);
        assert_eq!(rc, 0);
        assert_eq!(out, "Tag  Count\n---- -----\nfoo      2\nnext     1\n\n");
        assert_eq!(notes, ["2 tags", "(2 tasks)"]);
    }

    #[test]
    fn tags_quantity_counts_unfiltered_tasks() {
        let (rc, out, notes) = run(&CmdTags::new(), &SEED, "list.all.tags=1\n", &["+old", "tags"]);
        assert_eq!(rc, 0);
        assert_eq!(out, "Tag Count\n--- -----\nold     1\n\n");
        assert_eq!(notes, ["1 tag", "(3 tasks)"]);
    }

    #[test]
    fn no_tags_is_a_footnote_and_rc_1() {
        let seed = [r#"{"description":"x"}"#];
        let (rc, out, notes) = run(&CmdTags::new(), &seed, "", &["tags"]);
        assert_eq!((rc, out.as_str()), (1, ""));
        assert_eq!(notes, ["No tags."]);
    }

    #[test]
    fn completion_tags_merges_builtin_tags_in_map_order() {
        let (rc, out, _) = run(&CmdCompletionTags::new(), &SEED, "", &["_tags"]);
        assert_eq!(rc, 0);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 37);
        assert_eq!(lines[0], "ACTIVE");
        assert_eq!(lines[31], "YESTERDAY");
        assert_eq!(&lines[32..], ["foo", "next", "nocal", "nocolor", "nonag"]);
        assert!(!out.contains("old"));

        let (_, out, _) = run(&CmdCompletionTags::new(), &SEED, "complete.all.tags=1\n", &["_tags"]);
        assert!(out.contains("\nold\n"), "{out}");
    }
}
