//! Rust translation of `src/commands/CmdHistory.cpp` / `CmdHistory.h`.
//!
//! The C++ `template <class HistoryStrategy> class CmdHistoryBase` becomes the
//! generic [`CmdHistoryBase<S>`] over the [`HistoryStrategy`] trait; each C++
//! strategy class (`DailyHistoryStrategy`, ...) is a unit struct implementing
//! it. Every C++ `typedef CmdHistoryBase<...> CmdHistory*` keeps its own
//! command struct with the exact C++ metadata, whose `execute` runs
//! `CmdHistoryBase::<Strategy>::execute`.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::filter::Filter;
use crate::shared::color::{Color, ColorId};
use crate::shared::datetime::Datetime;
use crate::shared::table::Table;
use crate::task::{Status, Task};
use crate::util::{optional_blank_line, set_header_underline};

const STRING_CMD_HISTORY_YEAR: &str = "Year";
const STRING_CMD_HISTORY_MONTH: &str = "Month";
const STRING_CMD_HISTORY_DAY: &str = "Day";
const STRING_CMD_HISTORY_ADDED: &str = "Added";
const STRING_CMD_HISTORY_COMP: &str = "Completed";
const STRING_CMD_HISTORY_DEL: &str = "Deleted";

////////////////////////////////////////////////////////////////////////////////
/// The static interface every C++ `*HistoryStrategy` class provides to
/// `CmdHistoryBase`.
pub trait HistoryStrategy {
    /// C++ `keyword`.
    const KEYWORD: &'static str;
    /// C++ `usage`.
    const USAGE: &'static str;
    /// C++ `description`.
    const DESCRIPTION: &'static str;
    /// C++ `dateFieldCount` (`unsigned int`).
    const DATE_FIELD_COUNT: u32;
    /// C++ `graphical`.
    const GRAPHICAL: bool;
    /// C++ `labelWidth` (`unsigned int`; 0 = unused for the tabular reports).
    const LABEL_WIDTH: u32;

