//! Rust translation of `libshared src/Table.cpp` / `Table.h`.
//!
//! Translated from GothenburgBitFactory/libshared at commit d899eba6.
//!
//! Ported in full ahead of milestone 3 (Color/Table/Variant/Eval), because
//! `version` (milestone 1) renders two one-column tables byte-exactly, and
//! audited line by line against the C++ in milestone 3. Every member of the
//! C++ class is here. `libshared test/table.t.cpp` is ported, assertion-count
//! checked, in `table_tests.rs`, together with byte-exact comparisons against
//! the output of the compiled C++ `Table::render`.
//!
//! The C++ overloads map to: `add (col)` -> [`Table::add`], `add (col,
//! alignLeft, wrap)` -> [`Table::add_with`]; `set (row, col, string[,
//! color])` -> [`Table::set`] / [`Table::set_colored`], `set (row, col,
//! int[, color])` -> [`Table::set_int`] / [`Table::set_int_colored`],
//! `set (row, col, color)` -> [`Table::set_color`].

use crate::shared::color::Color;
use crate::shared::format::{left_justify, obfuscate_text, right_justify};
use crate::shared::shared::{longest_line, longest_word, wrap_text};
use crate::shared::utf8::utf8_text_width;

////////////////////////////////////////////////////////////////////////////////
/// C++ `class Table`: a text grid with wrapping, alignment and colors.
#[derive(Debug, Clone)]
pub struct Table {
    data: Vec<Vec<String>>,
    color: Vec<Vec<Color>>,
    columns: Vec<String>,
    align: Vec<bool>,
    wrap: Vec<bool>,
    oddness: Vec<bool>,
    width: i32,
    left_margin: i32,
    header: Color,
    odd: Color,
    even: Color,
    intra_padding: i32,
    intra_odd: Color,
    intra_even: Color,
    extra_padding: i32,
    extra_odd: Color,
    extra_even: Color,
    truncate_lines: i32,
    truncate_rows: i32,
    force_color: bool,
    with_color: bool,
    obfuscate: bool,
    underline_headers: bool,
    lines: i32,
    rows: i32,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    /// A table with the C++ member initialisers.
    pub fn new() -> Self {
        Table {
            data: Vec::new(),
            color: Vec::new(),
            columns: Vec::new(),
            align: Vec::new(),
            wrap: Vec::new(),
            oddness: Vec::new(),
            width: 0,
            left_margin: 0,
            header: Color::from_value(0),
            odd: Color::from_value(0),
            even: Color::from_value(0),
            intra_padding: 1,
            intra_odd: Color::from_value(0),
            intra_even: Color::from_value(0),
            extra_padding: 0,
            extra_odd: Color::from_value(0),
            extra_even: Color::from_value(0),
            truncate_lines: 0,
            truncate_rows: 0,
            force_color: false,
            with_color: true,
            obfuscate: false,
            underline_headers: false,
            lines: 0,
            rows: 0,
        }
    }

    // View specifications.

    /// `Table::add (col, alignLeft = true, wrap = true)`.
    pub fn add(&mut self, col: &str) {
        self.add_with(col, true, true);
    }

    /// `Table::add (col, alignLeft, wrap)`.
    pub fn add_with(&mut self, col: &str, align_left: bool, wrap: bool) {
        self.columns.push(col.to_string());
        self.align.push(align_left);
        self.wrap.push(wrap);
    }

