//! Rust translation of `src/ViewTask.h` / `src/ViewTask.cpp`: the task table
//! renderer used by custom reports (`CmdCustom`), `commands` and (in
//! milestone 8) `timesheet`.
//!
//! # Public API
//!
//! ```text
//! ViewTask::new()                               // C++ ViewTask::ViewTask ()
//! view.add(column: Box<dyn Column>, sort: bool) // C++ add (Column*, bool sort = false)
//! view.width(i32)                               // C++ width (int)
//! view.left_margin(i32)                         // C++ leftMargin (int)
//! view.color_header(&Color)                     // C++ colorHeader (Color&)
//! view.color_sort_header(&Color)                // C++ colorSortHeader (Color&)
//! view.color_odd(&Color) / color_even(&Color)   // C++ colorOdd / colorEven
//! view.intra_padding(i32)                       // C++ intraPadding (int)
//! view.intra_color_odd(&Color) / intra_color_even(&Color)
//! view.extra_padding(i32)                       // C++ extraPadding (int)
//! view.extra_color_odd(&Color) / extra_color_even(&Color)
//! view.truncate_lines(i32) / truncate_rows(i32) // C++ truncateLines / truncateRows
//! view.add_break(&str)                          // C++ addBreak (const std::string&)
//! view.lines() -> i32 / view.rows() -> i32      // C++ lines () / rows ()
//! view.render(ctx: &mut Context, data: &mut [Task], sequence: &[usize])
//!     -> crate::Result<String>                  // C++ render (std::vector<Task>&, std::vector<int>&)
//! ```
//!
//! The view owns its columns (`Box<dyn Column>`, the C++ destructor's
//! `delete`). Because the columns are owned by the view and not borrowed from
//! the Context, `render` can take `&mut Context` explicitly: it is used as
//! the columns' [`RenderEnv`](crate::columns::column::RenderEnv), for [`rules::auto_colorize`], `color ()`,
//! `debug`, `error` and the `time_render_us` accounting.
//!
//! `render` reads `obfuscate` and `print.empty.columns` from the
//! configuration, as C++ does. There is no ViewTask-level `forceColor` or
//! `obfuscate` setter in Taskwarrior 3.5.0: colour comes from
//! `Context::color ()` (which honours `_forcecolor`) and obfuscation from
//! `rc.obfuscate`. `rc.row.padding` and the break blank lines are wired by
//! the callers (`CmdCustom`: `extra_padding (row.padding)` and `add_break`),
//! exactly as in C++.
//!
//! # Errors
//! `render` returns the errors the C++ column `measure`/`render` overrides
//! and `autoColorize` throw (urgency, dependency lookups, `getStatus`,
//! `+LATEST`). The "report has a minimum width" problem is not an error: as
//! in C++ it is queued with `Context::error` and rendering continues.

