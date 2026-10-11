//! Rust translation of `src/commands/CmdCalendar.cpp`.
//!
//! `task calendar [due|<month> <year>|<year>] [y]`: a grid of months with
//! today, weekends, holidays, due and scheduled dates coloured, an optional
//! legend, and (with `calendar.details=full` / `calendar.holidays=full`) the
//! due-task report (run as a child process, as the C++ does) and a holiday
//! table for the displayed range.

use std::collections::BTreeMap;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::lexer::Lexer;
use crate::shared::color::Color;
use crate::shared::datetime::Datetime;
use crate::shared::format::format;
use crate::shared::shared::{auto_complete, execute};
use crate::shared::table::Table;
use crate::shared::utf8::utf8_substr;
use crate::task::{DateState, Status, Task};
use crate::util::{optional_blank_line, set_header_underline};
use crate::version::PACKAGE;

/// C `strtoll (value.c_str (), nullptr, 10)`.
fn c_strtoll(text: &str) -> i64 {
    let Ok(text) = std::ffi::CString::new(text) else {
        // An interior NUL ends the C string; parse the prefix.
        let prefix = text.split('\0').next().unwrap_or("");
        return c_strtoll(prefix);
    };
    // SAFETY: `text` is a valid NUL-terminated C string for the duration of
    // the call, and a null `endptr` is allowed.
    unsafe { libc::strtoll(text.as_ptr(), std::ptr::null_mut(), 10) }
}

/// C++ `key.substr (8, key.size () - n)` for a `holiday.` key: the
/// `size_t` length wraps to `npos` (the rest of the key) when the key is
/// shorter than `n`. Byte-based like C++; a cut inside a UTF-8 sequence is
/// converted lossily (such a name never matches a configured key, as in C++).
fn holiday_name(key: &str, n: usize) -> String {
    let bytes = key.as_bytes();
    let end = if bytes.len() >= n {
        8 + (bytes.len() - n)
    } else {
        bytes.len()
    };
    String::from_utf8_lossy(&bytes[8..end.min(bytes.len())]).into_owned()
}