    /// `Table::width`.
    pub fn width(&mut self, width: i32) {
        self.width = width;
    }
    /// `Table::leftMargin`.
    pub fn left_margin(&mut self, margin: i32) {
        self.left_margin = margin;
    }
    /// `Table::colorHeader`.
    pub fn color_header(&mut self, c: Color) {
        self.header = c;
    }
    /// `Table::colorOdd`.
    pub fn color_odd(&mut self, c: Color) {
        self.odd = c;
    }
    /// `Table::colorEven`.
    pub fn color_even(&mut self, c: Color) {
        self.even = c;
    }
    /// `Table::intraPadding`.
    pub fn intra_padding(&mut self, padding: i32) {
        self.intra_padding = padding;
    }
    /// `Table::intraColorOdd`.
    pub fn intra_color_odd(&mut self, c: Color) {
        self.intra_odd = c;
    }
    /// `Table::intraColorEven`.
    pub fn intra_color_even(&mut self, c: Color) {
        self.intra_even = c;
    }
    /// `Table::extraPadding`.
    pub fn extra_padding(&mut self, padding: i32) {
        self.extra_padding = padding;
    }
    /// `Table::extraColorOdd`.
    pub fn extra_color_odd(&mut self, c: Color) {
        self.extra_odd = c;
    }
    /// `Table::extraColorEven`.
    pub fn extra_color_even(&mut self, c: Color) {
        self.extra_even = c;
    }
    /// `Table::truncateLines`.
    pub fn truncate_lines(&mut self, n: i32) {
        self.truncate_lines = n;
    }
    /// `Table::truncateRows`.
    pub fn truncate_rows(&mut self, n: i32) {
        self.truncate_rows = n;
    }
    /// `Table::forceColor` (unused by `render`, as in C++).
    pub fn force_color(&mut self) {
        self.force_color = true;
    }
    /// `Table::obfuscate`.
    pub fn obfuscate(&mut self) {
        self.obfuscate = true;
    }
    /// `Table::underlineHeaders`.
    pub fn underline_headers(&mut self) {
        self.underline_headers = true;
    }
    /// `Table::withColor`.
    pub fn with_color(&mut self, with_color: bool) {
        self.with_color = with_color;
    }
    /// `Table::lines`: lines rendered by the last `render`.
    pub fn lines(&self) -> i32 {
        self.lines
    }
    /// `Table::rows`: the number of data rows.
    pub fn rows(&self) -> i32 {
        self.data.len() as i32
    }

    // Data provision.

    fn push_row(&mut self, odd: bool) -> usize {
        self.data.push(vec![String::new(); self.columns.len()]);
        self.color.push(vec![Color::new(); self.columns.len()]);
        self.oddness.push(odd);
        self.data.len() - 1
    }

    /// `Table::addRow`: a row whose oddness alternates with its position.
    pub fn add_row(&mut self) -> usize {
        let odd = !(self.data.len() + 1).is_multiple_of(2);
        self.push_row(odd)
    }

    /// `Table::addRowOdd`.
    pub fn add_row_odd(&mut self) -> usize {
        self.push_row(true)
    }

    /// `Table::addRowEven`.
    pub fn add_row_even(&mut self) -> usize {
        self.push_row(false)
    }

    /// `Table::set (row, col, const std::string&)` (no color).
    pub fn set(&mut self, row: usize, col: usize, value: &str) {
        self.set_colored(row, col, value, Color::new());
    }

    /// `Table::set (row, col, const std::string&, const Color)`.
    pub fn set_colored(&mut self, row: usize, col: usize, value: &str, color: Color) {
        self.data[row][col] = value.to_string();

        if color.nontrivial() {
            self.color[row][col] = color;
        }
    }

    /// `Table::set (row, col, int)` (no color).
    pub fn set_int(&mut self, row: usize, col: usize, value: i32) {
        self.set_int_colored(row, col, value, Color::new());
    }

    /// `Table::set (row, col, int, const Color)`.
    pub fn set_int_colored(&mut self, row: usize, col: usize, value: i32, color: Color) {
        self.set_colored(row, col, &value.to_string(), color);
    }

    /// `Table::set (row, col, const Color)`.
    pub fn set_color(&mut self, row: usize, col: usize, color: Color) {
        if color.nontrivial() {
            self.color[row][col] = color;
        }
    }

    // View rendering.

    /// C++ `out.erase (out.find_last_not_of (' ') + 1)`.
    fn trim_right(out: &mut String) {
        let len = out.trim_end_matches(' ').len();
        out.truncate(len);
    }

