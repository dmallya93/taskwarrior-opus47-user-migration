//! Rust translation of `src/commands/CmdExport.cpp`.
//!
//! `task <filter> export [<report>]`: JSON export of the filtered tasks.
//!
//! With one miscellaneous word, the first report-category command (in
//! registry order) that `closeEnough` matches it supplies
//! `report.<name>.sort` and `report.<name>.filter`; the report filter is
//! added to the command-line filter via `CLI2::addFilter`. Sort order `none`
//! keeps the command line's UUID order (`CLI2::_uuid_list`); no sort order
//! means `id,uuid`. `limit:N` is obeyed, `rc.json.array` wraps the output in
//! `[`/`]`, and every task is `composeJSON (true)` (decorated with `id` and
//! `urgency`). Taskwarrior 3.5.0 has no `json.depends.array` setting (it was
//! removed before 3.0): `composeJSON` always emits `depends` as an array, so
//! the setting is ignored here too.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::filter::Filter;
use crate::legacy::legacy_sort_column_map;
use crate::shared::format::format;
use crate::shared::shared::{close_enough, split};
use crate::shared::timer::Timer;
use crate::sort::sort_tasks;
use crate::task::Task;

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdExport` (`export`).
pub struct CmdExport {
    meta: CommandMeta,
}

impl CmdExport {
    /// C++ `CmdExport::CmdExport`.
    pub fn new() -> Self {
        CmdExport {
            meta: CommandMeta {
                keyword: "export".into(),
                usage: "task <filter> export [<report>]".into(),
                description: "Exports tasks in JSON format".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: true,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Migration,
                ..CommandMeta::default()
            },
        }
    }

    /// C++ `CmdExport::validateSortColumns`: maps (and complains about)
    /// legacy sort column names via `legacySortColumnMap`. As in C++, the
    /// mapped names are not used afterwards (the raw `report.<name>.sort`
    /// string goes to `sort_tasks`).
    fn validate_sort_columns(context: &mut Context, columns: &mut [String]) {
        for col in columns.iter_mut() {
            legacy_sort_column_map(context, col);
        }
    }
}

impl Default for CmdExport {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdExport {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdExport::execute`.
    ///
    /// # Errors
    /// `Unable to find report that matches '<word>'.`, and those of
    /// `CLI2::addFilter`, `Filter::subset`, `sort_tasks` and `composeJSON`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let rc = 0;

        let words = context.cli2.get_words(&context.config, &mut context.debug_messages);
        let mut selected_report = String::new();

        if words.len() == 1 {
            // Find the report matching the prompt
            for command in context.commands.values() {
                if command.category() == Category::Report && close_enough(command.keyword(), &words[0], 0) {
                    selected_report = command.keyword().to_string();
                    break;
                }
            }

            if selected_report.is_empty() {
                return Err(TwError::Message(format(
                    "Unable to find report that matches '{1}'.",
                    &[&words[0]],
                )));
            }
        }

        let mut report_sort = context.config.get(&format!("report.{selected_report}.sort"));
        let report_filter = context.config.get(&format!("report.{selected_report}.filter"));

        let mut sort_order = split(&report_sort, ',');
        if !sort_order.is_empty() && sort_order[0] != "none" {
            Self::validate_sort_columns(context, &mut sort_order);
        }

        // Add the report filter to any existing filter.
        if !report_filter.is_empty() {
            crate::cli2::with_cli2(context, |cli2, ctx| cli2.add_filter(ctx, &report_filter))?;
        }

        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        let mut sequence: Vec<usize> = Vec::new();
        if !sort_order.is_empty() && sort_order[0] == "none" {
            // Assemble a sequence vector that represents the tasks listed in
            // Context::getContext ().cli2._uuid_ranges, in the order in which they appear. This
            // equates to no sorting, just a specified order.
            sort_order.clear();
            for i in &context.cli2.uuid_list {
                for (t, task) in filtered.iter().enumerate() {
                    if task.get_ref("uuid") == i {
                        sequence.push(t);
                    }
                }
            }
        } else {
            // sort_tasks requires the order array initially be identity
            sequence.extend(0..filtered.len());

            // if no sort order, sort by id
            if sort_order.is_empty() {
                report_sort = "id,uuid".to_string();
            }

            // Sort the tasks.
            sort_tasks(context, &mut filtered, &mut sequence, &report_sort)?;
        }

        // Export == render.
        let timer = Timer::new();

        // Obey 'limit:N'.
        let (rows, lines) = context.get_limits();
        let limit = if rows > lines { rows } else { lines };

        // Is output contained within a JSON array?
        let json_array = context.config.get_boolean("json.array");

        // Compose output.
        if json_array {
            output.push_str("[\n");
        }

        let mut counter: i32 = 0;
        for &t in &sequence {
            let mut task = filtered[t].clone();
            if counter != 0 {
                if json_array {
                    output.push(',');
                }
                output.push('\n');
            }

            output.push_str(&task.compose_json(context, true)?);

            counter += 1;
            if limit != 0 && counter >= limit {
                break;
            }
        }

        if !filtered.is_empty() {
            output.push('\n');
        }

        if json_array {
            output.push_str("]\n");
        }

        context.time_render_us += timer.total_us() as i64;
        Ok(rc)
    }
}