/// C++ `std::setw (width) << ' '`: the space right-aligned in `width`
/// columns (at least one character).
fn setw_space(width: usize) -> String {
    " ".repeat(width.max(1))
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCalendar` (`calendar`).
pub struct CmdCalendar {
    meta: CommandMeta,
}

impl CmdCalendar {
    /// C++ `CmdCalendar::CmdCalendar`.
    pub fn new() -> Self {
        CmdCalendar {
            meta: CommandMeta {
                keyword: "calendar".into(),
                usage: "task          calendar [due|<month> <year>|<year>] [y]".into(),
                description: "Shows a calendar, with due tasks marked".into(),
                read_only: true,
                displays_id: true,
                needs_gc: true,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Graphs,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCalendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCalendar {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCalendar::execute`.
    ///
    /// # Errors
    /// `Argument '<arg>' is not a valid month.`, `Could not recognize
    /// argument '<arg>'.`, `The setting 'calendar.details.report' must
    /// contain a single report name.`, an invalid `color.calendar.*` colour,
    /// an unparsable holiday date, a task status/tag error, or a failure to
    /// run the details report (all C++ throws).
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let rc = 0;

        // Each month requires 28 text columns width.  See how many will actually
        // fit.  But if a preference is specified, and it fits, use it.
        let width = context.get_width();
        let preferred_months_per_line: i32 = if context.config.has("calendar.monthsperline") {
            context.config.get_integer("calendar.monthsperline")
        } else {
            // Legacy configuration variable value
            context.config.get_integer("monthsperline")
        };

        let months_that_fit = width / 26;

        let mut months_per_line = months_that_fit;
        if preferred_months_per_line != 0 && preferred_months_per_line < months_that_fit {
            months_per_line = preferred_months_per_line;
        }

        // Load the pending tasks.
        let tasks: Vec<Task> = context.tdb2.pending_tasks()?.to_vec();

        let today = Datetime::new();
        let mut get_pending_date = false;
        let mut months_to_display;
        let mut m_from;
        let mut y_from;
        let mut m_to;
        let mut y_to;

        // Defaults.
        months_to_display = months_per_line;
        m_from = today.month();
        y_from = today.year();

        // Set up a vector of commands (1), for autoComplete.
        let command_names = ["calendar"];

        // Set up a vector of keywords, for autoComplete.
        let keyword_names = ["due"];

        // Set up a vector of months, for autoComplete.
        let month_names: Vec<String> = (1..=12)
            .map(|i| Lexer::lower_case(&Datetime::month_name(i)))
            .collect();

        // Look at all args, regardless of sequence.
        let mut arg_month = 0;
        let mut arg_year = 0;
        let mut arg_whole_year = false;

        let words = context
            .cli2
            .get_words(&context.config, &mut context.debug_messages);
        for arg in &words {
            let minimum = context.config.get_integer("abbreviation.minimum");

            // Some version of "calendar".
            if auto_complete(&Lexer::lower_case(arg), &command_names, minimum).len() == 1 {
                continue;
            }
            // "due".
            else if auto_complete(&Lexer::lower_case(arg), &keyword_names, minimum).len() == 1 {
                get_pending_date = true;
            }
            // "y".
            else if Lexer::lower_case(arg) == "y" {
                arg_whole_year = true;
            }
            // YYYY.
            else if Lexer::is_all_digits(arg) && arg.len() == 4 {
                arg_year = c_strtoll(arg) as i32;
            }
            // MM.
            else if Lexer::is_all_digits(arg) && arg.len() <= 2 {
                arg_month = c_strtoll(arg) as i32;
                if !(1..=12).contains(&arg_month) {
                    return Err(TwError::Message(format(
                        "Argument '{1}' is not a valid month.",
                        &[arg],
                    )));
                }
            }
            // "January" etc.
            else {
                let matches = auto_complete(&Lexer::lower_case(arg), &month_names, minimum);
                if matches.len() == 1 {
                    arg_month = Datetime::month_of_year(&matches[0]);
                    if arg_month == -1 {
                        return Err(TwError::Message(format(
                            "Argument '{1}' is not a valid month.",
                            &[arg],
                        )));
                    }
                } else {
                    return Err(TwError::Message(format(
                        "Could not recognize argument '{1}'.",
                        &[arg],
                    )));
                }
            }
        }

        // Supported combinations:
        //
        //   Command line  monthsToDisplay  mFrom  yFrom  getPendingDate
        //   ------------  ---------------  -----  -----  --------------
        //   cal             monthsPerLine  today  today           false
        //   cal y                      12  today  today           false
        //   cal due         monthsPerLine  today  today            true
        //   cal YYYY                   12      1    arg           false
        //   cal due y                  12  today  today            true
        //   cal MM YYYY     monthsPerLine    arg    arg           false
        //   cal MM YYYY y              12    arg    arg           false

        // C++ `argWholeYear || (argYear && !argMonth && !argWholeYear)`; the
        // redundant `!argWholeYear` is dropped (same truth table).
        if arg_whole_year || (arg_year != 0 && arg_month == 0) {
            months_to_display = 12;
        }

        if arg_month == 0 && arg_year != 0 {
            m_from = 1;
        } else if arg_month != 0 && arg_year != 0 {
            m_from = arg_month;
        }

        if arg_year != 0 {
            y_from = arg_year;
        }

        // Now begin the data subset and rendering.
        if get_pending_date {
            // Find the oldest pending due date.
            let mut oldest = Datetime::from_ymd(9999, 12, 31);
            for task in &tasks {
                let status = task.get_status()?;
                if (status == Status::Pending || status == Status::Waiting)
                    && task.has("due")
                    && !task.has_tag("nocal", context)?
                {
                    let d = Datetime::from_string(&task.get("due"), "")?;
                    if d < oldest {
                        oldest = d;
                    }
                }
            }

            // Default to current month if no due date is present
            if oldest != Datetime::from_ymd(9999, 12, 31) {
                m_from = oldest.month();
                y_from = oldest.year();
            }
        }

        if context.config.get_boolean("calendar.offset") {
            let moffset = context.config.get_integer("calendar.offset.value") % 12;
            let yoffset = context.config.get_integer("calendar.offset.value") / 12;
            m_from += moffset;
            y_from += yoffset;
            if m_from < 1 {
                m_from += 12;
                y_from -= 1;
            } else if m_from > 12 {
                m_from -= 12;
                y_from += 1;
            }
        }

        m_to = m_from + months_to_display - 1;
        y_to = y_from;
        if m_to > 12 {
            m_to -= 12;
            y_to += 1;
        }

        let mut details_y_from = y_from;
        let mut details_m_from = m_from;

        let mut out = String::new();
        out.push('\n');

        while y_from < y_to || (y_from == y_to && m_from <= m_to) {
            let mut next_m = m_from;
            let mut next_y = y_from;

            // Print month headers (cheating on the width settings, yes)
            for _ in 0..months_per_line {
                let month = Datetime::month_name(next_m);

                //    12345678901234567890123456 = 26 chars wide
                //                ^^             = center
                //    <------->                  = 13 - (month.length / 2) + 1
                //                      <------> = 26 - above
                //   +--------------------------+
                //   |         July 2009        |
                //   |     Mo Tu We Th Fr Sa Su |
                //   |  27        1  2  3  4  5 |
                //   |  28  6  7  8  9 10 11 12 |
                //   |  29 13 14 15 16 17 18 19 |
                //   |  30 20 21 22 23 24 25 26 |
                //   |  31 27 28 29 30 31       |
                //   +--------------------------+

                let total_width: usize = 26;
                let label_width = month.len() + 5; // 5 = " 2009"
                let left_gap = (total_width / 2).wrapping_sub(label_width / 2);
                let right_gap = total_width.wrapping_sub(left_gap).wrapping_sub(label_width);

                out.push_str(&setw_space(left_gap));
                out.push_str(&month);
                out.push(' ');
                out.push_str(&next_y.to_string());
                out.push_str(&setw_space(right_gap));

                next_m += 1;
                if next_m > 12 {
                    next_m = 1;
                    next_y += 1;
                }
            }

            out.push('\n');
            out.push_str(optional_blank_line(context));
            out.push_str(&self.render_months(
                context,
                m_from,
                y_from,
                &today,
                &tasks,
                months_per_line,
            )?);
            out.push('\n');

            m_from += months_per_line;
            if m_from > 12 {
                m_from -= 12;
                y_from += 1;
            }
        }

        let config = &context.config;
        let color_today = Color::parse(&config.get("color.calendar.today"))?;
        let color_due = Color::parse(&config.get("color.calendar.due"))?;
        let color_duetoday = Color::parse(&config.get("color.calendar.due.today"))?;
        let color_overdue = Color::parse(&config.get("color.calendar.overdue"))?;
        let color_weekend = Color::parse(&config.get("color.calendar.weekend"))?;
        let color_holiday = Color::parse(&config.get("color.calendar.holiday"))?;
        let color_scheduled = Color::parse(&config.get("color.calendar.scheduled"))?;
        let color_weeknumber = Color::parse(&config.get("color.calendar.weeknumber"))?;

        if context.color() && context.config.get_boolean("calendar.legend") {
            out.push_str("Legend: ");
            out.push_str(&color_today.colorize("today"));
            out.push_str(", ");
            out.push_str(&color_weekend.colorize("weekend"));
            out.push_str(", ");

            // If colorizing due dates, print legend
            if context.config.get("calendar.details") != "none" {
                out.push_str(&color_due.colorize("due"));
                out.push_str(", ");
                out.push_str(&color_duetoday.colorize("due-today"));
                out.push_str(", ");
                out.push_str(&color_overdue.colorize("overdue"));
                out.push_str(", ");
                out.push_str(&color_scheduled.colorize("scheduled"));
                out.push_str(", ");
            }

            // If colorizing holidays, print legend
            if context.config.get("calendar.holidays") != "none" {
                out.push_str(&color_holiday.colorize("holiday"));
                out.push_str(", ");
            }

            out.push_str(&color_weeknumber.colorize("weeknumber"));
            out.push('.');
            out.push_str(optional_blank_line(context));
            out.push('\n');
        }

        if context.config.get("calendar.details") == "full"
            || context.config.get("calendar.holidays") == "full"
        {
            details_m_from -= 1;
            if details_m_from == 0 {
                details_m_from = 12;
                details_y_from -= 1;
            }
            let details_d_from = Datetime::days_in_month(details_y_from, details_m_from);

            m_to += 1;
            if m_to == 13 {
                m_to = 1;
                y_to += 1;
            }

            let date_after = Datetime::from_ymd(details_y_from, details_m_from, details_d_from);
            let after = date_after.to_string(&context.config.get("dateformat"));

            let date_before = Datetime::from_ymd(y_to, m_to, 1);
            let before = date_before.to_string(&context.config.get("dateformat"));

            // Table with due date information
            if context.config.get("calendar.details") == "full" {
                // Assert that 'report' is a valid report.
                let report = context.config.get("calendar.details.report");
                if !context.commands.contains_key(&report) {
                    return Err(TwError::Message(
                        "The setting 'calendar.details.report' must contain a single report name."
                            .into(),
                    ));
                }

                // TODO Fix this:  cal      --> task
                //                 calendar --> taskendar

                // If the executable was "cal" or equivalent, replace it with "task".
                let mut executable = context.cli2.original_args[0].attribute("raw");
                if let Some(cal) = executable.find("cal") {
                    executable = executable[..cal].to_string() + PACKAGE;
                }

                let mut args: Vec<String> = Vec::new();
                args.push("rc:".to_string() + &context.rc_file.data);
                args.push("rc.due:0".into());
                args.push("rc.verbose:label,affected,blank".into());
                if context.color() {
                    args.push("rc._forcecolor:on".into());
                }
                args.push("due.after:".to_string() + &after);
                args.push("due.before:".to_string() + &before);
                args.push("-nocal".into());
                args.push(report);

                let (_, output) = execute(&executable, &args, "")?;
                out.push_str(&output);
            }

            // Table with holiday information
            if context.config.get("calendar.holidays") == "full" {
                let mut hol_table = Table::new();
                hol_table.width(context.get_width());
                hol_table.add("Date");
                hol_table.add("Holiday");
                hol_table.with_color(context.color());
                set_header_underline(context, &mut hol_table)?;

                let config = &context.config;
                let date_format = config.get("dateformat.holiday");

                // we need to store multiple holidays per day
                let mut hm: BTreeMap<i64, Vec<String>> = BTreeMap::new();
                for (key, value) in config {
                    if key.starts_with("holiday.") && key.ends_with("name") {
                        let hol_name = value.clone();
                        let name = holiday_name(key, 13);
                        let date = config.get(&("holiday.".to_string() + &name + ".date"));
                        let start = config.get(&("holiday.".to_string() + &name + ".start"));
                        let end = config.get(&("holiday.".to_string() + &name + ".end"));
                        if !date.is_empty() {
                            let hol_date = Datetime::from_string(&date, &date_format)?;

                            if date_after < hol_date && hol_date < date_before {
                                hm.entry(hol_date.to_epoch()).or_default().push(hol_name.clone());
                            }
                        }
                        if !start.is_empty() && !end.is_empty() {
                            let hol_start = Datetime::from_string(&start, &date_format)?;
                            let hol_end = Datetime::from_string(&end, &date_format)?;

                            if date_after < hol_start && hol_start < date_before {
                                hm.entry(hol_start.to_epoch())
                                    .or_default()
                                    .push("Start of ".to_string() + &hol_name);
                            }
                            if date_after < hol_end && hol_end < date_before {
                                hm.entry(hol_end.to_epoch())
                                    .or_default()
                                    .push("End of ".to_string() + &hol_name);
                            }
                        }
                    }
                }

                let mut format_ = config.get(
                    &("report.".to_string() + &config.get("calendar.details.report") + ".dateformat"),
                );
                if format_.is_empty() {
                    format_ = config.get("dateformat.report");
                }
                if format_.is_empty() {
                    format_ = config.get("dateformat");
                }

                for (epoch, v) in &hm {
                    let h_date = Datetime::from_epoch(*epoch);
                    let d = h_date.to_string(&format_);
                    for i in v {
                        let row = hol_table.add_row();
                        hol_table.set(row, 0, &d);
                        hol_table.set(row, 1, i);
                    }
                }

                out.push_str(optional_blank_line(context));
                out.push_str(&hol_table.render());
                out.push('\n');
            }
        }

        *output = out;
        Ok(rc)
    }
}

