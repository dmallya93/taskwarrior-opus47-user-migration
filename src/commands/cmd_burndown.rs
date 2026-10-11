//! Rust translation of `src/commands/CmdBurndown.cpp`.
//!
//! `task <filter> burndown.{daily,weekly,monthly,annual}`: a stacked ASCII
//! bar chart of pending/started/done task counts per period, going back from
//! today, plus the net fix rate and an estimated completion date.
//!
//! Each `CmdBurndown*` struct is one C++ class from `CmdBurndown.cpp` (the
//! `CMDBURNDOWN_CTOR` macro expansions); all four run [`run_burndown`] with
//! their period character. The C++ helper classes `Bar` and `Chart` and the
//! `TaskEpochRange` struct are private to this module, as in C++.

use std::collections::{BTreeMap, HashMap};

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::filter::Filter;
use crate::shared::color::Color;
use crate::shared::datetime::Datetime;
use crate::shared::duration::Duration;
use crate::shared::format::{left_justify, printf_f, stream_double};
use crate::task::{Status, Task};

////////////////////////////////////////////////////////////////////////////////
/// C++ `class Bar`.
#[derive(Clone, Default)]
struct Bar {
    /// from left of chart
    _offset: i32,
    /// x-axis label, major (year/-/month)
    _major_label: String,
    /// x-axis label, minor (month/week/day)
    _minor_label: String,
    /// Number of pending tasks in period
    _pending: i32,
    /// Number of started tasks in period
    _started: i32,
    /// Number of done tasks in period
    _done: i32,
}

////////////////////////////////////////////////////////////////////////////////
// Task counting pattern.
// In the original version of the burndown chart, the names here corresponded
// literally to the terms in the gathering algorithm. Following a rewrite to
// pre-compute the epochs, the relationship of these dates is more indirect
// (see code comments throughout the file), but the broad semantics remain
// similar.
//
// If you're familiar with the old version of the burndown code, note that
// we no longer count deleted tasks except for peak calculation.
//
//   e = entry
//   s = start
//   C = end/Completed
//   D = end/Deleted
//   > = Pending/Waiting
//
//   ID  30 31 01 02 03 04 05 06 07 08 09 10
//   --  ------------------------------------
//   1          e-----s--C                    completed
//   2             e--s-----D                 deleted (peak count only)
//   3                e-----s-------------->  pending, started
//   4                   e----------------->  pending
//   5                               e----->  pending
//   --  ------------------------------------
//   PP         1  1  2  2  1  1  1  2  2  2
//   SS                     1  1  1  1  1  1
//   DD                  1  1  1  1  1  1  1
//   --  ------------------------------------
// Peak         1  2  3  4  3  2  2  3  3  3

/// C++ `class Chart`.
struct Chart {
    /// Terminal width
    _width: i32,
    /// Terminal height
    _height: i32,
    /// Width of plot area
    _graph_width: i32,
    /// Height of plot area
    _graph_height: i32,
    /// Largest value of pending+started+done +carried over done
    _max_value: i32,
    /// Longest y-axis label
    _max_label: i32,
    /// Max number of bars that fit the terminal
    _estimated_bars: i32,
    /// Max no. of bars that fit given y-axis label width
    _actual_bars: i32,
    /// Epoch-indexed set of bars
    _bars: BTreeMap<i64, Bar>,
    /// Date of earliest estimated bar
    _earliest: Datetime,
    /// Number of 'done' tasks prior to chart range
    _carryover_done: i32,
    /// D, W, M, Y
    _period: char,
    /// All consecutive day epochs (for peak range)
    _peak_day_epochs: Vec<i64>,
    /// Difference array for peak counting, quantized by day
    _peak_diff: Vec<i32>,
    /// a map of per-day epochs to _peak_diff
    _peak_day_index: HashMap<i64, usize>,
    /// Day of highest pending peak
    _peak_day: i64,
    /// Corresponding peak pending count
    _peak_count: i32,
    /// Current count of tasks without end date
    _current_count: i32,
    /// Calculated fix rate (tasks/day)
    _net_fix_rate: f32,
    /// Estimated completion date
    _completion: String,
}

/// C++ `std::string::replace (pos, len, str)` on the byte grid: replaces up
/// to `len` bytes at `pos` (clamped to the end) with `text`, which may be
/// longer or shorter, shifting everything after it.
///
/// # Errors
/// C++ throws `std::out_of_range` when `pos` is past the end; no Taskwarrior
/// `catch` names it, so it is reported as [`TwError::Runtime`] ("Unknown
/// error. Please report.").
fn replace(grid: &mut Vec<u8>, pos: i64, len: usize, text: &[u8]) -> Result<()> {
    if pos < 0 || pos as usize > grid.len() {
        return Err(TwError::Runtime(
            "basic_string::replace: __pos (which is out of range)".to_string(),
        ));
    }
    let pos = pos as usize;
    let end = pos + len.min(grid.len() - pos);
    grid.splice(pos..end, text.iter().copied());
    Ok(())
}