    /// C++ `getRelevantDate`.
    fn get_relevant_date(dt: &Datetime) -> Datetime;
    /// C++ `setupTableDates`.
    fn setup_table_dates(view: &mut Table);
    /// C++ `insertRowDate`.
    fn insert_row_date(view: &mut Table, row: usize, row_time: i64, last_time: i64);
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHistoryBase<HistoryStrategy>` state: the per-interval counters
/// (`std::map<time_t, int>`, ordered) and `rc`.
pub struct CmdHistoryBase<S: HistoryStrategy> {
    /// Represents any timeinterval with data.
    groups: BTreeMap<i64, i32>,
    /// Additions by timeinterval.
    added_group: BTreeMap<i64, i32>,
    /// Completions by timeinterval.
    completed_group: BTreeMap<i64, i32>,
    /// Deletions by timeinterval.
    deleted_group: BTreeMap<i64, i32>,
    rc: i32,
    strategy: PhantomData<S>,
}

impl<S: HistoryStrategy> Default for CmdHistoryBase<S> {
    fn default() -> Self {
        Self::new()
    }
}

/// C++ `std::map<time_t, int>::operator[]`: inserts a zero entry when the
/// key is absent and returns the value.
fn at(map: &mut BTreeMap<i64, i32>, key: i64) -> i32 {
    *map.entry(key).or_insert(0)
}

/// C++ `std::string (n, ' ')` for `n = leftOffset - aBar.length ()`
/// (`unsigned int - size_t`). A negative difference wraps to a size above
/// `max_size ()`, so the C++ constructor throws `std::length_error`, which
/// `Context::run` reports through `catch (...)` ("Unknown error. Please
/// report.", status 3); returned here as [`TwError::Runtime`].
fn padding(left_offset: u32, bar_len: usize) -> Result<String> {
    if bar_len as u64 > left_offset as u64 {
        return Err(TwError::Runtime("basic_string::_M_create".into()));
    }
    Ok(" ".repeat(left_offset as usize - bar_len))
}

impl<S: HistoryStrategy> CmdHistoryBase<S> {
    /// Empty counters (the C++ members before `execute`).
    pub fn new() -> Self {
        CmdHistoryBase {
            groups: BTreeMap::new(),
            added_group: BTreeMap::new(),
            completed_group: BTreeMap::new(),
            deleted_group: BTreeMap::new(),
            rc: 0,
            strategy: PhantomData,
        }
    }

    /// C++ `CmdHistoryBase<HistoryStrategy>::outputGraphical`.
    ///
    /// The bar arithmetic keeps the C++ `unsigned int` types:
    /// `widthOfBar = getWidth () - labelWidth` and every `(widthOfBar * n) /
    /// maxLine` wrap modulo 2^32.
    fn output_graphical(&mut self, context: &mut Context, output: &mut String) -> Result<()> {
        let width_of_bar: u32 = (context.get_width() as u32).wrapping_sub(S::LABEL_WIDTH);

        // Now build the view.
        let mut view = Table::new();
        set_header_underline(context, &mut view)?;
        view.width(context.get_width());

        S::setup_table_dates(&mut view);

        view.add_with("Number Added/Completed/Deleted", true, false); // Fixed.

        let color_add = Color::parse(&context.config.get("color.history.add"))?;
        let color_done = Color::parse(&context.config.get("color.history.done"))?;
        let color_delete = Color::parse(&context.config.get("color.history.delete"))?;
        let _label = Color::parse(&context.config.get("color.label"))?;

        // Determine the longest line, and the longest "added" line.
        let mut max_added_line: i32 = 0;
        let mut max_removed_line: i32 = 0;
        let keys: Vec<i64> = self.groups.keys().copied().collect();
        for &key in &keys {
            let removed = at(&mut self.completed_group, key) + at(&mut self.deleted_group, key);
            if removed > max_removed_line {
                max_removed_line = removed;
            }

            let added = at(&mut self.added_group, key);
            if added > max_added_line {
                max_added_line = added;
            }
        }

        let max_line = max_added_line + max_removed_line;
        if max_line > 0 {
            let left_offset: u32 =
                width_of_bar.wrapping_mul(max_added_line as u32) / (max_line as u32);

            let mut prior_time: i64 = 0;
            for &key in &keys {
                let row = view.add_row();

                S::insert_row_date(&mut view, row, key, prior_time);
                prior_time = key;

                let added = at(&mut self.added_group, key);
                let completed = at(&mut self.completed_group, key);
                let deleted = at(&mut self.deleted_group, key);

                let added_bar: u32 = width_of_bar.wrapping_mul(added as u32) / (max_line as u32);
                let completed_bar: u32 =
                    width_of_bar.wrapping_mul(completed as u32) / (max_line as u32);
                let deleted_bar: u32 =
                    width_of_bar.wrapping_mul(deleted as u32) / (max_line as u32);

                let mut bar = String::new();
                if context.color() {
                    let mut a_bar = String::new();
                    if added != 0 {
                        a_bar = added.to_string();
                        while (a_bar.len() as u64) < added_bar as u64 {
                            a_bar.insert(0, ' ');
                        }
                    }

                    let mut c_bar = String::new();
                    if completed != 0 {
                        c_bar = completed.to_string();
                        while (c_bar.len() as u64) < completed_bar as u64 {
                            c_bar.insert(0, ' ');
                        }
                    }

                    let mut d_bar = String::new();
                    if deleted != 0 {
                        d_bar = deleted.to_string();
                        while (d_bar.len() as u64) < deleted_bar as u64 {
                            d_bar.insert(0, ' ');
                        }
                    }

                    bar += &padding(left_offset, a_bar.len())?;
                    bar += &color_add.colorize(&a_bar);
                    bar += &color_done.colorize(&c_bar);
                    bar += &color_delete.colorize(&d_bar);
                } else {
                    let a_bar = "+".repeat(added_bar as usize);
                    let c_bar = "X".repeat(completed_bar as usize);
                    let d_bar = "-".repeat(deleted_bar as usize);

                    bar += &padding(left_offset, a_bar.len())?;
                    bar += &a_bar;
                    bar += &c_bar;
                    bar += &d_bar;
                }

                view.set(row, S::DATE_FIELD_COUNT as usize, &bar);
            }
        }

        let mut out = String::new();
        if view.rows() != 0 {
            out += optional_blank_line(context);
            out += &view.render();
            out.push('\n');

            if context.color() {
                out += &format!(
                    "Legend: {}, {}, {}",
                    color_add.colorize(STRING_CMD_HISTORY_ADDED),
                    color_done.colorize(STRING_CMD_HISTORY_COMP),
                    color_delete.colorize(STRING_CMD_HISTORY_DEL)
                );
                out += optional_blank_line(context);
                out.push('\n');
            } else {
                out += "Legend: + Added, X Completed, - Deleted\n";
            }
        } else {
            context.footnote("No tasks.");
            self.rc = 1;
        }

        *output = out;
        Ok(())
    }