    /// `Table::render`.
    ///
    /// Deviation: two inputs are undefined behaviour in the C++, and render
    /// deterministically here:
    /// - headers wrapping to different heights: the C++ reads
    ///   `headers[c][i]` past the end of the shorter column's lines; here the
    ///   missing line renders as blank (header-colored) padding;
    /// - `overage == 0` with every minimal width 0 but some ideal width
    ///   non-zero: the C++ leaves `widths` empty and indexes it; here the
    ///   minimal widths (all 0) are used.
    // The loops index several parallel per-column vectors, as the C++ does.
    #[allow(clippy::needless_range_loop)]
    pub fn render(&mut self) -> String {
        // Piped output disables color, unless overridden.
        if !self.with_color {
            self.header = Color::new();
            self.odd = Color::new();
            self.even = Color::new();
            self.intra_odd = Color::new();
            self.intra_even = Color::new();
            self.extra_odd = Color::new();
            self.extra_even = Color::new();

            for row in &mut self.color {
                for col in row.iter_mut() {
                    *col = Color::new();
                }
            }

            self.underline_headers = true;
        }

        let ncols = self.columns.len();

        // Determine minimal, ideal column widths.
        let mut minimal: Vec<i32> = Vec::with_capacity(ncols);
        let mut ideal: Vec<i32> = Vec::with_capacity(ncols);
        for col in 0..ncols {
            // Headers factor in to width calculations.
            let mut global_min = utf8_text_width(&self.columns[col]);
            let mut global_ideal = global_min;

            for row in &self.data {
                // Determine minimum and ideal width for this column.
                let (min, ideal) = Self::measure_cell(&row[col]);

                if min > global_min {
                    global_min = min;
                }
                if ideal > global_ideal {
                    global_ideal = ideal;
                }
            }

            minimal.push(global_min as i32);
            ideal.push(global_ideal as i32);
        }

        // Sum the minimal widths.
        let sum_minimal: i32 = minimal.iter().fold(0i32, |a, &c| a.wrapping_add(c));

        // Sum the ideal widths.
        let sum_ideal: i32 = ideal.iter().fold(0i32, |a, &c| a.wrapping_add(c));

        // Calculate final column widths. (C++ mixes int and size_t here; the
        // result truncated to int equals this wrapping int arithmetic.)
        let mut overage = self
            .width
            .wrapping_sub(self.left_margin)
            .wrapping_sub(self.extra_padding.wrapping_mul(2))
            .wrapping_sub((ncols as i32).wrapping_sub(1).wrapping_mul(self.intra_padding));

        let widths: Vec<i32> = if sum_ideal <= overage {
            ideal.clone()
        } else if sum_minimal > overage || overage < 0 {
            minimal.clone()
        } else if overage > 0 {
            let mut widths = minimal.clone();
            overage -= sum_minimal;

            // Spread 'overage' among columns where width[i] < ideal[i]
            while overage != 0 {
                for i in 0..ncols {
                    if overage == 0 {
                        break;
                    }
                    if widths[i] < ideal[i] {
                        widths[i] += 1;
                        overage -= 1;
                    }
                }
            }
            widths
        } else {
            // Deviation (see above): C++ leaves `widths` empty here
            // (overage == 0 == sum_minimal) and then indexes it; every minimal
            // width is 0, so use those.
            minimal.clone()
        };

        // Compose column headers.
        let mut max_lines = 0;
        let mut headers: Vec<Vec<String>> = Vec::with_capacity(ncols);
        for c in 0..ncols {
            let mut lines = Vec::new();
            Self::render_cell(&mut lines, &self.columns[c], widths[c], self.align[c], self.wrap[c], &self.header);

            if lines.len() > max_lines {
                max_lines = lines.len();
            }
            headers.push(lines);
        }

        // Output string.
        let mut out = String::new();
        self.lines = 0;

        // Render column headers.
        let left_margin = " ".repeat(self.left_margin.max(0) as usize);
        let extra = " ".repeat(self.extra_padding.max(0) as usize);
        let intra = " ".repeat(self.intra_padding.max(0) as usize);

        let extra_odd = self.extra_odd.colorize(&extra);
        let extra_even = self.extra_even.colorize(&extra);
        let intra_odd = self.intra_odd.colorize(&intra);
        let intra_even = self.intra_even.colorize(&intra);

        for i in 0..max_lines {
            out.push_str(&left_margin);
            out.push_str(&extra);

            for c in 0..ncols {
                if c != 0 {
                    out.push_str(&intra);
                }

                let blank = self.header.colorize(&" ".repeat(widths[c].max(0) as usize));
                if headers[c].len() < max_lines - i {
                    out.push_str(&blank);
                } else {
                    // Deviation (see above): C++ reads past the end here.
                    out.push_str(headers[c].get(i).unwrap_or(&blank));
                }
            }

            out.push_str(&extra);

            // Trim right.
            Self::trim_right(&mut out);
            out.push('\n');

            // Stop if the line limit is exceeded.
            self.lines += 1;
            if self.lines >= self.truncate_lines && self.truncate_lines != 0 {
                return out;
            }
        }

        // Underline headers with ------ if necessary.
        if self.underline_headers {
            out.push_str(&left_margin);
            out.push_str(&extra);
            for c in 0..ncols {
                if c != 0 {
                    out.push_str(&intra);
                }

                out.push_str(&self.header.colorize(&"-".repeat(widths[c].max(0) as usize)));
            }

            out.push('\n');
        }

        // Compose, render columns, in sequence.
        self.rows = 0;
        let mut cells: Vec<Vec<String>> = Vec::with_capacity(ncols);
        for row in 0..self.data.len() {
            let mut max_lines = 0;

            // Alternate rows based on |s % 2|
            let oddness = self.oddness[row];
            let row_color = if oddness { self.odd } else { self.even };

            // TODO row_color.blend (provided color);
            // TODO Problem: colors for columns are specified, not rows,
            //      therefore there are only cell colors, not intra colors.

            for col in 0..ncols {
                let mut cell_color = row_color;
                cell_color.blend(&self.color[row][col]);

                let mut lines = Vec::new();
                Self::render_cell(
                    &mut lines,
                    &self.data[row][col],
                    widths[col],
                    self.align[col],
                    self.wrap[col],
                    &cell_color,
                );

                if lines.len() > max_lines {
                    max_lines = lines.len();
                }

                if self.obfuscate {
                    for value in lines.iter_mut() {
                        *value = obfuscate_text(value);
                    }
                }
                cells.push(lines);
            }

            for i in 0..max_lines {
                out.push_str(&left_margin);
                out.push_str(if oddness { &extra_odd } else { &extra_even });

                for col in 0..ncols {
                    if col != 0 {
                        if row_color.nontrivial() {
                            out.push_str(&row_color.colorize(&intra));
                        } else {
                            out.push_str(if oddness { &intra_odd } else { &intra_even });
                        }
                    }

                    if i < cells[col].len() {
                        out.push_str(&cells[col][i]);
                    } else {
                        let mut cell_color = row_color;
                        cell_color.blend(&self.color[row][col]);
                        out.push_str(&cell_color.colorize(&" ".repeat(widths[col].max(0) as usize)));
                    }
                }

                out.push_str(if oddness { &extra_odd } else { &extra_even });

                // Trim right.
                Self::trim_right(&mut out);
                out.push('\n');

                // Stop if the line limit is exceeded.
                self.lines += 1;
                if self.lines >= self.truncate_lines && self.truncate_lines != 0 {
                    return out;
                }
            }

            cells.clear();

            // Stop if the row limit is exceeded.
            self.rows += 1;
            if self.rows >= self.truncate_rows && self.truncate_rows != 0 {
                return out;
            }
        }

        out
    }

    /// `Table::measureCell`: the (minimum, maximum) widths of `data`, i.e.
    /// its longest word and longest line, ignoring color codes.
    fn measure_cell(data: &str) -> (u32, u32) {
        let stripped = Color::strip(data);
        let maximum = longest_line(&stripped) as u32;
        let minimum = longest_word(&stripped) as u32;
        (minimum, maximum)
    }

    /// `Table::renderCell`: the colored, justified lines of `value` at
    /// `width`, wrapped if `wrap`.
    fn render_cell(lines: &mut Vec<String>, value: &str, width: i32, align_left: bool, wrap: bool, color: &Color) {
        let justify = |line: &str| {
            if align_left {
                color.colorize(&left_justify(line, width))
            } else {
                color.colorize(&right_justify(line, width))
            }
        };

        if wrap {
            let mut raw = Vec::new();
            wrap_text(&mut raw, value, width, false);

            for line in &raw {
                lines.push(justify(line));
            }
        } else {
            lines.push(justify(value));
        }
    }
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