impl Chart {
    /// C++ `#define LOC(y, x) (((y) * (_width + 1)) + (x))`.
    fn loc(&self, y: i64, x: i64) -> i64 {
        (y * (self._width as i64 + 1)) + x
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::Chart (char type)`.
    fn new(context: &mut Context, type_: char) -> Self {
        // How much space is there to render in?  This chart will occupy the
        // maximum space, and the width drives various other parameters.
        let width = context.get_width();
        let height = context
            .get_height()
            .wrapping_sub(context.config.get_integer("reserved.lines"))
            .wrapping_sub(1); // Allow for new line with prompt.
        let max_label = 1;

        Chart {
            _width: width,
            _height: height,
            _graph_height: height.wrapping_sub(7),
            _graph_width: width.wrapping_sub(max_label).wrapping_sub(14),
            _max_value: 0,
            _max_label: max_label,
            // Estimate how many 'bars' can be displayed.  This will help subset a
            // potentially enormous data set.
            _estimated_bars: (width.wrapping_sub(1).wrapping_sub(14)) / 3,
            _actual_bars: 0,
            _bars: BTreeMap::new(),
            _earliest: Datetime::new(),
            _carryover_done: 0,
            _period: type_,
            _peak_day_epochs: Vec::new(),
            _peak_diff: Vec::new(),
            _peak_day_index: HashMap::new(),
            _peak_day: 0,
            _peak_count: 0,
            _current_count: 0,
            _net_fix_rate: 0.0,
            _completion: String::new(),
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::accumulateTasks`.
    fn accumulate_tasks(
        &mut self,
        tasks: &[Task],
        ranges: &[TaskEpochRange],
        now_epoch: i64,
        cumulative: bool,
    ) -> Result<()> {
        for i in 0..tasks.len() {
            // ranges is the taskEpochRange returned by findTaskEpochRange().
            let r = &ranges[i];
            let task = &tasks[i];

            // Peak and _current_count are made regardless of the task status.
            self.peak_add(r.peak_entry, r.peak_end);
            if r.end_epoch.is_none() {
                self._current_count += 1;
            }

            let status = task.get_status()?;

            // Deleted/recurring tasks contribute to the peak and _current_count,
            // but make no contribution to the chart's bars.
            if status == Status::Deleted || status == Status::Recurring {
                continue;
            }

            if status == Status::Pending || status == Status::Waiting {
                // Pending tasks contribute to the pending bars through [entry, last],
                // inclusive of values equal to last. For pending tasks, last_epoch will be
                // either the now_epoch or end_epoch.
                self.for_bars_through(r.entry_epoch, r.last_epoch, |b| b._pending += 1);

                // If the task has a start date, bars from that date through last_epoch
                // change from pending to started.
                if let Some(start_epoch) = r.start_epoch {
                    self.for_bars_through(start_epoch, r.last_epoch, |b| {
                        b._pending -= 1;
                        b._started += 1;
                    });
                }
            } else if status == Status::Completed {
                let end_epoch = completed_end(r);

                // Completed tasks are counted as pending exclusive of their
                // end_epoch. The peak count, however, includes values equal to
                // end_epoch. See findTaskEpochRange() for details of assignment.
                self.for_bars_before(r.entry_epoch, end_epoch, |b| b._pending += 1);

                if cumulative {
                    // In cumulative mode, completed tasks range through
                    // [max(entry, end), today], including values equal to today.
                    // Nb: now_epoch is quantized by the period of the chart mode,
                    // ie. daily/weekly/monthly.
                    let done_start_epoch = r.entry_epoch.max(end_epoch);
                    self.for_bars_through(done_start_epoch, now_epoch, |b| b._done += 1);

                    // If the end date of a completed task is before the leftmost
                    // rendered bar we need to carry over the count. _earliest is simply
                    // the earliest epoch in the dataset.
                    if end_epoch < self._earliest.to_epoch() {
                        self._carryover_done += 1;
                    }
                } else {
                    // In non-cumulative mode, each end date of a completed task counts
                    // only for the bars of that day.
                    let done_epoch = r.entry_epoch.max(end_epoch);
                    if let Some(done_bar) = self._bars.get_mut(&done_epoch) {
                        done_bar._done += 1;
                    }
                }
            }
        }
        Ok(())
    }

    /// The C++ `peak_add` lambda of `accumulateTasks`: maps epochs to their
    /// indices, and then keeps a difference array via `_peak_diff`. This array
    /// is then prefix-summed by `finalize()`, giving us the `_peak_count` for
    /// each day. The `_peak_day_index` is always quantized by day, regardless
    /// of the mode the chart runs in.
    fn peak_add(&mut self, first_day: i64, last_day: i64) {
        let (Some(&fi), Some(&li)) = (
            self._peak_day_index.get(&first_day),
            self._peak_day_index.get(&last_day),
        ) else {
            return;
        };
        self._peak_diff[fi] += 1;
        if li + 1 < self._peak_diff.len() {
            self._peak_diff[li + 1] -= 1;
        }
    }

    /// The C++ `for_bars_through` lambda: operates on each bar in
    /// `[start, last]`, including values equal to last.
    fn for_bars_through(&mut self, start: i64, last: i64, fn_: impl Fn(&mut Bar)) {
        for (_, bar) in self._bars.range_mut(start..).take_while(|(k, _)| **k <= last) {
            fn_(bar);
        }
    }

    /// The C++ `for_bars_before` lambda: operates on each bar in
    /// `[start, end)`, excluding values equal to end.
    fn for_bars_before(&mut self, start: i64, end_excl: i64, fn_: impl Fn(&mut Bar)) {
        for (_, bar) in self._bars.range_mut(start..).take_while(|(k, _)| **k < end_excl) {
            fn_(bar);
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    // Graph should render like this:
    //   +---------------------------------------------------------------------+
    //   |                                                                     |
    //   | 20 |                                                                |
    //   |    |                            DD DD DD DD DD DD DD DD             |
    //   |    |          DD DD DD DD DD DD DD DD DD DD DD DD DD DD             |
    //   |    | PP PP SS SS SS SS SS SS SS SS SS DD DD DD DD DD DD   DD Done   |
    //   | 10 | PP PP PP PP PP PP SS SS SS SS SS SS DD DD DD DD DD   SS Started|
    //   |    | PP PP PP PP PP PP PP PP PP PP PP SS SS SS SS DD DD   PP Pending|
    //   |    | PP PP PP PP PP PP PP PP PP PP PP PP PP PP PP SS DD             |
    //   |    | PP PP PP PP PP PP PP PP PP PP PP PP PP PP PP PP PP             |
    //   |  0 +----------------------------------------------------            |
    //   |      21 22 23 24 25 26 27 28 29 30 31 01 02 03 04 05 06             |
    //   |      July                             August                        |
    //   |                                                                     |
    //   |      ADD rate 1.7/d           Estimated completion 8/12/2010        |
    //   |      Don/Delete rate  1.3/d                                         |
    //   +---------------------------------------------------------------------+
    /// C++ `Chart::render`.
    ///
    /// The grid is a byte string, as in C++: every `grid.replace` is the
    /// `std::string::replace` splice (see [`replace`]), including the major
    /// x-axis labels that insert one more byte than they replace.
    fn render(&mut self, context: &mut Context) -> Result<String> {
        if self._graph_height < 5 || // a 4-line graph is essentially unreadable.
            self._graph_width < 2
        // A single-bar graph is useless.
        {
            return Ok("Terminal window too small to draw a graph.\n".to_string());
        } else if self._graph_height > 1000 || // each line is a string allloc
            self._graph_width > 1000
        {
            return Ok("Terminal window too large to draw a graph.\n".to_string());
        }

        if self._max_value == 0 {
            context.footnote("No matches.");
        }

        // Create a grid, folded into a string.
        let width = self._width as usize;
        let mut grid: Vec<u8> = Vec::with_capacity(self._height as usize * (width + 1));
        for _ in 0..self._height {
            grid.extend(std::iter::repeat_n(b' ', width));
            grid.push(b'\n');
        }

        // Title.
        let mut title = match self._period {
            'D' => "Daily",
            'W' => "Weekly",
            'M' => "Monthly",
            'Y' => "Annual",
            _ => "Monthly",
        }
        .to_string();
        title += " Burndown";
        let at = self.loc(0, (self._width as i64 - title.len() as i64) / 2);
        replace(&mut grid, at, title.len(), title.as_bytes())?;

        // Legend.
        let gh = self._graph_height as i64;
        let w = self._width as i64;
        let at = self.loc(gh / 2 - 1, w - 10);
        replace(&mut grid, at, 10, ("DD ".to_string() + &left_justify("Done", 7)).as_bytes())?;
        let at = self.loc(gh / 2, w - 10);
        replace(&mut grid, at, 10, ("SS ".to_string() + &left_justify("Started", 7)).as_bytes())?;
        let at = self.loc(gh / 2 + 1, w - 10);
        replace(&mut grid, at, 10, ("PP ".to_string() + &left_justify("Pending", 7)).as_bytes())?;

        // Determine y-axis labelling.
        let labels = self.y_labels();
        // Digit count of the high y-axis label (avoids log10 FP).
        self._max_label = 1;
        let mut t = labels[2];
        while t >= 10 {
            self._max_label += 1;
            t /= 10;
        }
        let ml = self._max_label as i64;

        // Draw y-axis.
        for i in 0..gh {
            let at = self.loc(i + 1, ml + 1);
            replace(&mut grid, at, 1, b"|")?;
        }

        // Draw y-axis labels.
        let label = snprintf_12(&format!("{:>width$}", labels[2], width = ml as usize));
        let at = self.loc(1, ml - label.len() as i64);
        replace(&mut grid, at, label.len(), label.as_bytes())?;
        let label = snprintf_12(&format!("{:>width$}", labels[1], width = ml as usize));
        let at = self.loc(1 + (gh / 2), ml - label.len() as i64);
        replace(&mut grid, at, label.len(), label.as_bytes())?;
        let at = self.loc(gh + 1, ml - 1);
        replace(&mut grid, at, 1, b"0")?;

        // Draw x-axis.
        let h = self._height as i64;
        let at = self.loc(h - 6, ml + 1);
        replace(&mut grid, at, 1, b"+")?;
        let at = self.loc(h - 6, ml + 2);
        let axis = vec![b'-'; self._graph_width as usize];
        replace(&mut grid, at, self._graph_width as usize, &axis)?;

        // Draw x-axis labels. _bars is a std::map (already sorted by epoch).
        let mut last_major = String::new();
        for bar in self._bars.values() {
            // If it fits within the allowed space.
            if bar._offset < self._actual_bars {
                let col = ml + 3 + ((self._actual_bars - bar._offset - 1) as i64 * 3);
                let at = self.loc(h - 5, col);
                replace(
                    &mut grid,
                    at,
                    bar._minor_label.len(),
                    bar._minor_label.as_bytes(),
                )?;

                if last_major != bar._major_label {
                    let at = self.loc(h - 4, col - 1);
                    let text = " ".to_string() + &bar._major_label;
                    replace(&mut grid, at, bar._major_label.len(), text.as_bytes())?;
                }

                last_major = bar._major_label.clone();
            }
        }

        // Draw bars.
        for bar in self._bars.values() {
            // If it fits within the allowed space.
            if bar._offset < self._actual_bars {
                let col = ml + 3 + ((self._actual_bars - bar._offset - 1) as i64 * 3);
                let pending = (bar._pending * self._graph_height) / labels[2];
                let started = ((bar._pending + bar._started) * self._graph_height) / labels[2];
                let done = ((bar._pending + bar._started + bar._done + self._carryover_done)
                    * self._graph_height)
                    / labels[2];

                let mut draw_segment = |from: i32, to: i32, tag: &[u8]| -> Result<()> {
                    for b in from..to {
                        let at = self.loc(gh - b as i64, col);
                        replace(&mut grid, at, 2, tag)?;
                    }
                    Ok(())
                };
                draw_segment(0, pending, b"PP")?;
                draw_segment(pending, started, b"SS")?;
                draw_segment(started, done, b"DD")?;
            }
        }

        // Draw rates.
        self.calculate_rates(context)?;
        let rate = if self._net_fix_rate != 0.0 {
            snprintf_12(&(printf_f(self._net_fix_rate as f64, 1) + "/d"))
        } else {
            snprintf_12("-")
        };

        let at = self.loc(h - 2, ml + 3);
        replace(
            &mut grid,
            at,
            22 + rate.len(),
            ("Net Fix Rate:         ".to_string() + &rate).as_bytes(),
        )?;

        // Draw completion date.
        if !self._completion.is_empty() {
            let at = self.loc(h - 1, ml + 3);
            replace(
                &mut grid,
                at,
                22 + self._completion.len(),
                ("Estimated completion: ".to_string() + &self._completion).as_bytes(),
            )?;
        }

        optimize_grid(&mut grid);

        // Scan the grid char-by-char and replace the PP/SS/DD markers with
        // colored or plain equivalents. Non-marker characters are copied.
        let use_color = context.color();
        let color_pending = Color::parse(&context.config.get("color.burndown.pending"))?;
        let color_done = Color::parse(&context.config.get("color.burndown.done"))?;
        let color_started = Color::parse(&context.config.get("color.burndown.started"))?;

        let mut result: Vec<u8> = Vec::with_capacity(grid.len() * 2);
        let mut pos = 0;
        while pos < grid.len() {
            if pos + 1 < grid.len() {
                let (c1, c2) = (grid[pos], grid[pos + 1]);
                let marker = match (c1, c2) {
                    (b'P', b'P') => Some((&color_pending, " X")),
                    (b'S', b'S') => Some((&color_started, " +")),
                    (b'D', b'D') => Some((&color_done, " .")),
                    _ => None,
                };
                if let Some((color, plain)) = marker {
                    if use_color {
                        result.extend_from_slice(color.colorize("  ").as_bytes());
                    } else {
                        result.extend_from_slice(plain.as_bytes());
                    }
                    pos += 2;
                    continue;
                }
            }
            result.push(grid[pos]);
            pos += 1;
        }

        Ok(String::from_utf8_lossy(&result).into_owned())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::increment`.
    fn increment(input: &Datetime, period: char) -> Datetime {
        // Move to the next period.
        let mut d = input.day();
        let mut m = input.month();
        let mut y = input.year();

        match period {
            'D' => {
                d += 1;
                if d > Datetime::days_in_month(y, m) {
                    d = 1;

                    m += 1;
                    if m == 13 {
                        m = 1;
                        y += 1;
                    }
                }
            }

            'W' => {
                d += 7;
                let days = Datetime::days_in_month(y, m);
                if d > days {
                    d -= days;

                    m += 1;
                    if m == 13 {
                        m = 1;
                        y += 1;
                    }
                }
            }

            'M' => {
                d = 1;
                m += 1;
                if m == 13 {
                    m = 1;
                    y += 1;
                }
            }

            'Y' => {
                d = 1;
                m = 1;
                y += 1;
            }

            _ => {}
        }

        Datetime::from_ymdhms(y, m, d, 0, 0, 0)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::buildPeakRange`: build the epoch range used for peak
    /// counting.  The peak is always quantized by day regardless of chart mode.
    fn build_peak_range(&mut self, earliest_day: i64, latest_day: i64) {
        self._peak_diff.clear();
        self._peak_day_epochs.clear();
        self._peak_day_index.clear();

        let mut cursor = Datetime::from_epoch(earliest_day);
        let end = Datetime::from_epoch(latest_day);
        while cursor <= end {
            let ep = cursor.to_epoch();
            self._peak_day_index.insert(ep, self._peak_diff.len());
            self._peak_day_epochs.push(ep);
            self._peak_diff.push(0);
            cursor = Chart::increment(&cursor, 'D');
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::decrement`.
    fn decrement(input: &Datetime, period: char) -> Datetime {
        // Move to the previous period.
        let mut d = input.day();
        let mut m = input.month();
        let mut y = input.year();

        match period {
            'D' => {
                d -= 1;
                if d == 0 {
                    m -= 1;
                    if m == 0 {
                        m = 12;
                        y -= 1;
                    }

                    d = Datetime::days_in_month(y, m);
                }
            }

            'W' => {
                d -= 7;
                if d < 1 {
                    m -= 1;
                    if m == 0 {
                        m = 12;
                        y -= 1;
                    }

                    d += Datetime::days_in_month(y, m);
                }
            }

            'M' => {
                d = 1;
                m -= 1;
                if m == 0 {
                    m = 12;
                    y -= 1;
                }
            }

            'Y' => {
                d = 1;
                m = 1;
                y -= 1;
            }

            _ => {}
        }

        Datetime::from_ymdhms(y, m, d, 0, 0, 0)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::generateBars`: create one bar per period going backwards
    /// from today. Each bar is keyed by the quantized epoch of the chart mode
    /// (daily/weekly/monthly). `_earliest` is the leftmost bar's epoch, used
    /// for carrying over the done count in cumulative mode.
    ///
    /// # Errors
    /// `Datetime::week` throws for a weekstart other than Sunday/Monday.
    fn generate_bars(&mut self) -> Result<()> {
        let mut bar = Bar::default(); // future contributor: if you change the logic used for counting,
                                      // such that this function changes the counts, be mindful:
                                      // because we copy the struct, your state would be carried through
                                      // into each loop.
        let mut cursor = quantize(&Datetime::new(), self._period);

        // Iterate backwards from today, generating labels for each bar.
        for i in 0..self._estimated_bars {
            // Create the major and minor labels.
            match self._period {
                'D' => {
                    // month/day
                    let month = Datetime::month_name(cursor.month());
                    bar._major_label = month[..3].to_string();

                    bar._minor_label = format!("{:02}", cursor.day());
                }

                'W' => {
                    // year/week
                    bar._major_label = format!("{}", cursor.year());

                    bar._minor_label = format!("{:02}", cursor.try_week()?);
                }

                'M' => {
                    // year/month
                    bar._major_label = format!("{}", cursor.year());

                    bar._minor_label = format!("{:02}", cursor.month());
                }

                'Y' => {
                    // 2-digit year abbreviation
                    bar._minor_label = format!("{:02}", cursor.year() % 100);
                    bar._major_label = String::new();
                }

                _ => {}
            }

            bar._offset = i;
            self._bars.insert(cursor.to_epoch(), bar.clone());

            // Record the earliest date, used as the cutoff for carrying over done
            // counts in cumulative mode.
            self._earliest = cursor;

            // Move to the previous period.
            cursor = Chart::decrement(&cursor, self._period);
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::finalize`: compute the peak pending count from the
    /// accumulated quantized data, then size the bar data for rendering.
    fn finalize(&mut self) {
        // _peak_diff is a difference array, here we prefix-sum it to get the
        // actual counts.
        self._peak_count = 0;
        self._peak_day = 0;
        let mut running = 0;
        for i in 0..self._peak_diff.len() {
            running += self._peak_diff[i];
            if running > self._peak_count {
                self._peak_count = running;
                self._peak_day = self._peak_day_epochs[i];
            }
        }
        self.maxima();
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::maxima`.
    fn maxima(&mut self) {
        self._max_value = 0;
        self._max_label = 1;

        for bar in self._bars.values() {
            // pending + started + done + carryover.
            let total = bar._pending + bar._started + bar._done + self._carryover_done;

            // Determine _max_value.
            if total > self._max_value {
                self._max_value = total;
            }

            // Count of the tallest bar. Shouldn't == 0.
            if total > 0 {
                let mut length = 1;
                let mut t = total;
                while t >= 10 {
                    length += 1;
                    t /= 10;
                }
                if length > self._max_label {
                    self._max_label = length;
                }
            }
        }

        // How many bars can be shown?
        self._actual_bars = (self._width - self._max_label - 14) / 3;
        self._graph_width = self._width - self._max_label - 14;
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::yLabels`: given the vertical chart area size
    /// (graph_height), the largest value (_max_value), populate a vector of
    /// labels for the y axis.
    fn y_labels(&self) -> [i32; 3] {
        // Calculate max Y using a nice algorithm that rounds the data.
        let high = Chart::burndown_size(self._max_value as u32) as i32;
        let half = high / 2;

        [0, half, high]
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::calculateRates`.
    fn calculate_rates(&mut self, context: &mut Context) -> Result<()> {
        // Q: Why is this equation written out as a debug message?
        // A: People are going to want to know how the rates and the completion date
        //    are calculated.  This may also help debugging.
        let peak_message = format!(
            "Chart::calculateRates Maximum of {} pending tasks on {}, with currently {} pending tasks",
            self._peak_count,
            Datetime::from_epoch(self._peak_day).to_iso(),
            self._current_count
        );
        context.debug(&peak_message);

        // If there are no current pending tasks, then it is meaningless to find
        // rates or estimated completion date.
        if self._current_count == 0 {
            return Ok(());
        }

        let now = Datetime::new(); // unquantized current time - fix rate should be independent
                                   // of chart quantization mode.

        // If there is a net fix rate, and the peak was at least three days ago.
        let peak = Datetime::from_epoch(self._peak_day);
        if self._peak_count > self._current_count && (now - peak) > 3 * 86400 {
            // Fixes per second.  Not a large number. Multiplied by 86400 to get
            // fixes per day.
            let fix_rate = 1.0 * (self._peak_count - self._current_count) as f64
                / (now.to_epoch() - self._peak_day) as f64;
            self._net_fix_rate = (fix_rate * 86400.0) as f32;

            let rate_message = format!(
                "Chart::calculateRates Net reduction is {} tasks in {} = {} tasks/d",
                self._peak_count - self._current_count,
                Duration::from_seconds(now.to_epoch() - self._peak_day).format_iso(),
                stream_double(self._net_fix_rate as f64)
            );
            context.debug(&rate_message);

            let delta = Duration::from_seconds((self._current_count as f64 / fix_rate) as i64);
            let end = now + delta.to_time_t();

            // Prefer dateformat.report over dateformat.
            let mut format = context.config.get("dateformat.report");
            if format.is_empty() {
                format = context.config.get("dateformat");
                if format.is_empty() {
                    format = "Y-M-D".to_string();
                }
            }

            self._completion = end.to_string(&format) + " (" + &delta.format_vague(false) + ")";

            let completion_message = format!(
                "Chart::calculateRates ({} tasks / {}) = {} --> {}",
                self._current_count,
                stream_double(self._net_fix_rate as f64),
                delta.format(),
                end.to_iso()
            );
            context.debug(&completion_message);
        } else {
            self._completion = "No convergence".to_string();
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::round_up_to` (unsigned arithmetic).
    fn round_up_to(n: u32, target: u32) -> u32 {
        n.wrapping_add(target).wrapping_sub(n % target)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `Chart::burndown_size`.
    fn burndown_size(ntasks: u32) -> u32 {
        // Nearest 2
        if ntasks < 20 {
            return Chart::round_up_to(ntasks, 2);
        }

        // Nearest 10
        if ntasks < 50 {
            return Chart::round_up_to(ntasks, 10);
        }

        // Nearest 20
        if ntasks < 100 {
            return Chart::round_up_to(ntasks, 20);
        }

        // Choose the number from here rounded up to the nearest 10% of the next
        // highest power of 10 or half of power of 10.  UINT_MAX has 10 digits,
        // so we iterate from i=2 (we handled 5/10/50/100 above) to i=9.
        const COUNT: u32 = 10;
        let mut half: u32 = 500;
        let mut full: u32 = 1000;

        // We start at two because we handle 5, 10, 50, and 100 above.
        for _ in 2..COUNT {
            if ntasks < half {
                return Chart::round_up_to(ntasks, half / 10);
            }

            if ntasks < full {
                return Chart::round_up_to(ntasks, full / 10);
            }

            half = half.wrapping_mul(10);
            full = full.wrapping_mul(10);
        }

        // Round up to max of unsigned.
        u32::MAX
    }
}

/// C++ `snprintf (buf, 12, ...)`: at most 11 bytes of the formatted text.
fn snprintf_12(text: &str) -> String {
    let mut end = text.len().min(11);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `Chart::optimizeGrid`: `grid =~ /\s+$//g`.
///
/// C++ restarts `grid.find (" \n")` from the beginning on every pass; nothing
/// before the previous match can match again (the bytes before it are
/// untouched and held no `" \n"`), so the search here resumes at the last
/// non-space byte kept, or at 0 after the whole-prefix case, with the same
/// result.
fn optimize_grid(grid: &mut Vec<u8>) {
    let mut from = 0;
    while let Some(found) = grid[from..].windows(2).position(|w| w == b" \n") {
        let ws = from + found;
        let mut non_ws = ws;
        while non_ws > 0 && grid[non_ws] == b' ' {
            non_ws -= 1;
        }
        if grid[non_ws] == b' ' {
            // The entire line up to ws is spaces; nothing to trim before.
            grid.splice(0..ws + 1, *b"\n");
            from = 0;
            continue;
        }

        let end = (non_ws + 1 + (ws - non_ws + 1)).min(grid.len());
        grid.splice(non_ws + 1..end, *b"\n");
        from = non_ws;
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ free function `quantize`.
fn quantize(input: &Datetime, period: char) -> Datetime {
    match period {
        'D' => input.start_of_day(),
        'W' => input.start_of_week(),
        'M' => input.start_of_month(),
        'Y' => input.start_of_year(),
        _ => *input,
    }
}

/// C++ `struct TaskEpochRange`.
///
///  - Bar fields (first/last/entry/end_epoch): quantized by chart period
///    (D/W/M), used for rendered bars.
///  - Peak fields (peak_entry/peak_end): quantized by day, used for the
///    peak pending count.
#[derive(Clone, Copy)]
struct TaskEpochRange {
    /// leftmost bar this task touches
    #[allow(dead_code)]
    first_epoch: i64,
    /// rightmost bar this task touches
    last_epoch: i64,
    /// quantized entry by period
    entry_epoch: i64,
    /// quantized end by period; `None` when task has no end
    end_epoch: Option<i64>,
    /// quantized entry by day
    peak_entry: i64,
    /// quantized end by day, or today when no end
    peak_end: i64,
    /// quantized start by period, `None` if no start
    start_epoch: Option<i64>,
}

/// C++ `*end_epoch` for a completed task ("completed tasks always have an
/// end").
///
/// Deviation: dereferencing an empty `std::optional` is undefined behaviour
/// in C++ (in practice an indeterminate value); a completed task without an
/// `end` (only possible through hand-edited data) uses epoch 0 here.
fn completed_end(r: &TaskEpochRange) -> i64 {
    r.end_epoch.unwrap_or(0)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `findTaskEpochRange`: compute the bar-epoch range and quantized
/// attribute epochs for one task.
///
/// Used by `scan_epochs` to size the peak range, and by `accumulateTasks` to
/// pre-skip zero-contribution tasks. Per-bar contributions (pending/
/// started/done) are computed in `accumulateTasks` from `entry_epoch` and
/// `end_epoch`, because `last_epoch` is not always the per-bar boundary,
/// eg. for a completed task in cumulative mode, `last_epoch` extends to today.
/// `now_epoch` is now quantized by chart period; `now_day_epoch` is now
/// quantized by day.
fn find_task_epoch_range(
    task: &Task,
    now_epoch: i64,
    now_day_epoch: i64,
    period: char,
    cumulative: bool,
) -> Result<TaskEpochRange> {
    let entry_date = Datetime::from_epoch(task.get_date("entry"));
    let entry_epoch = quantize(&entry_date, period).to_epoch();
    // The peak count is always quantized by day, regardless of chart period.
    let peak_entry = quantize(&entry_date, 'D').to_epoch();

    let mut start_epoch = None;
    if task.has("start") {
        start_epoch = Some(quantize(&Datetime::from_epoch(task.get_date("start")), period).to_epoch());
    }

    // Quantize end epochs once.
    // When absent, peak_end defaults to today.
    let mut end_epoch = None;
    let mut peak_end = now_day_epoch;
    if task.has("end") {
        let end_date = Datetime::from_epoch(task.get_date("end"));
        end_epoch = Some(quantize(&end_date, period).to_epoch());
        peak_end = quantize(&end_date, 'D').to_epoch();
    }

    let status = task.get_status()?;

    // Pending/waiting tasks extend to today if they lack an end.
    if status == Status::Pending || status == Status::Waiting {
        let last_epoch = end_epoch.unwrap_or(now_epoch);
        return Ok(TaskEpochRange {
            first_epoch: entry_epoch,
            last_epoch,
            entry_epoch,
            end_epoch,
            peak_entry,
            peak_end,
            start_epoch,
        });
    }

    // Completed tasks extend to end (non-cumulative) or today (cumulative).
    if status == Status::Completed {
        let e = end_epoch.unwrap_or(0); // completed tasks always have an end (see completed_end)
        let first_epoch = entry_epoch.min(e); // we handle the case where
                                              // a task has an end before
                                              // entry for some reason
        let last_epoch = if cumulative {
            e.max(now_epoch)
        } else {
            e.max(entry_epoch)
        };
        return Ok(TaskEpochRange {
            first_epoch,
            last_epoch,
            entry_epoch,
            end_epoch,
            peak_entry,
            peak_end,
            start_epoch,
        });
    }

    // Deleted/recurring tasks don't contribute to bars, but they do to the
    // peak count.
    let last_epoch = match end_epoch {
        Some(end) => end.max(entry_epoch),
        None => entry_epoch,
    };
    Ok(TaskEpochRange {
        first_epoch: entry_epoch,
        last_epoch,
        entry_epoch,
        end_epoch,
        peak_entry,
        peak_end,
        start_epoch,
    })
}

/// C++ `struct ScanResult` (local to `runBurndown`).
struct ScanResult {
    ranges: Vec<TaskEpochRange>,
    earliest_day: i64,
    latest_day: i64,
}

////////////////////////////////////////////////////////////////////////////////
/// C++ static `runBurndown (char period, std::string& output)`.
fn run_burndown(context: &mut Context, period: char, output: &mut String) -> Result<i32> {
    let mut chart = Chart::new(context, period); // size the chart
    chart.generate_bars()?; // create the bar map from today backwards

    let has_filter = context.cli2.args.iter().any(|a| a.has_tag("FILTER"));

    // now quantized by chart period
    let now_epoch = quantize(&Datetime::new(), period).to_epoch();
    // now quantized by day, for peak counting
    let now_day_epoch = quantize(&Datetime::new(), 'D').to_epoch();
    let cumulative = if context.config.has("burndown.cumulative") {
        context.config.get_boolean("burndown.cumulative")
    } else {
        true
    };

    // First pass: walk all tasks to find the peak range quantized by day,
    // cache each task's taskEpochRange so accumulateTasks doesn't repeat
    // quantization work.
    // Returns {ranges, earliest_day, latest_day} for the task vector.
    let scan_epochs =
        |tasks: &[Task], earliest_day_in: i64, latest_day_in: i64| -> Result<ScanResult> {
            let mut result = ScanResult {
                ranges: Vec::with_capacity(tasks.len()),
                earliest_day: earliest_day_in,
                latest_day: latest_day_in,
            };
            for task in tasks {
                let r = find_task_epoch_range(task, now_epoch, now_day_epoch, period, cumulative)?;
                result.ranges.push(r);
                if r.peak_entry < result.earliest_day {
                    result.earliest_day = r.peak_entry;
                }
                if r.peak_end > result.latest_day {
                    result.latest_day = r.peak_end;
                }
            }
            Ok(result)
        };

    if has_filter {
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;
        let sr = scan_epochs(&filtered, now_day_epoch, 0)?;
        chart.build_peak_range(sr.earliest_day, sr.latest_day);
        chart.accumulate_tasks(&filtered, &sr.ranges, now_epoch, cumulative)?;
    } else {
        let pending: Vec<Task> = context.tdb2.pending_tasks()?.to_vec();
        let completed: Vec<Task> = context.tdb2.completed_tasks()?.to_vec();
        let srp = scan_epochs(&pending, now_day_epoch, 0)?;
        let src = scan_epochs(&completed, srp.earliest_day, srp.latest_day)?;
        chart.build_peak_range(src.earliest_day, src.latest_day);
        chart.accumulate_tasks(&pending, &srp.ranges, now_epoch, cumulative)?;
        chart.accumulate_tasks(&completed, &src.ranges, now_epoch, cumulative)?;
    }

    chart.finalize();
    *output = chart.render(context)?;
    Ok(0)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdBurndownMonthly` (`burndown.monthly`).
pub struct CmdBurndownMonthly {
    meta: CommandMeta,
}

impl CmdBurndownMonthly {
    /// C++ `CmdBurndownMonthly::CmdBurndownMonthly` (expanded from `CMDBURNDOWN_CTOR`).
    pub fn new() -> Self {
        CmdBurndownMonthly {
            meta: CommandMeta {
                keyword: "burndown.monthly".into(),
                usage: "task <filter> burndown.monthly".into(),
                description: "Shows a graphical burndown chart, by monthly".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
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

impl Default for CmdBurndownMonthly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdBurndownMonthly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdBurndownMonthly::execute` (expanded from `CMDBURNDOWN_CTOR`):
    /// `runBurndown ('M', output)`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        run_burndown(context, 'M', output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdBurndownWeekly` (`burndown.weekly`).
pub struct CmdBurndownWeekly {
    meta: CommandMeta,
}

impl CmdBurndownWeekly {
    /// C++ `CmdBurndownWeekly::CmdBurndownWeekly` (expanded from `CMDBURNDOWN_CTOR`).
    pub fn new() -> Self {
        CmdBurndownWeekly {
            meta: CommandMeta {
                keyword: "burndown.weekly".into(),
                usage: "task <filter> burndown.weekly".into(),
                description: "Shows a graphical burndown chart, by weekly".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
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

impl Default for CmdBurndownWeekly {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdBurndownWeekly {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdBurndownWeekly::execute` (expanded from `CMDBURNDOWN_CTOR`):
    /// `runBurndown ('W', output)`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        run_burndown(context, 'W', output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdBurndownDaily` (`burndown.daily`).
pub struct CmdBurndownDaily {
    meta: CommandMeta,
}

impl CmdBurndownDaily {
    /// C++ `CmdBurndownDaily::CmdBurndownDaily` (expanded from `CMDBURNDOWN_CTOR`).
    pub fn new() -> Self {
        CmdBurndownDaily {
            meta: CommandMeta {
                keyword: "burndown.daily".into(),
                usage: "task <filter> burndown.daily".into(),
                description: "Shows a graphical burndown chart, by daily".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
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

impl Default for CmdBurndownDaily {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdBurndownDaily {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdBurndownDaily::execute` (expanded from `CMDBURNDOWN_CTOR`):
    /// `runBurndown ('D', output)`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        run_burndown(context, 'D', output)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdBurndownAnnual` (`burndown.annual`).
pub struct CmdBurndownAnnual {
    meta: CommandMeta,
}

impl CmdBurndownAnnual {
    /// C++ `CmdBurndownAnnual::CmdBurndownAnnual` (expanded from `CMDBURNDOWN_CTOR`).
    pub fn new() -> Self {
        CmdBurndownAnnual {
            meta: CommandMeta {
                keyword: "burndown.annual".into(),
                usage: "task <filter> burndown.annual".into(),
                description: "Shows a graphical burndown chart, by annual".into(),
                read_only: true,
                displays_id: false,
                needs_gc: true,
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

impl Default for CmdBurndownAnnual {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdBurndownAnnual {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdBurndownAnnual::execute` (expanded from `CMDBURNDOWN_CTOR`):
    /// `runBurndown ('Y', output)`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        run_burndown(context, 'Y', output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    #[test]
    fn burndown_size_rounds_like_cpp() {
        // Nearest 2 / 10 / 20, always strictly above the input.
        assert_eq!(Chart::burndown_size(0), 2);
        assert_eq!(Chart::burndown_size(1), 2);
        assert_eq!(Chart::burndown_size(2), 4);
        assert_eq!(Chart::burndown_size(19), 20);
        assert_eq!(Chart::burndown_size(20), 30);
        assert_eq!(Chart::burndown_size(49), 50);
        assert_eq!(Chart::burndown_size(50), 60);
        assert_eq!(Chart::burndown_size(99), 100);
        // Nearest 10% of the next half/full power of ten.
        assert_eq!(Chart::burndown_size(100), 150);
        assert_eq!(Chart::burndown_size(499), 500);
        assert_eq!(Chart::burndown_size(500), 600);
        assert_eq!(Chart::burndown_size(999), 1000);
        assert_eq!(Chart::burndown_size(1000), 1500);
        assert_eq!(Chart::burndown_size(4_000_000_000), u32::MAX);
        assert_eq!(Chart::round_up_to(7, 5), 10);
        assert_eq!(Chart::round_up_to(10, 5), 15);
    }

    #[test]
    fn replace_is_std_string_replace() {
        let mut grid = b"abcdef".to_vec();
        replace(&mut grid, 1, 2, b"XY").unwrap();
        assert_eq!(grid, b"aXYdef");
        // A longer replacement inserts (the major x-axis label quirk).
        replace(&mut grid, 1, 1, b" XY").unwrap();
        assert_eq!(grid, b"a XYYdef");
        // The count is clamped at the end; pos == size appends.
        replace(&mut grid, 6, 10, b"Z").unwrap();
        assert_eq!(grid, b"a XYYdZ");
        replace(&mut grid, 7, 0, b"!").unwrap();
        assert_eq!(grid, b"a XYYdZ!");
        assert!(matches!(
            replace(&mut grid, 9, 1, b"?"),
            Err(TwError::Runtime(_))
        ));
    }

    #[test]
    fn optimize_grid_trims_trailing_spaces() {
        let mut grid = b"ab  \n    \n c \nd\n".to_vec();
        optimize_grid(&mut grid);
        assert_eq!(grid, b"ab\n\n c\nd\n");
        // A first line made only of spaces: C++ replaces [0, ws] by "\n",
        // keeping the line's own newline too.
        let mut grid = b"   \nx \n".to_vec();
        optimize_grid(&mut grid);
        assert_eq!(grid, b"\n\nx\n");
    }

    #[test]
    fn snprintf_truncates_to_eleven_bytes() {
        assert_eq!(snprintf_12("-"), "-");
        assert_eq!(snprintf_12("12345678.0/d"), "12345678.0/");
    }

    #[test]
    fn increment_and_decrement_wrap() {
        let d = Datetime::from_ymdhms(2021, 12, 31, 0, 0, 0);
        assert_eq!(
            Chart::increment(&d, 'D'),
            Datetime::from_ymdhms(2022, 1, 1, 0, 0, 0)
        );
        assert_eq!(
            Chart::increment(&d, 'W'),
            Datetime::from_ymdhms(2022, 1, 7, 0, 0, 0)
        );
        assert_eq!(
            Chart::increment(&d, 'M'),
            Datetime::from_ymdhms(2022, 1, 1, 0, 0, 0)
        );
        assert_eq!(
            Chart::increment(&d, 'Y'),
            Datetime::from_ymdhms(2022, 1, 1, 0, 0, 0)
        );
        let d = Datetime::from_ymdhms(2021, 3, 1, 0, 0, 0);
        assert_eq!(
            Chart::decrement(&d, 'D'),
            Datetime::from_ymdhms(2021, 2, 28, 0, 0, 0)
        );
        assert_eq!(
            Chart::decrement(&d, 'W'),
            Datetime::from_ymdhms(2021, 2, 22, 0, 0, 0)
        );
        let d = Datetime::from_ymdhms(2021, 1, 15, 0, 0, 0);
        assert_eq!(
            Chart::decrement(&d, 'M'),
            Datetime::from_ymdhms(2020, 12, 1, 0, 0, 0)
        );
        assert_eq!(
            Chart::decrement(&d, 'Y'),
            Datetime::from_ymdhms(2020, 1, 1, 0, 0, 0)
        );
        assert_eq!(Chart::increment(&d, 'x'), d.start_of_day());
    }

    fn run(
        command: &dyn Command,
        seed: &[&str],
        rc: &str,
        args: &[&str],
    ) -> (Result<i32>, String, CommandFixture) {
        let mut f = CommandFixture::new(
            seed,
            &format!("defaultwidth=80\ndefaultheight=24\ndetection=off\n{rc}"),
            args,
        );
        let mut output = String::new();
        let result = command.execute(&mut f.ctx, &mut output);
        (result, output, f)
    }

    const SEED: [&str; 3] = [
        r#"{"description":"one","entry":"20200101T000000Z"}"#,
        r#"{"description":"two","entry":"20200101T000000Z","start":"20200102T000000Z"}"#,
        r#"{"description":"three","status":"completed","entry":"20200101T000000Z","end":"20200103T000000Z"}"#,
    ];

    #[test]
    fn daily_chart_layout() {
        let (rc, out, f) = run(&CmdBurndownDaily::new(), &SEED, "", &["burndown.daily"]);
        assert_eq!(rc.unwrap(), 0);
        let lines: Vec<&str> = out.split('\n').collect();
        // 24 - reserved.lines (1) - 1 rows, each trimmed of trailing spaces.
        assert_eq!(lines.len(), 23, "{out}");
        assert_eq!(lines[0], format!("{}Daily Burndown", " ".repeat(33)));
        // Every bar holds 1 pending, 1 started and (cumulative) 1 done task,
        // plus 1 carried-over done (the 2020 end is before the leftmost
        // bar; C++ counts it in both): max_value 4 -> labels 0/3/6.
        assert!(lines[1].starts_with("6 |"), "{out}");
        assert!(lines[8].starts_with("3 |"), "{out}");
        assert!(lines[16].starts_with("0 +---"), "{out}");
        // 21 bars at width 80. Segments: (1 * 15) / 6 = 2 rows pending,
        // (2 * 15) / 6 = 5, (4 * 15) / 6 = 10.
        let row = |m: &str| format!("  |{}", m.repeat(21));
        assert_eq!(lines[15], row("  X"), "{out}");
        assert_eq!(lines[14], row("  X"), "{out}");
        assert_eq!(lines[13], row("  +"), "{out}");
        assert_eq!(lines[11], row("  +"), "{out}");
        assert_eq!(lines[10], row("  ."), "{out}");
        assert_eq!(lines[9], row("  ."), "{out}");
        assert_eq!(lines[8], format!("3 |{}     X Pending", "  .".repeat(21)), "{out}");
        assert_eq!(lines[7], format!("  |{}     + Started", "  .".repeat(21)), "{out}");
        assert_eq!(lines[6], format!("  |{}     . Done", "  .".repeat(21)), "{out}");
        assert_eq!(lines[5], "  |", "{out}");
        // The peak (3 on 2020-01-01) is far in the past and above the
        // current count (2): a small positive rate and a completion estimate.
        // (The rate lines start at column 4 minus one byte per major x-axis
        // label inserted, i.e. one or two months.)
        assert!(lines[20].trim_start().starts_with("Net Fix Rate:         0.0/d"), "{out}");
        assert!(lines[21].trim_start().starts_with("Estimated completion: "), "{out}");
        assert_eq!(lines[22], "");
        assert!(f.ctx.footnotes.is_empty());
        drop(f);
    }

    #[test]
    fn titles_per_period() {
        let cases: [(&dyn Command, &str, &str); 4] = [
            (&CmdBurndownWeekly::new(), "burndown.weekly", "Weekly Burndown"),
            (&CmdBurndownMonthly::new(), "burndown.monthly", "Monthly Burndown"),
            (&CmdBurndownAnnual::new(), "burndown.annual", "Annual Burndown"),
            (&CmdBurndownDaily::new(), "burndown.daily", "Daily Burndown"),
        ];
        for (command, keyword, title) in cases {
            let (rc, out, f) = run(command, &SEED, "", &[keyword]);
            assert_eq!(rc.unwrap(), 0);
            assert!(out.starts_with(&format!(
                "{}{title}\n",
                " ".repeat((80 - title.len()) / 2)
            )));
            assert!(out.contains(" X") && out.contains(" +") && out.contains(" ."));
            drop(f);
        }
    }

    #[test]
    fn no_matches_footnote_and_no_rates() {
        let (rc, out, f) = run(
            &CmdBurndownDaily::new(),
            &SEED,
            "",
            &["description:nothing", "burndown.daily"],
        );
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(f.ctx.footnotes, vec!["No matches.".to_string()]);
        assert!(!out.contains("|  X"), "{out}");
        assert!(out.contains("Net Fix Rate:         -\n"), "{out}");
        assert!(!out.contains("Estimated completion"));
        // labels 0/1/2 for an empty chart.
        assert!(out.contains("\n2 |"));
        assert!(out.contains("\n1 |"));
        drop(f);
    }

    #[test]
    fn terminal_size_limits() {
        let (rc, out, f) = run(&CmdBurndownDaily::new(), &[], "defaultheight=11\n", &["burndown.daily"]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "Terminal window too small to draw a graph.\n");
        drop(f);
        let (rc, out, f) = run(&CmdBurndownDaily::new(), &[], "defaultwidth=16\n", &["burndown.daily"]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "Terminal window too small to draw a graph.\n");
        drop(f);
        let (rc, out, f) = run(&CmdBurndownMonthly::new(), &[], "defaultheight=1100\n", &["burndown.monthly"]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(out, "Terminal window too large to draw a graph.\n");
        drop(f);
    }

    #[test]
    fn colour_markers_and_bad_colour() {
        let (rc, out, f) = run(
            &CmdBurndownDaily::new(),
            &SEED,
            "_forcecolor=on\ncolor.burndown.pending=on red\n",
            &["burndown.daily"],
        );
        assert_eq!(rc.unwrap(), 0);
        assert!(!out.contains(" X"));
        assert!(out.contains("\x1b[41m  \x1b[0m"), "{out:?}");
        drop(f);
        let (rc, _, f) = run(
            &CmdBurndownDaily::new(),
            &SEED,
            "color.burndown.done=nocolour\n",
            &["burndown.daily"],
        );
        assert_eq!(
            rc.unwrap_err().to_string(),
            "The color 'nocolour' is not recognized."
        );
        drop(f);
    }

    #[test]
    fn cumulative_setting() {
        // A task completed in 2020 is carried over into every daily bar in
        // cumulative mode, and only counted on its end day otherwise.
        let (rc, cumulative, f) = run(&CmdBurndownDaily::new(), &SEED, "", &["burndown.daily"]);
        assert_eq!(rc.unwrap(), 0);
        drop(f);
        let (rc, plain, f) = run(
            &CmdBurndownDaily::new(),
            &SEED,
            "burndown.cumulative=0\n",
            &["burndown.daily"],
        );
        assert_eq!(rc.unwrap(), 0);
        assert!(cumulative.contains("|  .  ."), "{cumulative}");
        assert!(!plain.contains("|  ."), "{plain}");
        assert!(plain.contains("|  +  +"), "{plain}");
        drop(f);
    }

    #[test]
    fn debug_messages() {
        let (rc, _, f) = run(&CmdBurndownDaily::new(), &SEED, "", &["burndown.daily"]);
        assert_eq!(rc.unwrap(), 0);
        let messages = &f.ctx.debug_messages;
        assert!(messages.iter().any(|m| m.starts_with(
            "Chart::calculateRates Maximum of 3 pending tasks on 20"
        ) && m.ends_with(", with currently 2 pending tasks")), "{messages:?}");
        assert!(messages
            .iter()
            .any(|m| m.starts_with("Chart::calculateRates Net reduction is 1 tasks in P")));
        assert!(messages
            .iter()
            .any(|m| m.starts_with("Chart::calculateRates (2 tasks / ")));
        drop(f);
    }
}