    /// C++ `CmdHistoryBase<HistoryStrategy>::outputTabular`.
    ///
    /// As in C++, `totalAdded += addedGroup[i.first]` (`operator[]`) inserts a
    /// zero entry first, so the following `find` checks always succeed and
    /// every Added/Completed/Deleted cell shows a number, `0` included.
    fn output_tabular(&mut self, context: &mut Context, output: &mut String) -> Result<()> {
        let mut view = Table::new();
        set_header_underline(context, &mut view)?;
        view.width(context.get_width());

        S::setup_table_dates(&mut view);

        view.add_with(STRING_CMD_HISTORY_ADDED, false, true);
        view.add_with(STRING_CMD_HISTORY_COMP, false, true);
        view.add_with(STRING_CMD_HISTORY_DEL, false, true);
        view.add_with("Net", false, true);

        let mut total_added: i32 = 0;
        let mut total_completed: i32 = 0;
        let mut total_deleted: i32 = 0;

        let dfc = S::DATE_FIELD_COUNT as usize;
        let mut row: usize;
        let mut last_time: i64 = 0;
        let keys: Vec<i64> = self.groups.keys().copied().collect();
        for &key in &keys {
            row = view.add_row();

            total_added += at(&mut self.added_group, key);
            total_completed += at(&mut self.completed_group, key);
            total_deleted += at(&mut self.deleted_group, key);

            S::insert_row_date(&mut view, row, key, last_time);
            last_time = key;

            let mut net: i32 = 0;

            if let Some(&added) = self.added_group.get(&key) {
                view.set_int(row, dfc, added);
                net += added;
            }

            if let Some(&completed) = self.completed_group.get(&key) {
                view.set_int(row, dfc + 1, completed);
                net -= completed;
            }

            if let Some(&deleted) = self.deleted_group.get(&key) {
                view.set_int(row, dfc + 2, deleted);
                net -= deleted;
            }

            let mut net_color = Color::new();
            if context.color() && net != 0 {
                net_color = if net > 0 {
                    Color::from_id(ColorId::Red)
                } else {
                    Color::from_id(ColorId::Green)
                };
            }

            view.set_int_colored(row, dfc + 3, net, net_color);
        }

        if view.rows() != 0 {
            row = view.add_row();
            view.set(row, 1, " ");
            row = view.add_row();

            let mut row_color = Color::new();
            if context.color() {
                row_color =
                    Color::from_ids(ColorId::Nocolor, ColorId::Nocolor, false, true, false);
            }

            view.set_colored(row, dfc - 1, "Average", row_color);
            view.set_int_colored(row, dfc, total_added / (view.rows() - 2), row_color);
            view.set_int_colored(row, dfc + 1, total_completed / (view.rows() - 2), row_color);
            view.set_int_colored(row, dfc + 2, total_deleted / (view.rows() - 2), row_color);
            view.set_int_colored(
                row,
                dfc + 3,
                (total_added - total_completed - total_deleted) / (view.rows() - 2),
                row_color,
            );
        }

        let mut out = String::new();
        if view.rows() != 0 {
            out += optional_blank_line(context);
            out += &view.render();
            out.push('\n');
        } else {
            context.footnote("No tasks.");
            self.rc = 1;
        }

        *output = out;
        Ok(())
    }

    /// C++ `CmdHistoryBase<HistoryStrategy>::execute`.
    pub fn execute(&mut self, context: &mut Context, output: &mut String) -> Result<i32> {
        self.rc = 0;

        // Reset the counters (the C++ clears them, asking whether that is necessary).
        self.groups.clear();
        self.added_group.clear();
        self.deleted_group.clear();
        self.completed_group.clear();

        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        for task in &filtered {
            let entry = Datetime::from_epoch(task.get_date("entry"));

            let mut end = Datetime::new();
            if task.has("end") {
                end = Datetime::from_epoch(task.get_date("end"));
            }

            let mut epoch = S::get_relevant_date(&entry).to_epoch();
            self.groups.insert(epoch, 0);

            // Every task has an entry date, but exclude templates.
            let status = task.get_status()?;
            if status != Status::Recurring {
                *self.added_group.entry(epoch).or_insert(0) += 1;
            }

            // All deleted tasks have an end date.
            if status == Status::Deleted {
                epoch = S::get_relevant_date(&end).to_epoch();
                self.groups.insert(epoch, 0);
                *self.deleted_group.entry(epoch).or_insert(0) += 1;
            }
            // All completed tasks have an end date.
            else if status == Status::Completed {
                epoch = S::get_relevant_date(&end).to_epoch();
                self.groups.insert(epoch, 0);
                *self.completed_group.entry(epoch).or_insert(0) += 1;
            }
        }

        // Now build the view.
        if S::GRAPHICAL {
            self.output_graphical(context, output)?;
        } else {
            self.output_tabular(context, output)?;
        }

        Ok(self.rc)
    }
}

////////////////////////////////////////////////////////////////////////////////
// `insertRowDate` bodies. The C++ repeats them verbatim in each strategy
// class; the three distinct bodies are written once here and each strategy
// calls its own.

/// C++ `{Monthly,MonthlyG}HistoryStrategy::insertRowDate`. The year is only
/// printed when it differs from the previous row's (for the first row,
/// `Datetime (0)`, i.e. 1970 in local time; no `lastTime == 0` check here).
fn insert_row_date_monthly(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
    let dt = Datetime::from_epoch(row_time);
    let (mut m, mut d, mut y) = (0, 0, 0);
    dt.to_ymd(&mut y, &mut m, &mut d);

    let last_dt = Datetime::from_epoch(last_time);
    let (mut last_m, mut last_d, mut last_y) = (0, 0, 0);
    last_dt.to_ymd(&mut last_y, &mut last_m, &mut last_d);

    if y != last_y {
        view.set_int(row, 0, y);
    }

    view.set(row, 1, &Datetime::month_name(m));
}