impl CmdCalendar {
    /// C++ `CmdCalendar::renderMonths`: the day grid for `months_per_line`
    /// months starting at `first_month`/`first_year`.
    ///
    /// As in C++, when colour is on and `calendar.details` is not `none`
    /// this sets `rc.due` and `rc.scheduled` to 0 in the running
    /// configuration (so every future due date counts as "due").
    ///
    /// # Errors
    /// An invalid colour, header colour, holiday date or weekstart, or a
    /// task status/tag error (C++ throws).
    fn render_months(
        &self,
        context: &mut Context,
        first_month: i32,
        first_year: i32,
        today: &Datetime,
        all: &[Task],
        months_per_line: i32,
    ) -> Result<String> {
        let week_start = Datetime::weekstart();

        // Build table for the number of months to be displayed.
        let mut view = Table::new();
        set_header_underline(context, &mut view)?;
        view.width(context.get_width());
        view.with_color(context.color());
        let mut i = 0;
        while i < (months_per_line * 8) {
            if week_start == 1 {
                view.add_with("", false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(1), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(2), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(3), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(4), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(5), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(6), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(0), 0, 2), false, true);
            } else {
                view.add_with("", false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(0), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(1), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(2), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(3), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(4), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(5), 0, 2), false, true);
                view.add_with(&utf8_substr(&Datetime::day_name(6), 0, 2), false, true);
            }
            i += 8;
        }

        // At most, we need 6 rows.
        view.add_row();
        view.add_row();
        view.add_row();
        view.add_row();
        view.add_row();
        view.add_row();

        // Set number of days per month, months to render, and years to render.
        let mut years: Vec<i32> = Vec::new();
        let mut months: Vec<i32> = Vec::new();
        let mut days_in_month: Vec<i32> = Vec::new();
        let mut this_year = first_year;
        let mut this_month = first_month;
        for _ in 0..months_per_line {
            if this_month < 13 {
                years.push(this_year);
            } else {
                this_month -= 12;
                this_year += 1;
                years.push(this_year);
            }
            months.push(this_month);
            days_in_month.push(Datetime::days_in_month(this_year, this_month));
            this_month += 1;
        }

        let mut row: usize = 0;

        let config = &context.config;
        let color_today = Color::parse(&config.get("color.calendar.today"))?;
        let color_due = Color::parse(&config.get("color.calendar.due"))?;
        let color_duetoday = Color::parse(&config.get("color.calendar.due.today"))?;
        let color_overdue = Color::parse(&config.get("color.calendar.overdue"))?;
        let color_weekend = Color::parse(&config.get("color.calendar.weekend"))?;
        let color_holiday = Color::parse(&config.get("color.calendar.holiday"))?;
        let color_scheduled = Color::parse(&config.get("color.calendar.scheduled"))?;
        let color_weeknumber = Color::parse(&config.get("color.calendar.weeknumber"))?;

        // Loop through months to be added on this line.
        for mpl in 0..months_per_line as usize {
            // Reset row counter for subsequent months
            if mpl != 0 {
                row = 0;
            }

            // Loop through days in month and add to table.
            for d in 1..=days_in_month[mpl] {
                let date = Datetime::from_ymd(years[mpl], months[mpl], d);
                let dow = date.day_of_week();
                let woy = date.try_week()?;

                if context.config.get_boolean("displayweeknumber") {
                    view.set_colored(
                        row,
                        8 * mpl,
                        // Make sure the week number is always 4 columns, space-padded.
                        &format(if woy < 10 { "   {1}" } else { "  {1}" }, &[&woy]),
                        color_weeknumber,
                    );
                }

                // Calculate column id.
                let mut this_col = dow as usize                  // 0 = Sunday
                    + (if week_start == 1 { 0 } else { 1 })      // Offset for weekStart
                    + (8 * mpl); // Columns in 1 month

                if this_col == (8 * mpl) {
                    this_col += 7;
                }

                view.set_int(row, this_col, d);

                if context.color() {
                    let mut cell_color = Color::new();

                    // colorize weekends
                    if dow == 0 || dow == 6 {
                        cell_color.blend(&color_weekend);
                    }

                    // colorize holidays
                    let config = &context.config;
                    if config.get("calendar.holidays") != "none" {
                        let date_format = config.get("dateformat.holiday");
                        for (key, value) in config {
                            if key.starts_with("holiday.") {
                                if key.ends_with("date") {
                                    let hol_date = Datetime::from_string(value, &date_format)?;
                                    if hol_date.same_day(&date) {
                                        cell_color.blend(&color_holiday);
                                    }
                                }

                                if key.ends_with("start") {
                                    let end_key =
                                        "holiday.".to_string() + &holiday_name(key, 14) + ".end";
                                    if config.has(&end_key) {
                                        let start = value;
                                        let end = config.get(&end_key);
                                        let hol_start = Datetime::from_string(start, &date_format)?;
                                        let hol_end = Datetime::from_string(&end, &date_format)?;
                                        if hol_start <= date && date <= hol_end {
                                            cell_color.blend(&color_holiday);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // colorize today
                    if today.same_day(&date) {
                        cell_color.blend(&color_today);
                    }

                    // colorize due and scheduled tasks
                    if context.config.get("calendar.details") != "none" {
                        context.config.set("due", 0);
                        context.config.set("scheduled", 0);
                        // if a date has a task that is due on that day, the due color
                        // takes precedence over the scheduled color
                        let mut colored_with_due = false;
                        for task in all {
                            let status = task.get_status()?;
                            if (status == Status::Pending || status == Status::Waiting)
                                && !task.has_tag("nocal", context)?
                            {
                                if task.has("scheduled") && !colored_with_due {
                                    let scheduled = task.get("scheduled");
                                    let scheduleddmy = Datetime::from_epoch(c_strtoll(&scheduled));

                                    if scheduleddmy.same_day(&date) {
                                        cell_color.blend(&color_scheduled);
                                    }
                                }
                                if task.has("due") {
                                    let due = task.get("due");
                                    let duedmy = Datetime::from_epoch(c_strtoll(&due));

                                    if duedmy.same_day(&date) {
                                        colored_with_due = true;
                                        match task.get_date_state("due", context) {
                                            DateState::NotDue => {}

                                            DateState::AfterToday => {
                                                cell_color.blend(&color_due);
                                            }

                                            DateState::LaterToday => {
                                                cell_color.blend(&color_duetoday);
                                            }

                                            DateState::EarlierToday | DateState::BeforeToday => {
                                                cell_color.blend(&color_overdue);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    view.set_color(row, this_col, cell_color);
                }

                // Check for end of week, and...
                let mut eow = 6;
                if week_start == 1 {
                    eow = 0;
                }
                if dow == eow && d < days_in_month[mpl] {
                    row += 1;
                }
            }
        }

        Ok(view.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    #[test]
    fn holiday_name_is_cpp_substr() {
        assert_eq!(holiday_name("holiday.en-US1.name", 13), "en-US1");
        assert_eq!(holiday_name("holiday.x.start", 14), "x");
        // size - n wraps to npos in C++: the rest of the key.
        assert_eq!(holiday_name("holiday.name", 13), "name");
        assert_eq!(holiday_name("holiday.start", 14), "start");
        assert_eq!(holiday_name("holiday..name", 13), "");
    }

    #[test]
    fn setw_space_pads_at_least_one() {
        assert_eq!(setw_space(0), " ");
        assert_eq!(setw_space(1), " ");
        assert_eq!(setw_space(6), "      ");
    }

    fn run(rc: &str, args: &[&str]) -> (Result<i32>, String, CommandFixture) {
        let mut f = CommandFixture::new(
            &[],
            &format!("defaultwidth=80\ndetection=off\nfontunderline=off\n{rc}"),
            args,
        );
        let mut output = String::new();
        let result = CmdCalendar::new().execute(&mut f.ctx, &mut output);
        (result, output, f)
    }

    #[test]
    fn renders_requested_month() {
        let (rc, out, f) = run("calendar.monthsperline=1\n", &["calendar", "2", "2021"]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(
            out,
            "\n       February 2021      \n     Su Mo Tu We Th Fr Sa\n\
             ---- -- -- -- -- -- -- --\n   5     1  2  3  4  5  6\n\
             \x20  6  7  8  9 10 11 12 13\n   7 14 15 16 17 18 19 20\n\
             \x20  8 21 22 23 24 25 26 27\n   9 28\n\n"
        );
        drop(f);
    }

    #[test]
    fn whole_year_and_month_name() {
        let (rc, out, f) = run("calendar.monthsperline=3\n", &["calendar", "y", "march", "2022"]);
        assert_eq!(rc.unwrap(), 0);
        assert!(out.contains("March 2022"), "{out:?}");
        assert!(out.contains("February 2023"), "{out:?}");
        assert!(!out.contains("March 2023"), "{out:?}");
        drop(f);
    }

    #[test]
    fn argument_errors() {
        let (rc, out, f) = run("", &["calendar", "0"]);
        assert_eq!(rc.unwrap_err().to_string(), "Argument '0' is not a valid month.");
        assert_eq!(out, "");
        drop(f);
        let (rc, _, f) = run("", &["calendar", "12345"]);
        assert_eq!(
            rc.unwrap_err().to_string(),
            "Could not recognize argument '12345'."
        );
        drop(f);
    }

    #[test]
    fn bad_holiday_date_propagates() {
        let (rc, _, f) = run(
            "calendar.holidays=full\nholiday.x.name=X\nholiday.x.date=notadate\n",
            &["calendar", "2", "2021"],
        );
        assert_eq!(
            rc.unwrap_err().to_string(),
            "'notadate' is not a valid date in the 'YMD' format."
        );
        drop(f);
    }

    #[test]
    fn colour_forces_rc_due_zero() {
        // C++ renderMonths sets rc.due/rc.scheduled to 0 when colouring due dates.
        let (rc, out, f) = run("_forcecolor=on\n", &["calendar", "2", "2021"]);
        assert_eq!(rc.unwrap(), 0);
        assert!(out.contains("Legend: "), "{out:?}");
        assert_eq!(f.ctx.config.get("due"), "0");
        assert_eq!(f.ctx.config.get("scheduled"), "0");
        drop(f);
        let (rc, _, f) = run("calendar.details=none\n_forcecolor=on\n", &["calendar"]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(f.ctx.config.get("due"), "7");
        drop(f);
    }
}