use crate::columns::column::Column;
use crate::context::Context;
use crate::error::Result;
use crate::rules;
use crate::shared::color::Color;
use crate::shared::format::{format, obfuscate_text};
use crate::shared::timer::Timer;
use crate::shared::utf8::utf8_width;
use crate::task::Task;

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ViewTask`.
pub struct ViewTask {
    /// C++ `_columns`.
    columns: Vec<Box<dyn Column>>,
    /// C++ `_sort`.
    sort: Vec<bool>,
    /// C++ `_breaks`.
    breaks: Vec<String>,
    /// C++ `_width`.
    width: i32,
    /// C++ `_left_margin`.
    left_margin: i32,
    /// C++ `_header`.
    header: Color,
    /// C++ `_sort_header`.
    sort_header: Color,
    /// C++ `_odd`.
    odd: Color,
    /// C++ `_even`.
    even: Color,
    /// C++ `_intra_padding`.
    intra_padding: i32,
    /// C++ `_intra_odd`.
    intra_odd: Color,
    /// C++ `_intra_even`.
    intra_even: Color,
    /// C++ `_extra_padding`.
    extra_padding: i32,
    /// C++ `_extra_odd`.
    extra_odd: Color,
    /// C++ `_extra_even`.
    extra_even: Color,
    /// C++ `_truncate_lines`.
    truncate_lines: i32,
    /// C++ `_truncate_rows`.
    truncate_rows: i32,
    /// C++ `_lines`.
    lines: i32,
    /// C++ `_rows`.
    rows: i32,
}

impl Default for ViewTask {
    fn default() -> Self {
        ViewTask::new()
    }
}

impl std::fmt::Debug for ViewTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self.columns.iter().map(|c| format!("{}.{}", c.name(), c.style())).collect();
        f.debug_struct("ViewTask")
            .field("columns", &names)
            .field("width", &self.width)
            .field("lines", &self.lines)
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

/// Spaces, as C++ `std::string (n, ' ')`; a negative count is treated as 0.
fn spaces(n: i64) -> String {
    " ".repeat(usize::try_from(n).unwrap_or(0))
}

/// C++ `out.erase (out.find_last_not_of (' ') + 1)`: trims trailing spaces
/// from the whole buffer (all of it, if it holds nothing but spaces).
fn trim_right(out: &mut String) {
    let len = out.trim_end_matches(' ').len();
    out.truncate(len);
}

impl ViewTask {
    /// C++ `ViewTask::ViewTask ()`.
    pub fn new() -> Self {
        ViewTask {
            columns: Vec::new(),
            sort: Vec::new(),
            breaks: Vec::new(),
            width: 0,
            left_margin: 0,
            header: Color::new(),
            sort_header: Color::new(),
            odd: Color::new(),
            even: Color::new(),
            intra_padding: 1,
            intra_odd: Color::new(),
            intra_even: Color::new(),
            extra_padding: 0,
            extra_odd: Color::new(),
            extra_even: Color::new(),
            truncate_lines: 0,
            truncate_rows: 0,
            lines: 0,
            rows: 0,
        }
    }

    /// C++ `void add (Column* column, bool sort = false)`: the view takes
    /// ownership of the column.
    pub fn add(&mut self, column: Box<dyn Column>, sort: bool) {
        self.columns.push(column);
        self.sort.push(sort);
    }

    /// C++ `void width (int)`; 0 means unlimited.
    pub fn width(&mut self, width: i32) {
        self.width = width;
    }

    /// C++ `void leftMargin (int)`.
    pub fn left_margin(&mut self, margin: i32) {
        self.left_margin = margin;
    }

    /// C++ `void colorHeader (Color&)`: also becomes the sort header colour
    /// unless one is already set (C++ `if (!_sort_header)`, i.e. its `int`
    /// value is 0).
    pub fn color_header(&mut self, c: &Color) {
        self.header = *c;
        if self.sort_header.to_int() == 0 {
            self.sort_header = *c;
        }
    }

    /// C++ `void colorSortHeader (Color&)`.
    pub fn color_sort_header(&mut self, c: &Color) {
        self.sort_header = *c;
    }

    /// C++ `void colorOdd (Color&)`.
    pub fn color_odd(&mut self, c: &Color) {
        self.odd = *c;
    }

    /// C++ `void colorEven (Color&)`.
    pub fn color_even(&mut self, c: &Color) {
        self.even = *c;
    }

    /// C++ `void intraPadding (int)`: spaces between columns.
    pub fn intra_padding(&mut self, padding: i32) {
        self.intra_padding = padding;
    }

    /// C++ `void intraColorOdd (Color&)`.
    pub fn intra_color_odd(&mut self, c: &Color) {
        self.intra_odd = *c;
    }

    /// C++ `void intraColorEven (Color&)`.
    pub fn intra_color_even(&mut self, c: &Color) {
        self.intra_even = *c;
    }

    /// C++ `void extraPadding (int)`: spaces left and right of each row.
    pub fn extra_padding(&mut self, padding: i32) {
        self.extra_padding = padding;
    }

    /// C++ `void extraColorOdd (Color&)`.
    pub fn extra_color_odd(&mut self, c: &Color) {
        self.extra_odd = *c;
    }

    /// C++ `void extraColorEven (Color&)`.
    pub fn extra_color_even(&mut self, c: &Color) {
        self.extra_even = *c;
    }

    /// C++ `void truncateLines (int)`: stop after this many output lines
    /// (headers and break lines included); 0 means no limit.
    pub fn truncate_lines(&mut self, n: i32) {
        self.truncate_lines = n;
    }

    /// C++ `void truncateRows (int)`: stop after this many tasks; 0 means
    /// no limit.
    pub fn truncate_rows(&mut self, n: i32) {
        self.truncate_rows = n;
    }

    /// C++ `void addBreak (const std::string&)`: a blank line is inserted
    /// between consecutive tasks whose value of this attribute differs.
    pub fn add_break(&mut self, attr: &str) {
        self.breaks.push(attr.to_string());
    }

    /// C++ `int lines ()`: lines rendered by the last `render`.
    pub fn lines(&self) -> i32 {
        self.lines
    }

    /// C++ `int rows ()`: tasks rendered by the last `render` (not reset when
    /// the header alone exhausts `truncateLines`, as in C++).
    pub fn rows(&self) -> i32 {
        self.rows
    }

    /// Adds the elapsed render time to `Context::time_render_us`.
    fn account(ctx: &mut Context, timer: &Timer) {
        ctx.time_render_us += timer.total_us() as i64;
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::string ViewTask::render (std::vector<Task>& data,
    /// std::vector<int>& sequence)`: the table of `data[sequence[...]]`.
    ///
    /// ```text
    ///   |<---------- terminal width ---------->|
    ///
    ///         +-------+  +-------+  +-------+
    ///         |header |  |header |  |header |
    ///   +--+--+-------+--+-------+--+-------+--+
    ///   |ma|ex|cell   |in|cell   |in|cell   |ex|
    ///   +--+--+-------+--+-------+--+-------+--+
    ///   |ma|ex|cell   |in|cell   |in|cell   |ex|
    ///   +--+--+-------+--+-------+--+-------+--+
    /// ```
    ///
    /// Layout: every column is measured over the tasks (minimum = longest
    /// indivisible word, ideal = full value; the header label counts too).
    /// If the ideal widths fit (or the width is 0) they are used; if the
    /// minimum widths do not fit, the longest (two) columns are shortened,
    /// else `The report has a minimum width of N and does not fit in the
    /// available width of M.` is queued as an error; otherwise the overage
    /// is spread one character at a time over the columns below their ideal
    /// width. Without `print.empty.columns`, columns with a zero minimum are
    /// dropped from the view (permanently, as in C++).
    ///
    /// Deviation: where C++ would index an empty width vector (no columns
    /// left and not enough room) or pass a negative width to a column (a
    /// single very wide column squeezed by more than half), both undefined
    /// behaviour, Rust renders no widths / clamps the width at 0.
    ///
    /// # Errors
    /// Errors from the columns' `measure`/`render` and from
    /// [`rules::auto_colorize`].
    pub fn render(&mut self, ctx: &mut Context, data: &mut [Task], sequence: &[usize]) -> Result<String> {
        let timer = Timer::new();

        let obfuscate = ctx.config.get_boolean("obfuscate");
        let print_empty_columns = ctx.config.get_boolean("print.empty.columns");

        // Determine minimal, ideal column widths.
        let mut minimal: Vec<i64> = Vec::new();
        let mut ideal: Vec<i64> = Vec::new();
        let mut keep: Vec<bool> = Vec::with_capacity(self.columns.len());

        for column in &self.columns {
            // Headers factor in to width calculations.
            let mut global_min: usize = 0;
            let mut global_ideal: usize = global_min;

            for (s, &index) in sequence.iter().enumerate() {
                let s = s as i64;
                if s >= i64::from(self.truncate_lines) && self.truncate_lines != 0 {
                    break;
                }

                if s >= i64::from(self.truncate_rows) && self.truncate_rows != 0 {
                    break;
                }

                // Determine minimum and ideal width for this column.
                let (min, ideal) = column.measure(&mut data[index], ctx)?;

                if min > global_min {
                    global_min = min;
                }
                if ideal > global_ideal {
                    global_ideal = ideal;
                }

                // If a fixed-width column was just measured, there is no point repeating
                // the measurement for all tasks.
                if column.is_fixed_width() {
                    break;
                }
            }

            if print_empty_columns || global_min != 0 {
                let label_length = utf8_width(column.label()) as usize;
                if label_length > global_min {
                    global_min = label_length;
                }
                if label_length > global_ideal {
                    global_ideal = label_length;
                }
                minimal.push(global_min as i64);
                ideal.push(global_ideal as i64);
            }

            // Column is nonempty, or empty and dropped.
            keep.push(print_empty_columns || global_min != 0);
        }

        if !print_empty_columns {
            let columns = std::mem::take(&mut self.columns);
            let sort = std::mem::take(&mut self.sort);
            for ((column, sort), keep) in columns.into_iter().zip(sort).zip(keep) {
                if keep {
                    self.columns.push(column);
                    self.sort.push(sort);
                }
            }
        }

        // C++ computes `(_columns.size () - 1) * _intra_padding` in size_t
        // and converts to int, which for no columns is `-_intra_padding`.
        let all_extra = i64::from(self.left_margin)
            + 2 * i64::from(self.extra_padding)
            + (self.columns.len() as i64 - 1) * i64::from(self.intra_padding);

        // Sum the widths.
        let sum_minimal: i64 = minimal.iter().sum();
        let sum_ideal: i64 = ideal.iter().sum();

        // Calculate final column widths.
        let width = i64::from(self.width);
        let mut overage = width - sum_minimal - all_extra;
        ctx.debug(&format(
            "ViewTask::render min={1} ideal={2} overage={3} width={4}",
            &[&(sum_minimal + all_extra), &(sum_ideal + all_extra), &overage, &width],
        ));

        let mut widths: Vec<i64> = Vec::new();

        // Ideal case.  Everything fits.
        if width == 0 || sum_ideal + all_extra <= width {
            widths = ideal.clone();
        }
        // Not enough for minimum. Decrease certain columns.
        else if overage < 0 {
            if !minimal.is_empty() {
                // Determine which columns are the longest.
                let mut longest = 0usize;
                let mut second_longest = 0usize;
                for j in 0..minimal.len() {
                    if minimal[j] > minimal[longest] {
                        second_longest = longest;
                        longest = j;
                    } else if minimal[j] > minimal[second_longest] {
                        second_longest = j;
                    }
                }

                // Case 1: Shortening longest column still keeps it longest. Let it bear
                //         all the shortening.
                widths = minimal.clone();
                if minimal[longest] + overage >= minimal[second_longest] {
                    widths[longest] += overage;
                }
                // Case 2: Shorten the longest column to second longest length. Try to
                //         split shortening them evenly.
                else {
                    let decrease = minimal[second_longest] - minimal[longest];
                    widths[longest] += decrease;
                    overage -= decrease;

                    // Attempt to decrease the two longest columns (at most to two characters)
                    if -overage <= widths[longest] + widths[second_longest] - 4 {
                        // Compute half of the overage, rounding up (C++ `/` and `%`
                        // truncate toward zero, as Rust's do).
                        let half_overage = overage / 2 + overage % 2;

                        // Decrease both larges columns by this amount
                        widths[longest] += half_overage;
                        widths[second_longest] += half_overage;
                    } else {
                        // If reducing two of the longest solumns to 2 characters is not sufficient, then give up.
                        ctx.error(&format(
                            "The report has a minimum width of {1} and does not fit in the available width of {2}.",
                            &[&(sum_minimal + all_extra), &width],
                        ));
                    }
                }
            }
        }
        // Perfect minimal width.
        else if overage == 0 {
            widths = minimal.clone();
        }
        // Extra space to share.
        else if overage > 0 {
            widths = minimal.clone();

            // Spread 'overage' among columns where width[i] < ideal[i]
            let mut needed = true;
            while overage != 0 && needed {
                needed = false;
                let mut i = 0;
                while i < self.columns.len() && overage != 0 {
                    if widths[i] < ideal[i] {
                        widths[i] += 1;
                        overage -= 1;
                        needed = true;
                    }
                    i += 1;
                }
            }
        }

        let widths: Vec<usize> = widths.iter().map(|&w| usize::try_from(w).unwrap_or(0)).collect();

        // Compose column headers.
        let mut max_lines = 0usize;
        let mut headers: Vec<Vec<String>> = Vec::with_capacity(self.columns.len());
        for (c, column) in self.columns.iter().enumerate() {
            let mut header = Vec::new();
            let color = if self.sort[c] { self.sort_header } else { self.header };
            column.render_header(&mut header, widths[c], &color, ctx);

            if header.len() > max_lines {
                max_lines = header.len();
            }
            headers.push(header);
        }

        // Render column headers.
        let left_margin = spaces(i64::from(self.left_margin));
        let extra = spaces(i64::from(self.extra_padding));
        let intra = spaces(i64::from(self.intra_padding));

        let color = ctx.color();
        let extra_odd = if color { self.extra_odd.colorize(&extra) } else { extra.clone() };
        let extra_even = if color { self.extra_even.colorize(&extra) } else { extra.clone() };
        let intra_odd = if color { self.intra_odd.colorize(&intra) } else { intra.clone() };
        let intra_even = if color { self.intra_even.colorize(&intra) } else { intra.clone() };

        let mut out = String::new();
        self.lines = 0;
        for i in 0..max_lines {
            out.push_str(&left_margin);
            out.push_str(&extra);

            for c in 0..self.columns.len() {
                if c != 0 {
                    out.push_str(&intra);
                }

                if headers[c].len() < max_lines - i {
                    out.push_str(&self.header.colorize(&spaces(widths[c] as i64)));
                } else {
                    out.push_str(&headers[c][i]);
                }
            }

            out.push_str(&extra);

            // Trim right.
            trim_right(&mut out);
            out.push('\n');

            // Stop if the line limit is exceeded.
            self.lines += 1;
            if self.lines >= self.truncate_lines && self.truncate_lines != 0 {
                ViewTask::account(ctx, &timer);
                return Ok(out);
            }
        }

        // Compose, render columns, in sequence.
        self.rows = 0;
        let mut cells: Vec<Vec<String>> = Vec::with_capacity(self.columns.len());
        for (s, &index) in sequence.iter().enumerate() {
            max_lines = 0;

            // Apply color rules to task.
            let mut rule_color = Color::new();
            rules::auto_colorize(ctx, &data[index], &mut rule_color)?;

            // Alternate rows based on |s % 2|
            let odd = s % 2 != 0;
            let mut row_color = Color::new();
            if ctx.color() {
                row_color = if odd { self.odd } else { self.even };
                row_color.blend(&rule_color);
            }

            for (c, column) in self.columns.iter().enumerate() {
                let mut cell = Vec::new();
                column.render(&mut cell, &mut data[index], widths[c], &row_color, ctx)?;

                if cell.len() > max_lines {
                    max_lines = cell.len();
                }

                if obfuscate && column.type_() == "string" && column.name() != "depends" {
                    for line in &mut cell {
                        *line = obfuscate_text(line);
                    }
                }
                cells.push(cell);
            }

            // Listing breaks are simply blank lines inserted when a column value
            // changes.
            if s > 0 && !self.breaks.is_empty() {
                let previous = sequence[s - 1];
                for b in &self.breaks {
                    if data[previous].get(b) != data[index].get(b) {
                        out.push('\n');
                        self.lines += 1;

                        // Only want one \n, regardless of how many values change.
                        break;
                    }
                }
            }

            for i in 0..max_lines {
                out.push_str(&left_margin);
                out.push_str(if odd { &extra_odd } else { &extra_even });

                for c in 0..self.columns.len() {
                    if c != 0 {
                        if row_color.nontrivial() {
                            row_color.colorize_into(&mut out, &intra);
                        } else {
                            out.push_str(if odd { &intra_odd } else { &intra_even });
                        }
                    }

                    if i < cells[c].len() {
                        out.push_str(&cells[c][i]);
                    } else {
                        row_color.colorize_into(&mut out, &spaces(widths[c] as i64));
                    }
                }

                out.push_str(if odd { &extra_odd } else { &extra_even });

                // Trim right.
                trim_right(&mut out);
                out.push('\n');

                // Stop if the line limit is exceeded.
                self.lines += 1;
                if self.lines >= self.truncate_lines && self.truncate_lines != 0 {
                    ViewTask::account(ctx, &timer);
                    return Ok(out);
                }
            }

            cells.clear();

            // Stop if the row limit is exceeded.
            self.rows += 1;
            if self.rows >= self.truncate_rows && self.truncate_rows != 0 {
                ViewTask::account(ctx, &timer);
                return Ok(out);
            }
        }

        ViewTask::account(ctx, &timer);
        Ok(out)
    }
}