/// C++ `{Annual,AnnualG}HistoryStrategy::insertRowDate`.
fn insert_row_date_annual(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
    let dt = Datetime::from_epoch(row_time);
    let (mut m, mut d, mut y) = (0, 0, 0);
    dt.to_ymd(&mut y, &mut m, &mut d);

    let last_dt = Datetime::from_epoch(last_time);
    let (mut last_m, mut last_d, mut last_y) = (0, 0, 0);
    last_dt.to_ymd(&mut last_y, &mut last_m, &mut last_d);

    if y != last_y {
        view.set_int(row, 0, y);
    }
}

/// C++ `{Daily,DailyG,Weekly,WeeklyG}HistoryStrategy::insertRowDate`.
fn insert_row_date_daily(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
    let dt = Datetime::from_epoch(row_time);
    let (mut m, mut d, mut y) = (0, 0, 0);
    dt.to_ymd(&mut y, &mut m, &mut d);

    let last_dt = Datetime::from_epoch(last_time);
    let (mut last_m, mut last_d, mut last_y) = (0, 0, 0);
    last_dt.to_ymd(&mut last_y, &mut last_m, &mut last_d);

    let y_changed = (y != last_y) || (last_time == 0);
    let m_changed = (m != last_m) || (last_time == 0);

    if y_changed {
        view.set_int(row, 0, y);
    }

    if y_changed || m_changed {
        view.set(row, 1, &Datetime::month_name(m));
    }

    view.set_int(row, 2, d);
}

/// C++ `setupTableDates` of the Year/Month strategies.
fn setup_year_month(view: &mut Table) {
    view.add_with(STRING_CMD_HISTORY_YEAR, true, true);
    view.add_with(STRING_CMD_HISTORY_MONTH, true, true);
}

