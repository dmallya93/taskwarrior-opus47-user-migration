//! Rust translation of `src/commands/CmdCustom.cpp`.
//!
//! [`CmdCustom`] is the C++ class `CmdCustom`: one instance per custom
//! report (`report.<name>.columns`), constructed by
//! [`factory`](crate::commands::command::factory) with the C++ keyword, usage
//! and description arguments. `execute` renders the report through
//! [`ViewTask`] after `Filter::subset` and `sort_tasks`.

use crate::cli2;
use crate::columns::column;
use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::feedback::feedback_backlog;
use crate::filter::Filter;
use crate::legacy::{legacy_column_map, legacy_sort_column_map};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::fs::Path;
use crate::shared::shared::split;
use crate::sort::sort_tasks;
use crate::task::Task;
use crate::util::optional_blank_line;
use crate::version::Version;
use crate::view_task::ViewTask;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCustom` (a custom report).
///
/// The C++ class also overrides `uses_context` to honour
/// `report.<name>.context` ([`Command::uses_context`]).
pub struct CmdCustom {
    meta: CommandMeta,
}

impl CmdCustom {
    /// C++ `CmdCustom::CmdCustom (keyword, usage, description)`.
    pub fn new(keyword: &str, usage: &str, description: &str) -> Self {
        CmdCustom {
            meta: CommandMeta {
                keyword: keyword.into(),
                usage: usage.into(),
                description: description.into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Report,
                ..CommandMeta::default()
            },
        }
    }
}

impl Command for CmdCustom {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCustom::uses_context`: whether a report uses context is
    /// defined by the `report.<name>.context` configuration variable.
    fn uses_context(&self, config: &Configuration) -> bool {
        let key = format!("report.{}.context", self.meta.keyword);

        if config.has(&key) {
            config.get_boolean(&key)
        } else {
            self.meta.uses_context
        }
    }

    /// C++ `CmdCustom::execute`: renders the report defined by
    /// `report.<keyword>.{columns,labels,sort,filter}`.
    ///
    /// Returns 1 (with the `No matches.` footnote) when no task matches.
    ///
    /// # Errors
    /// `There are different numbers of columns and labels for report '{1}'.`,
    /// and the errors of `CLI2::addFilter`, `Filter::subset`, `sort_tasks`,
    /// `Column::factory`, `Color` parsing and `ViewTask::render`.
    ///
    /// Deviation: C++ `CmdNews::should_nag` lives in `CmdNews.cpp`, which is
    /// ported in milestone 8; [`should_nag`] reproduces it here from the
    /// `NewsItem::all ()` version list.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut rc = 0;
        let keyword = self.meta.keyword.clone();

        // Load report configuration.
        let report_columns = context.config.get(&format!("report.{keyword}.columns"));
        let report_labels = context.config.get(&format!("report.{keyword}.labels"));
        let report_sort = context.config.get(&format!("report.{keyword}.sort"));
        let report_filter = context.config.get(&format!("report.{keyword}.filter"));

        let mut columns = split(&report_columns, ',');
        validate_report_columns(context, &mut columns);

        let labels = split(&report_labels, ',');

        if columns.len() != labels.len() && !labels.is_empty() {
            return Err(TwError::Message(format(
                "There are different numbers of columns and labels for report '{1}'.",
                &[&keyword],
            )));
        }

        let mut sort_order = split(&report_sort, ',');
        if !sort_order.is_empty() && sort_order[0] != "none" {
            validate_sort_columns(context, &mut sort_order);
        }

        // Add the report filter to any existing filter.
        if !report_filter.is_empty() {
            cli2::with_cli2(context, |cli2, ctx| cli2.add_filter(ctx, &report_filter))?;
        }

        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        let mut sequence: Vec<usize> = Vec::new();
        if !sort_order.is_empty() && sort_order[0] == "none" {
            // Assemble a sequence vector that represents the tasks listed in
            // cli2._uuid_ranges, in the order in which they appear. This
            // equates to no sorting, just a specified order.
            sort_order.clear();
            for i in &context.cli2.uuid_list {
                for (t, task) in filtered.iter().enumerate() {
                    if task.get("uuid") == *i {
                        sequence.push(t);
                    }
                }
            }
        } else {
            // There is a sortOrder, so sorting will take place, which means the
            // initial order of sequence is ascending.
            sequence.extend(0..filtered.len());

            // Sort the tasks.
            if !sort_order.is_empty() {
                sort_tasks(context, &mut filtered, &mut sequence, &report_sort)?;
            }
        }

        // Configure the view.
        let mut view = ViewTask::new();
        view.width(context.get_width());
        view.left_margin(context.config.get_integer("indent.report"));
        view.extra_padding(context.config.get_integer("row.padding"));
        view.intra_padding(context.config.get_integer("column.padding"));

        if context.color() {
            let label = Color::parse(&context.config.get("color.label"))?;
            view.color_header(&label);

            let label_sort = Color::parse(&context.config.get("color.label.sort"))?;
            view.color_sort_header(&label_sort);

            // If an alternating row color is specified, notify the table.
            let alternate = Color::parse(&context.config.get("color.alternate"))?;
            if alternate.nontrivial() {
                view.color_odd(&alternate);
                view.intra_color_odd(&alternate);
            }
        }

        // Capture columns that are sorted.
        let mut sort_columns: Vec<String> = Vec::new();

        // Add the break columns, if any.
        for so in &sort_order {
            let (name, _ascending, break_indicator) = Context::decompose_sort_field(so);

            if break_indicator {
                view.add_break(&name);
            }

            sort_columns.push(name);
        }

        // Add the columns and labels.
        for (i, column) in columns.iter().enumerate() {
            let mut c = column::factory(column, &keyword, &context.config)?;
            if i < labels.len() {
                c.set_label(&labels[i]);
            }

            let sort = sort_columns.iter().any(|s| s == c.name());

            view.add(c, sort);
        }

        // How many lines taken up by table header?
        let mut table_header = 0;
        if context.verbose("label") {
            if context.color() && context.config.get_boolean("fontunderline") {
                table_header = 1; // Underlining doesn't use extra line.
            } else {
                table_header = 2; // Dashes use an extra line.
            }
        }

        // Report output can be limited by rows or lines.
        let (maxrows, mut maxlines) = context.get_limits();

        // Adjust for fluff in the output.
        if maxlines != 0 {
            let blank = i32::from(context.verbose("blank"));
            let footnotes = if context.verbose("footnote") {
                context.footnotes.len() as i32
            } else {
                0
            };
            let affected = i32::from(context.verbose("affected"));
            maxlines -= table_header
                + blank
                + footnotes
                + affected
                + context.config.get_integer("reserved.lines"); // For prompt, etc.
        }

        // Render.
        let mut out = String::new();
        if !filtered.is_empty() {
            view.truncate_rows(maxrows);
            view.truncate_lines(maxlines);

            out.push_str(optional_blank_line(context));
            let rendered = view.render(context, &mut filtered, &sequence)?;
            out.push_str(&rendered);
            out.push_str(optional_blank_line(context));

            // Print the number of rendered tasks
            if context.verbose("affected") {
                if filtered.len() == 1 {
                    out.push_str("1 task");
                } else {
                    out.push_str(&format("{1} tasks", &[&filtered.len()]));
                }

                if maxrows != 0 && (maxrows as i64) < filtered.len() as i64 {
                    out.push_str(", ");
                    out.push_str(&format("{1} shown", &[&maxrows]));
                }

                if maxlines != 0 && (maxlines as i64) < filtered.len() as i64 {
                    out.push_str(", ");
                    out.push_str(&format(
                        "truncated to {1} lines",
                        &[&(maxlines - table_header)],
                    ));
                }

                out.push('\n');
            }
        } else {
            context.footnote("No matches.");
            rc = 1;
        }

        // Inform user about the new release highlights if not presented yet
        if should_nag(context) {
            let notice = format!(
                "Recently upgraded to {}. Please run 'task news' to read highlights about the new release.",
                Version::current()
            );
            if context.verbose("footnote") {
                context.footnote(&notice);
            } else if context.verbose("header") {
                context.header(&notice);
            }
        }

        let location = context.data_dir.data.clone();
        let pending_data = Path::new(&format!("{location}/pending.data"));
        if pending_data.exists() {
            let warning = Color::parse(&context.config.get("color.warning"))?;
            eprintln!(
                "{}",
                warning.colorize(&format(
                    "Found existing '*.data' files in {1}",
                    &[&location]
                ))
            );
            eprintln!(
                "  Taskwarrior's storage format changed in 3.0, requiring a manual migration."
            );
            eprintln!(
                "  See https://taskwarrior.org/docs/upgrade-3/. Run `task import-v2` to import"
            );
            eprintln!("  the tasks into the Taskwarrior-3.x format");
        }

        feedback_backlog(context)?;
        *output = out;
        Ok(rc)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCustom::validateReportColumns`: maps legacy column names in place.
fn validate_report_columns(context: &mut Context, columns: &mut [String]) {
    for col in columns.iter_mut() {
        legacy_column_map(context, col);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCustom::validateSortColumns`: maps legacy sort column names in