#[cfg(test)]
mod tests {
    //! Port of `test/view_test.cpp`, plus layout cases the C++ test does not
    //! reach (wrapping, overage, the minimum-width error, truncation,
    //! breaks, empty columns, obfuscation).
    use super::*;
    use crate::columns::column::factory;
    use crate::context::{set_stdout_tty_override, task_rcdir, CONFIGURATION_DEFAULTS};
    use crate::shared::color::ColorId;
    use crate::sort::sort_tasks;
    use crate::test_util::{env_lock, EnvGuard};
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    /// The C++ `UnitTest` harness: counts executed assertions.
    #[derive(Default)]
    struct UnitTest {
        count: usize,
    }

    impl UnitTest {
        fn ok(&mut self, expression: bool, name: &str) {
            self.count += 1;
            assert!(expression, "{name}");
        }

        fn is<T: PartialEq + std::fmt::Debug>(&mut self, actual: T, expected: T, name: &str) {
            self.count += 1;
            assert_eq!(actual, expected, "{name}");
        }
    }

    /// A Context with the C++ test's configuration and an open replica. The
    /// TTY override, env vars and temp dir are restored/removed before the
    /// env lock is released (fields drop in declaration order).
    struct Fixture {
        ctx: Context,
        _tty: TtyReset,
        _env: EnvGuard,
        _dir: TempDir,
        _lock: MutexGuard<'static, ()>,
    }