/// C++ `setupTableDates` of the Year/Month/Day strategies.
fn setup_year_month_day(view: &mut Table) {
    view.add_with(STRING_CMD_HISTORY_YEAR, true, true);
    view.add_with(STRING_CMD_HISTORY_MONTH, true, true);
    view.add_with(STRING_CMD_HISTORY_DAY, false, true);
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `MonthlyHistoryStrategy`.
pub struct MonthlyHistoryStrategy;

impl HistoryStrategy for MonthlyHistoryStrategy {
    const KEYWORD: &'static str = "history.monthly";
    const USAGE: &'static str = "task <filter> history.monthly";
    const DESCRIPTION: &'static str = "Shows a report of task history, by month";
    const DATE_FIELD_COUNT: u32 = 2;
    const GRAPHICAL: bool = false;
    const LABEL_WIDTH: u32 = 0; // unused.

    fn get_relevant_date(dt: &Datetime) -> Datetime {
        dt.start_of_month()
    }
    fn setup_table_dates(view: &mut Table) {
        setup_year_month(view);
    }
    fn insert_row_date(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
        insert_row_date_monthly(view, row, row_time, last_time);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `MonthlyGHistoryStrategy`.
pub struct MonthlyGHistoryStrategy;

impl HistoryStrategy for MonthlyGHistoryStrategy {
    const KEYWORD: &'static str = "ghistory.monthly";
    const USAGE: &'static str = "task <filter> ghistory.monthly";
    const DESCRIPTION: &'static str = "Shows a graphical report of task history, by month";
    const DATE_FIELD_COUNT: u32 = 2;
    const GRAPHICAL: bool = true;
    const LABEL_WIDTH: u32 = 15; // length '2017 September ' = 15

    fn get_relevant_date(dt: &Datetime) -> Datetime {
        dt.start_of_month()
    }
    fn setup_table_dates(view: &mut Table) {
        setup_year_month(view);
    }
    fn insert_row_date(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
        insert_row_date_monthly(view, row, row_time, last_time);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `AnnualGHistoryStrategy`.
pub struct AnnualGHistoryStrategy;

impl HistoryStrategy for AnnualGHistoryStrategy {
    const KEYWORD: &'static str = "ghistory.annual";
    const USAGE: &'static str = "task <filter> ghistory.annual";
    const DESCRIPTION: &'static str = "Shows a graphical report of task history, by year";
    const DATE_FIELD_COUNT: u32 = 1;
    const GRAPHICAL: bool = true;
    const LABEL_WIDTH: u32 = 5; // length '2017 ' = 5

    fn get_relevant_date(dt: &Datetime) -> Datetime {
        dt.start_of_year()
    }
    fn setup_table_dates(view: &mut Table) {
        view.add_with(STRING_CMD_HISTORY_YEAR, true, true);
    }
    fn insert_row_date(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
        insert_row_date_annual(view, row, row_time, last_time);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `AnnualHistoryStrategy`.
pub struct AnnualHistoryStrategy;

impl HistoryStrategy for AnnualHistoryStrategy {
    const KEYWORD: &'static str = "history.annual";
    const USAGE: &'static str = "task <filter> history.annual";
    const DESCRIPTION: &'static str = "Shows a report of task history, by year";
    const DATE_FIELD_COUNT: u32 = 1;
    const GRAPHICAL: bool = false;
    const LABEL_WIDTH: u32 = 0; // unused.

    fn get_relevant_date(dt: &Datetime) -> Datetime {
        dt.start_of_year()
    }
    fn setup_table_dates(view: &mut Table) {
        view.add_with(STRING_CMD_HISTORY_YEAR, true, true);
    }
    fn insert_row_date(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
        insert_row_date_annual(view, row, row_time, last_time);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `DailyHistoryStrategy`.
pub struct DailyHistoryStrategy;

impl HistoryStrategy for DailyHistoryStrategy {
    const KEYWORD: &'static str = "history.daily";
    const USAGE: &'static str = "task <filter> history.daily";
    const DESCRIPTION: &'static str = "Shows a report of task history, by day";
    const DATE_FIELD_COUNT: u32 = 3;
    const GRAPHICAL: bool = false;
    const LABEL_WIDTH: u32 = 0; // unused.

    fn get_relevant_date(dt: &Datetime) -> Datetime {
        dt.start_of_day()
    }
    fn setup_table_dates(view: &mut Table) {
        setup_year_month_day(view);
    }
    fn insert_row_date(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
        insert_row_date_daily(view, row, row_time, last_time);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `DailyGHistoryStrategy`.
pub struct DailyGHistoryStrategy;

impl HistoryStrategy for DailyGHistoryStrategy {
    const KEYWORD: &'static str = "ghistory.daily";
    const USAGE: &'static str = "task <filter> ghistory.daily";
    const DESCRIPTION: &'static str = "Shows a graphical report of task history, by day";
    const DATE_FIELD_COUNT: u32 = 3;
    const GRAPHICAL: bool = true;
    const LABEL_WIDTH: u32 = 19; // length '2017 September Day ' = 19

    fn get_relevant_date(dt: &Datetime) -> Datetime {
        dt.start_of_day()
    }
    fn setup_table_dates(view: &mut Table) {
        setup_year_month_day(view);
    }
    fn insert_row_date(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
        insert_row_date_daily(view, row, row_time, last_time);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `WeeklyHistoryStrategy`.
pub struct WeeklyHistoryStrategy;

impl HistoryStrategy for WeeklyHistoryStrategy {
    const KEYWORD: &'static str = "history.weekly";
    const USAGE: &'static str = "task <filter> history.weekly";
    const DESCRIPTION: &'static str = "Shows a report of task history, by week";
    const DATE_FIELD_COUNT: u32 = 3;
    const GRAPHICAL: bool = false;
    const LABEL_WIDTH: u32 = 0; // unused.

    fn get_relevant_date(dt: &Datetime) -> Datetime {
        dt.start_of_week()
    }
    fn setup_table_dates(view: &mut Table) {
        setup_year_month_day(view);
    }
    fn insert_row_date(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
        insert_row_date_daily(view, row, row_time, last_time);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `WeeklyGHistoryStrategy`.
pub struct WeeklyGHistoryStrategy;

impl HistoryStrategy for WeeklyGHistoryStrategy {
    const KEYWORD: &'static str = "ghistory.weekly";
    const USAGE: &'static str = "task <filter> ghistory.weekly";
    const DESCRIPTION: &'static str = "Shows a graphical report of task history, by week";
    const DATE_FIELD_COUNT: u32 = 3;
    const GRAPHICAL: bool = true;
    const LABEL_WIDTH: u32 = 19; // length '2017 September Day ' = 19

    fn get_relevant_date(dt: &Datetime) -> Datetime {
        dt.start_of_week()
    }
    fn setup_table_dates(view: &mut Table) {
        setup_year_month_day(view);
    }
    fn insert_row_date(view: &mut Table, row: usize, row_time: i64, last_time: i64) {
        insert_row_date_daily(view, row, row_time, last_time);
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHistoryDaily` (`history.daily`).
pub struct CmdHistoryDaily {
    meta: CommandMeta,
}

impl CmdHistoryDaily {
    /// C++ `CmdHistoryBase<DailyHistoryStrategy>::CmdHistoryBase` (`typedef CmdHistoryDaily`).
    pub fn new() -> Self {
        CmdHistoryDaily {
            meta: CommandMeta {
                keyword: "history.daily".into(),
                usage: "task <filter> history.daily".into(),
                description: "Shows a report of task history, by day".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
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

impl Default for CmdHistoryDaily {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHistoryDaily {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryBase<DailyHistoryStrategy>::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        CmdHistoryBase::<DailyHistoryStrategy>::new().execute(context, output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGHistoryDaily` (`ghistory.daily`).
pub struct CmdGHistoryDaily {
    meta: CommandMeta,
}

impl CmdGHistoryDaily {
    /// C++ `CmdHistoryBase<DailyGHistoryStrategy>::CmdHistoryBase` (`typedef CmdGHistoryDaily`).
    pub fn new() -> Self {
        CmdGHistoryDaily {
            meta: CommandMeta {
                keyword: "ghistory.daily".into(),
                usage: "task <filter> ghistory.daily".into(),
                description: "Shows a graphical report of task history, by day".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
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

impl Default for CmdGHistoryDaily {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGHistoryDaily {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryBase<DailyGHistoryStrategy>::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        CmdHistoryBase::<DailyGHistoryStrategy>::new().execute(context, output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHistoryWeekly` (`history.weekly`).
pub struct CmdHistoryWeekly {
    meta: CommandMeta,
}

impl CmdHistoryWeekly {
    /// C++ `CmdHistoryBase<WeeklyHistoryStrategy>::CmdHistoryBase` (`typedef CmdHistoryWeekly`).
    pub fn new() -> Self {
        CmdHistoryWeekly {
            meta: CommandMeta {
                keyword: "history.weekly".into(),
                usage: "task <filter> history.weekly".into(),
                description: "Shows a report of task history, by week".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
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

impl Default for CmdHistoryWeekly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHistoryWeekly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryBase<WeeklyHistoryStrategy>::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        CmdHistoryBase::<WeeklyHistoryStrategy>::new().execute(context, output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGHistoryWeekly` (`ghistory.weekly`).
pub struct CmdGHistoryWeekly {
    meta: CommandMeta,
}

impl CmdGHistoryWeekly {
    /// C++ `CmdHistoryBase<WeeklyGHistoryStrategy>::CmdHistoryBase` (`typedef CmdGHistoryWeekly`).
    pub fn new() -> Self {
        CmdGHistoryWeekly {
            meta: CommandMeta {
                keyword: "ghistory.weekly".into(),
                usage: "task <filter> ghistory.weekly".into(),
                description: "Shows a graphical report of task history, by week".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
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

impl Default for CmdGHistoryWeekly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGHistoryWeekly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryBase<WeeklyGHistoryStrategy>::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        CmdHistoryBase::<WeeklyGHistoryStrategy>::new().execute(context, output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHistoryMonthly` (`history.monthly`).
pub struct CmdHistoryMonthly {
    meta: CommandMeta,
}

impl CmdHistoryMonthly {
    /// C++ `CmdHistoryBase<MonthlyHistoryStrategy>::CmdHistoryBase` (`typedef CmdHistoryMonthly`).
    pub fn new() -> Self {
        CmdHistoryMonthly {
            meta: CommandMeta {
                keyword: "history.monthly".into(),
                usage: "task <filter> history.monthly".into(),
                description: "Shows a report of task history, by month".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
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

impl Default for CmdHistoryMonthly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHistoryMonthly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryBase<MonthlyHistoryStrategy>::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        CmdHistoryBase::<MonthlyHistoryStrategy>::new().execute(context, output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGHistoryMonthly` (`ghistory.monthly`).
pub struct CmdGHistoryMonthly {
    meta: CommandMeta,
}

impl CmdGHistoryMonthly {
    /// C++ `CmdHistoryBase<MonthlyGHistoryStrategy>::CmdHistoryBase` (`typedef
    /// CmdGHistoryMonthly`).
    pub fn new() -> Self {
        CmdGHistoryMonthly {
            meta: CommandMeta {
                keyword: "ghistory.monthly".into(),
                usage: "task <filter> ghistory.monthly".into(),
                description: "Shows a graphical report of task history, by month".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
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

impl Default for CmdGHistoryMonthly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGHistoryMonthly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryBase<MonthlyGHistoryStrategy>::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        CmdHistoryBase::<MonthlyGHistoryStrategy>::new().execute(context, output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHistoryAnnual` (`history.annual`).
pub struct CmdHistoryAnnual {
    meta: CommandMeta,
}

impl CmdHistoryAnnual {
    /// C++ `CmdHistoryBase<AnnualHistoryStrategy>::CmdHistoryBase` (`typedef CmdHistoryAnnual`).
    pub fn new() -> Self {
        CmdHistoryAnnual {
            meta: CommandMeta {
                keyword: "history.annual".into(),
                usage: "task <filter> history.annual".into(),
                description: "Shows a report of task history, by year".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
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

impl Default for CmdHistoryAnnual {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHistoryAnnual {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryBase<AnnualHistoryStrategy>::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        CmdHistoryBase::<AnnualHistoryStrategy>::new().execute(context, output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdGHistoryAnnual` (`ghistory.annual`).
pub struct CmdGHistoryAnnual {
    meta: CommandMeta,
}

impl CmdGHistoryAnnual {
    /// C++ `CmdHistoryBase<AnnualGHistoryStrategy>::CmdHistoryBase` (`typedef CmdGHistoryAnnual`).
    pub fn new() -> Self {
        CmdGHistoryAnnual {
            meta: CommandMeta {
                keyword: "ghistory.annual".into(),
                usage: "task <filter> ghistory.annual".into(),
                description: "Shows a graphical report of task history, by year".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
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

impl Default for CmdGHistoryAnnual {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdGHistoryAnnual {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHistoryBase<AnnualGHistoryStrategy>::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        CmdHistoryBase::<AnnualGHistoryStrategy>::new().execute(context, output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    /// Renders the date columns of `S` for rows at `times` (local time).
    fn date_rows<S: HistoryStrategy>(times: &[Datetime]) -> Vec<String> {
        let mut view = Table::new();
        view.width(80);
        S::setup_table_dates(&mut view);
        let mut last = 0;
        for t in times {
            let row = view.add_row();
            S::insert_row_date(&mut view, row, t.to_epoch(), last);
            last = t.to_epoch();
        }
        view.render()
            .lines()
            .skip(1)
            .map(|l| l.trim_end().to_string())
            .collect()
    }

    #[test]
    fn insert_row_date_prints_changes_only() {
        let times = [
            Datetime::from_ymdhms(2014, 12, 28, 0, 0, 0),
            Datetime::from_ymdhms(2015, 1, 4, 0, 0, 0),
            Datetime::from_ymdhms(2015, 1, 11, 0, 0, 0),
            Datetime::from_ymdhms(2015, 2, 1, 0, 0, 0),
        ];
        assert_eq!(
            date_rows::<WeeklyHistoryStrategy>(&times),
            [
                "2014 December  28",
                "2015 January    4",
                "               11",
                "     February   1"
            ]
        );
        let months = [
            Datetime::from_ymdhms(2015, 1, 1, 0, 0, 0),
            Datetime::from_ymdhms(2015, 2, 1, 0, 0, 0),
            Datetime::from_ymdhms(2016, 2, 1, 0, 0, 0),
        ];
        assert_eq!(
            date_rows::<MonthlyGHistoryStrategy>(&months),
            ["2015 January", "     February", "2016 February"]
        );
        // A row of empty cells renders no line at all (why the tabular
        // report sets " " in its separator row).
        assert_eq!(date_rows::<AnnualHistoryStrategy>(&months), ["2015", "2016"]);
        // The monthly/annual strategies have no `lastTime == 0` check: a
        // first row in 1970 (the year of Datetime (0)) prints no year.
        let first = [Datetime::from_ymdhms(1970, 6, 1, 0, 0, 0)];
        assert_eq!(date_rows::<MonthlyHistoryStrategy>(&first), ["     June"]);
        assert_eq!(date_rows::<DailyGHistoryStrategy>(&first), ["1970 June    1"]);
    }

    #[test]
    fn strategy_constants_match_cpp() {
        assert_eq!(
            (DailyGHistoryStrategy::LABEL_WIDTH, WeeklyGHistoryStrategy::LABEL_WIDTH),
            (19, 19)
        );
        assert_eq!(
            (MonthlyGHistoryStrategy::LABEL_WIDTH, AnnualGHistoryStrategy::LABEL_WIDTH),
            (15, 5)
        );
        assert_eq!(
            [
                DailyHistoryStrategy::DATE_FIELD_COUNT,
                WeeklyHistoryStrategy::DATE_FIELD_COUNT,
                MonthlyHistoryStrategy::DATE_FIELD_COUNT,
                AnnualHistoryStrategy::DATE_FIELD_COUNT
            ],
            [3, 3, 2, 1]
        );
        assert_eq!(
            [MonthlyHistoryStrategy::GRAPHICAL, MonthlyGHistoryStrategy::GRAPHICAL],
            [false, true]
        );
        // Each command's metadata agrees with its strategy.
        let pairs: [(&dyn Command, &str, &str, &str); 8] = [
            (&CmdHistoryDaily::new(), DailyHistoryStrategy::KEYWORD, DailyHistoryStrategy::USAGE, DailyHistoryStrategy::DESCRIPTION),
            (&CmdGHistoryDaily::new(), DailyGHistoryStrategy::KEYWORD, DailyGHistoryStrategy::USAGE, DailyGHistoryStrategy::DESCRIPTION),
            (&CmdHistoryWeekly::new(), WeeklyHistoryStrategy::KEYWORD, WeeklyHistoryStrategy::USAGE, WeeklyHistoryStrategy::DESCRIPTION),
            (&CmdGHistoryWeekly::new(), WeeklyGHistoryStrategy::KEYWORD, WeeklyGHistoryStrategy::USAGE, WeeklyGHistoryStrategy::DESCRIPTION),
            (&CmdHistoryMonthly::new(), MonthlyHistoryStrategy::KEYWORD, MonthlyHistoryStrategy::USAGE, MonthlyHistoryStrategy::DESCRIPTION),
            (&CmdGHistoryMonthly::new(), MonthlyGHistoryStrategy::KEYWORD, MonthlyGHistoryStrategy::USAGE, MonthlyGHistoryStrategy::DESCRIPTION),
            (&CmdHistoryAnnual::new(), AnnualHistoryStrategy::KEYWORD, AnnualHistoryStrategy::USAGE, AnnualHistoryStrategy::DESCRIPTION),
            (&CmdGHistoryAnnual::new(), AnnualGHistoryStrategy::KEYWORD, AnnualGHistoryStrategy::USAGE, AnnualGHistoryStrategy::DESCRIPTION),
        ];
        for (command, keyword, usage, description) in pairs {
            let meta = command.meta();
            assert_eq!(
                (meta.keyword.as_str(), meta.usage.as_str(), meta.description.as_str()),
                (keyword, usage, description)
            );
        }
    }

    #[test]
    fn padding_is_std_string_fill_or_length_error() {
        assert_eq!(padding(5, 2).unwrap(), "   ");
        assert_eq!(padding(2, 2).unwrap(), "");
        assert!(matches!(padding(1, 2), Err(TwError::Runtime(_))));
    }

    fn run(command: &dyn Command, seed: &[&str], args: &[&str]) -> (i32, String, CommandFixture) {
        let mut f = CommandFixture::new(seed, "defaultwidth=80\ndetection=off\n", args);
        let mut output = String::new();
        let rc = command.execute(&mut f.ctx, &mut output).unwrap();
        (rc, output, f)
    }

    /// Mid-month noon UTC, so the month is the same in any time zone.
    const SEED: [&str; 4] = [
        r#"{"description":"p","entry":"20150115T120000Z"}"#,
        r#"{"description":"d","status":"deleted","entry":"20150115T120000Z","end":"20150215T120000Z"}"#,
        r#"{"description":"c","status":"completed","entry":"20150115T120000Z","end":"20150215T120000Z"}"#,
        r#"{"description":"c2","status":"completed","entry":"20150215T120000Z","end":"20150215T120000Z"}"#,
    ];

    #[test]
    fn monthly_counts_totals_and_averages() {
        let (rc, out, f) = run(&CmdHistoryMonthly::new(), &SEED, &["history.monthly"]);
        assert_eq!(rc, 0);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 6, "{out}");
        assert_eq!(lines[1], "2015 January      3         0       0   3", "{out}");
        assert_eq!(lines[2], "     February     1         2       1  -2", "{out}");
        assert_eq!(lines[3], "");
        // Integer averages over the two interval rows.
        assert_eq!(lines[4], "     Average      2         1       0   0", "{out}");
        assert_eq!(lines[5], "");
        assert!(f.ctx.footnotes.is_empty());
    }

    #[test]
    fn graphical_monthly_scales_bars() {
        let (rc, out, f) = run(
            &CmdGHistoryMonthly::new(),
            &SEED,
            &["ghistory.monthly"],
        );
        assert_eq!(rc, 0);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[1], format!("2015 January  {}", "+".repeat(32)), "{out}");
        assert_eq!(
            lines[2],
            format!(
                "     February {}{}{}{}",
                " ".repeat(22),
                "+".repeat(10),
                "X".repeat(21),
                "-".repeat(10)
            ),
            "{out}"
        );
        assert_eq!(lines[3], "");
        assert_eq!(lines[4], "Legend: + Added, X Completed, - Deleted");
        drop(f);
    }

    #[test]
    fn no_tasks_footnote_and_rc_1() {
        for command in [
            &CmdHistoryAnnual::new() as &dyn Command,
            &CmdGHistoryWeekly::new(),
        ] {
            let keyword = command.meta().keyword.clone();
            let (rc, out, f) = run(command, &[], &[keyword.as_str(), "description:nomatch"]);
            assert_eq!((rc, out.as_str()), (1, ""), "{keyword}");
            assert_eq!(f.ctx.footnotes, ["No tasks."], "{keyword}");
        }
    }

    #[test]
    fn recurring_templates_are_not_added() {
        let seed = [
            r#"{"description":"r","status":"recurring","recur":"weekly","due":"20150120T120000Z","entry":"20150115T120000Z"}"#,
        ];
        let (rc, out, f) = run(
            &CmdHistoryAnnual::new(),
            &seed,
            &["history.annual"],
        );
        assert_eq!(rc, 0);
        // The template's interval is listed, but with zero additions.
        assert!(out.lines().nth(1).unwrap().starts_with("2015        0"), "{out}");
        drop(f);
    }
}