/// place.
fn validate_sort_columns(context: &mut Context, columns: &mut [String]) {
    for col in columns.iter_mut() {
        legacy_sort_column_map(context, col);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// The versions of the C++ `NewsItem::all ()` items (`version2_6_0` …
/// `version3_5_0`); every one of those functions adds at least one item.
const NEWS_ITEM_VERSIONS: [&str; 7] = [
    "2.6.0", "3.0.0", "3.1.0", "3.2.0", "3.3.0", "3.4.0", "3.5.0",
];

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdNews::should_nag`: whether to remind the user to read
/// `task news` (the `news` verbosity token is on and `news.version` is older
/// than the current version, with news items newer than it).
///
/// Deviation: lives here until `CmdNews.cpp` is ported (milestone 8);
/// `NewsItem::all ()` is reduced to the versions of its items.
pub fn should_nag(context: &mut Context) -> bool {
    if !context.verbose("news") {
        return false;
    }

    let mut news_version = Version::new(&context.config.get("news.version"));
    if !news_version.is_valid() {
        news_version = Version::new("2.6.0");
    }

    let current_version = Version::current();

    if news_version >= current_version {
        return false;
    }

    // Check if there are actually any interesting news items to show.
    NEWS_ITEM_VERSIONS
        .iter()
        .any(|v| Version::new(v) > news_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(verbose: &str, news_version: &str) -> Context {
        let mut context = Context::new();
        context.config.set("verbose", verbose);
        context.config.set("news.version", news_version);
        context
    }

    #[test]
    fn should_nag_follows_news_version_and_verbosity() {
        // Older than the current release, with newer news items.
        assert!(should_nag(&mut context("news", "2.6.0")));
        assert!(should_nag(&mut context("news", "3.4.9")));
        // An invalid or empty version counts as 2.6.0.
        assert!(should_nag(&mut context("news", "")));
        assert!(should_nag(&mut context("news", "garbage")));
        // Up to date (or newer).
        assert!(!should_nag(&mut context("news", "3.5.0")));
        assert!(!should_nag(&mut context("news", "4.0.0")));
        // Without the 'news' verbosity token.
        assert!(!should_nag(&mut context("nothing", "2.6.0")));
        assert!(!should_nag(&mut context("affected", "2.6.0")));
    }

    #[test]
    fn uses_context_reads_report_context() {
        let command = CmdCustom::new("mine", "usage", "description");
        let mut config = Configuration::new();
        assert!(command.uses_context(&config));
        config.set("report.mine.context", "0");
        assert!(!command.uses_context(&config));
        config.set("report.mine.context", "1");
        assert!(command.uses_context(&config));
    }
}