    struct TtyReset;
    impl Drop for TtyReset {
        fn drop(&mut self) {
            set_stdout_tty_override(None);
        }
    }

    fn fixture(tty: bool) -> Fixture {
        let lock = env_lock();
        let dir = TempDir::new().unwrap();
        set_stdout_tty_override(Some(tty));
        let mut ctx = Context::new();
        ctx.tdb2.open_replica_at(dir.path().to_str().unwrap(), true, false).unwrap();

        // Ensure environment has no influence.
        let env = EnvGuard::save(&["TASKDATA", "TASKRC"]);
        std::env::remove_var("TASKDATA");
        std::env::remove_var("TASKRC");

        // Set up configuration.
        ctx.config.parse(CONFIGURATION_DEFAULTS, 0, &[task_rcdir()], "").unwrap();
        // C++ `set (key, true)` resolves to the int overload: "1".
        ctx.config.set("fontunderline", 1);
        ctx.config.set("tag.indicator", "+");
        ctx.config.set("dependency.indicator", "D");
        ctx.config.set("recurrence.indicator", "R");
        ctx.config.set("active.indicator", "A");
        ctx.config.set("dateformat", "Y-M-D");
        ctx.config.set("indent.annotation", "2");

        // Deviation: `getWidth` with `detection` queries TIOCGWINSZ on stdout,
        // which fails under ctest (width 0). Pin that result so the test does
        // not depend on the terminal `cargo test` runs in.
        ctx.terminal_width = 0;
        ctx.terminal_height = 1;

        Fixture { ctx, _tty: TtyReset, _env: env, _dir: dir, _lock: lock }
    }

    /// The body of C++ `view_test.cpp`; returns the rendered view.
    fn view_test(ctx: &mut Context, t: &mut UnitTest, tty: bool) -> String {
        // Two sample tasks.
        t.ok(true, "zero");
        let mut t1 = Task::from_input(
            ctx,
            "{\
             \"status\":\"pending\", \
             \"uuid\":\"2a64f6e0-bf8e-430d-bf71-9ec3f0d9b661\", \
             \"description\":\"This is the description text\", \
             \"project\":\"Home\", \
             \"priority\":\"H\", \
             \"annotation_1234567890\":\"This is an annotation\", \
             \"start\":\"1234567890\", \
             \"due\":\"1234567890\", \
             \"tags\":\"one,two\"\
             }",
        )
        .unwrap();
        t1.id = 1;
        t.ok(true, "one");
        let mut t2 = Task::from_input(
            ctx,
            "{\
             \"status\":\"pending\", \
             \"uuid\":\"f30cb9c3-3fc0-483f-bfb2-3bf134f00694\", \
             \"description\":\"This is the description text\", \
             \"project\":\"Garden Care\", \
             \"recur\":\"monthly\", \
             \"depends\":\"2a64f6e0-bf8e-430d-bf71-9ec3f0d9b661\"\
             }",
        )
        .unwrap();
        t2.id = 11;
        t.ok(true, "two");
        let mut t3 = Task::from_input(
            ctx,
            "{\
             \"status\":\"pending\", \
             \"uuid\":\"c44cb9c3-3fc0-483f-bfb2-3bf134f05554\", \
             \"description\":\"Another description\", \
             \"project\":\"Garden\"\
             }",
        )
        .unwrap();
        t3.id = 8;
        t.ok(true, "three");

        let mut data = vec![t1, t2, t3];

        // Sequence of tasks.
        let mut sequence: Vec<usize> = vec![0, 1, 2];

        sort_tasks(ctx, &mut data, &mut sequence, "description+,id-").unwrap();

        // Create colors.
        let header_color = Color::from_ids(ColorId::Yellow, ColorId::Nocolor, false, false, false);
        let odd_color = Color::parse("on gray1").unwrap();
        let even_color = Color::parse("on gray0").unwrap();

        // Create a view.
        let report = "view.t";
        let mut view = ViewTask::new();
        for name in [
            "id",
            "uuid.short",
            "project",
            "priority",
            "tags",
            "tags.count",
            "description",
            "depends.indicator",
            "recur.indicator",
            "status.short",
            "due.countdown",
            "start.active",
            "urgency",
        ] {
            view.add(factory(name, report, &ctx.config).unwrap(), false);
        }
        let width = ctx.get_width();
        view.width(width);
        view.left_margin(4);
        view.extra_padding(0);
        view.intra_padding(1);
        view.color_header(&header_color);
        view.color_odd(&odd_color);
        view.color_even(&even_color);
        view.intra_color_odd(&odd_color);
        view.intra_color_even(&even_color);

        // Render the view.
        let out = view.render(ctx, &mut data, &sequence).unwrap();
        let expected_lines = if tty { 5 } else { 6 };

        t.is(view.lines(), expected_lines, "View::lines == 5");

        // Now render a string-only grid.
        ctx.config.set("fontunderline", 0);
        let _single_cell = Color::parse("bold white on red").unwrap();

        out
    }

    /// The `due.countdown` cell of the 2009-02-13 due date, computed as
    /// `ColumnTypeDate` does (it depends on "now"); 8 characters until 2109.
    fn countdown() -> String {
        use crate::shared::datetime::Datetime;
        use crate::shared::duration::Duration;
        let s = Duration::from_seconds(Datetime::from_epoch(1_234_567_890) - Datetime::new()).format_vague(true);
        assert_eq!(s.len(), 8, "countdown {s:?}");
        s
    }

    #[test]
    fn view_test_not_a_tty() {
        let mut f = fixture(false);
        let mut t = UnitTest::default();
        let out = view_test(&mut f.ctx, &mut t, false);
        assert_eq!(t.count, 5, "view_test.cpp executes 5 assertions");

        // Additional: the exact table (C++ prints it to stdout).
        let count = countdown();
        let h = |label: &str| format!("\x1b[33m{label}\x1b[0m");
        let header: Vec<String> = [
            "ID", "UUID    ", "Project    ", "Priority", "Tags   ", "Tag", "Description                       ",
            "R", "St", "Count   ", "A", "Urgency",
        ]
        .iter()
        .map(|l| h(l))
        .collect();
        let dashes: Vec<String> =
            [2, 8, 11, 8, 7, 3, 34, 1, 2, 8, 1, 7].iter().map(|&w| h(&"-".repeat(w))).collect();
        let expected = format!(
            "    {}\n    {}\n\
             \x20    8 c44cb9c3 Garden                           Another description                  P                   0\n\
             \x20   11 f30cb9c3 Garden Care                      This is the description text       R P                   0\n\
             \x20    1 2a64f6e0 Home        H        one two [2] This is the description text         P  {count} A       0\n\
             \x20                                                  2009-02-13 This is an annotation\n",
            header.join(" "),
            dashes.join(" "),
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn view_test_tty() {
        let mut f = fixture(true);
        let mut t = UnitTest::default();
        let out = view_test(&mut f.ctx, &mut t, true);
        assert_eq!(t.count, 5, "view_test.cpp executes 5 assertions");

        // Additional: the exact table, underlined header and alternating rows.
        let count = countdown();
        let header: Vec<String> = [
            "ID", "UUID    ", "Project    ", "Priority", "Tags   ", "Tag", "Description                       ",
            "R", "St", "Count   ", "A", "Urgency",
        ]
        .iter()
        .map(|l| format!("\x1b[4;33m{l}\x1b[0m"))
        .collect();
        let row = |code: u32, cells: &[&str]| {
            let bg = |s: &str| format!("\x1b[48;5;{code}m{s}\x1b[0m");
            let cells: Vec<String> = cells.iter().map(|c| bg(c)).collect();
            format!("    {}\n", cells.join(&bg(" ")))
        };
        let mut expected = format!("    {}\n", header.join(" "));
        expected += &row(
            232,
            &[" 8", "c44cb9c3", "Garden     ", "        ", "       ", "   ", "Another description               ", " ", "P ", "        ", " ", "      0"],
        );
        expected += &row(
            233,
            &["11", "f30cb9c3", "Garden Care", "        ", "       ", "   ", "This is the description text      ", "R", "P ", "        ", " ", "      0"],
        );
        expected += &row(
            232,
            &[" 1", "2a64f6e0", "Home       ", "H       ", "one two", "[2]", "This is the description text      ", " ", "P ", &count, "A", "      0"],
        );
        expected += &row(
            232,
            &["  ", "        ", "           ", "        ", "       ", "   ", "  2009-02-13 This is an annotation", " ", "  ", "        ", " ", "       "],
        );
        assert_eq!(out, expected);
    }

    /// A non-TTY Context with the default configuration.
    fn plain_fixture() -> Fixture {
        let mut f = fixture(false);
        f.ctx.config.set("fontunderline", 0);
        f
    }

    fn task(id: i32, project: &str, description: &str) -> Task {
        let mut t = Task::new();
        t.id = id;
        if !project.is_empty() {
            t.set("project", project);
        }
        t.set("description", description);
        t
    }

    fn view(ctx: &Context, columns: &[&str]) -> ViewTask {
        let mut view = ViewTask::new();
        for name in columns {
            view.add(factory(name, "test", &ctx.config).unwrap(), false);
        }
        view
    }

    #[test]
    fn overage_is_spread_and_text_wraps() {
        let mut f = plain_fixture();
        let mut data = vec![task(1, "", "alpha beta gamma delta")];
        let mut v = view(&f.ctx, &["id", "description"]);
        // minimal: ID 2, Description 11 (both labels; the annotation
        // minimum only applies with annotations); ideal 2, 22; extra 1.
        v.width(20);
        let out = v.render(&mut f.ctx, &mut data, &[0]).unwrap();
        assert_eq!(out, "ID Description\n-- -----------------\n 1 alpha beta gamma\n   delta\n");
        assert_eq!(v.lines(), 4);
        assert_eq!(v.rows(), 1);
        assert!(f.ctx.debug_messages.iter().any(|m| m == "ViewTask::render min=14 ideal=25 overage=6 width=20"));

        // Exactly the minimum.
        let mut v = view(&f.ctx, &["id", "description"]);
        v.width(14);
        let out = v.render(&mut f.ctx, &mut data, &[0]).unwrap();
        assert_eq!(out, "ID Description\n-- -----------\n 1 alpha beta\n   gamma delta\n");

        // Width 0 is unlimited; everything fits.
        let mut v = view(&f.ctx, &["id", "description"]);
        let out = v.render(&mut f.ctx, &mut data, &[0]).unwrap();
        assert_eq!(out, "ID Description\n-- ----------------------\n 1 alpha beta gamma delta\n");
    }

    #[test]
    fn insufficient_width_shortens_longest_columns_or_errors() {
        let mut f = plain_fixture();
        let mut data = vec![task(1, "Home", "alpha beta")];
        // minimal: Project 7, Description 11 (labels); extra 1.

        // Case 1: the longest column bears all the shortening (11 - 3 = 8).
        let mut v = view(&f.ctx, &["project", "description"]);
        v.width(16);
        let out = v.render(&mut f.ctx, &mut data, &[0]).unwrap();
        assert_eq!(out, "Project Description\n------- --------\nHome    alpha\n        beta\n");
        assert!(f.ctx.errors.is_empty());

        // Case 2: both shortened to 7, then by half the rest (rounded up): 5, 5.
        let mut v = view(&f.ctx, &["project", "description"]);
        v.width(12);
        let out = v.render(&mut f.ctx, &mut data, &[0]).unwrap();
        assert_eq!(out, "Project Description\n----- -----\nHome  alpha\n      beta\n");
        assert!(f.ctx.errors.is_empty());

        // No solution: queued as an error, rendering continues at 7, 7.
        let mut v = view(&f.ctx, &["project", "description"]);
        v.width(2);
        let out = v.render(&mut f.ctx, &mut data, &[0]).unwrap();
        assert_eq!(
            f.ctx.errors,
            vec!["The report has a minimum width of 19 and does not fit in the available width of 2.".to_string()]
        );
        assert_eq!(out, "Project Description\n------- -------\nHome    alpha\n        beta\n");
    }

    #[test]
    fn empty_columns_are_dropped_unless_printed() {
        let mut f = plain_fixture();
        let mut data = vec![task(1, "", "one"), task(2, "", "two")];

        let mut v = view(&f.ctx, &["id", "project", "description"]);
        let out = v.render(&mut f.ctx, &mut data, &[0, 1]).unwrap();
        assert_eq!(out, "ID Description\n-- -----------\n 1 one\n 2 two\n");

        f.ctx.config.set("print.empty.columns", 1);
        let mut v = view(&f.ctx, &["id", "project", "description"]);
        let out = v.render(&mut f.ctx, &mut data, &[0, 1]).unwrap();
        assert_eq!(out, "ID Project Description\n-- ------- -----------\n 1         one\n 2         two\n");
    }

    #[test]
    fn breaks_margins_padding_and_truncation() {
        let mut f = plain_fixture();
        let mut data = vec![task(1, "A", "one"), task(2, "A", "two"), task(3, "B", "three")];

        let mut v = view(&f.ctx, &["id", "project", "description"]);
        v.left_margin(2);
        v.extra_padding(1);
        v.intra_padding(2);
        v.add_break("project");
        let out = v.render(&mut f.ctx, &mut data, &[0, 1, 2]).unwrap();
        assert_eq!(
            out,
            "   ID  Project  Description\n   --  -------  -----------\n    1  A        one\n    2  A        two\n\n    3  B        three\n"
        );
        assert_eq!(v.lines(), 6);
        assert_eq!(v.rows(), 3);

        // Row limit: also limits the measurement ("three" is not measured).
        let mut v = view(&f.ctx, &["id", "project", "description"]);
        v.truncate_rows(2);
        let out = v.render(&mut f.ctx, &mut data, &[0, 1, 2]).unwrap();
        assert_eq!(out, "ID Project Description\n-- ------- -----------\n 1 A       one\n 2 A       two\n");
        assert_eq!((v.lines(), v.rows()), (4, 2));

        // Line limit inside the headers.
        let mut v = view(&f.ctx, &["id", "project", "description"]);
        v.truncate_lines(1);
        let out = v.render(&mut f.ctx, &mut data, &[0, 1, 2]).unwrap();
        assert_eq!(out, "ID Project Description\n");
        assert_eq!((v.lines(), v.rows()), (1, 0));

        // Line limit inside the rows.
        let mut v = view(&f.ctx, &["id", "project", "description"]);
        v.truncate_lines(3);
        let out = v.render(&mut f.ctx, &mut data, &[0, 1, 2]).unwrap();
        assert_eq!(out, "ID Project Description\n-- ------- -----------\n 1 A       one\n");
        assert_eq!((v.lines(), v.rows()), (3, 0));
    }

    #[test]
    fn obfuscation_covers_string_columns() {
        let mut f = plain_fixture();
        f.ctx.config.set("obfuscate", 1);
        let mut data = vec![task(1, "Home", "secret plan")];
        let mut v = view(&f.ctx, &["id", "project", "description"]);
        let out = v.render(&mut f.ctx, &mut data, &[0]).unwrap();
        assert_eq!(out, "ID Project Description\n-- ------- -----------\n 1 xxxx    xxxxxx xxxx\n");
    }

    #[test]
    fn colours_sort_header_and_extra_padding() {
        let mut f = fixture(true);
        f.ctx.config.set("fontunderline", 0);
        let mut data = vec![task(1, "", "one"), task(2, "", "two")];
        let red = Color::parse("red").unwrap();
        let blue = Color::parse("blue").unwrap();
        let on_green = Color::parse("on green").unwrap();

        let mut v = ViewTask::new();
        v.add(factory("id", "test", &f.ctx.config).unwrap(), true);
        v.add(factory("description", "test", &f.ctx.config).unwrap(), false);
        v.color_sort_header(&blue);
        v.color_header(&red);
        v.extra_padding(1);
        v.extra_color_odd(&on_green);
        v.extra_color_even(&on_green);
        let out = v.render(&mut f.ctx, &mut data, &[0, 1]).unwrap();
        let g = "\x1b[42m \x1b[0m";
        assert_eq!(
            out,
            format!(
                " \x1b[34mID\x1b[0m \x1b[31mDescription\x1b[0m\n \x1b[34m--\x1b[0m \x1b[31m-----------\x1b[0m\n\
                 {g} 1 one        {g}\n{g} 2 two        {g}\n"
            )
        );

        // colorHeader also sets the sort header when none is set.
        let mut v = ViewTask::new();
        v.add(factory("id", "test", &f.ctx.config).unwrap(), true);
        v.color_header(&red);
        let out = v.render(&mut f.ctx, &mut data, &[0]).unwrap();
        assert_eq!(out, "\x1b[31mID\x1b[0m\n\x1b[31m--\x1b[0m\n 1\n");
    }
}
