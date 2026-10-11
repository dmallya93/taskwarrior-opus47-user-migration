//! Port of libshared (d899eba6) `test/table.t.cpp`, plus byte-exact
//! comparisons against the C++ `Table::render`.
//!
//! The C++ `UnitTest` harness is mirrored by [`UnitTest`]: each `t.ok` of
//! table.t.cpp becomes one call with the identical message, and the run checks
//! the number of executed assertions against the static count of the C++ file
//! (3, equal to its `UnitTest t (3)` plan; the `t.fail` in the `catch` block is
//! unreachable, as `Table` throws nothing).
//!
//! [`GOLDEN`] holds the output of the upstream C++ for every table built by
//! [`scenarios`]; they cover column-width fitting, wrapping by display width,
//! dash underlines versus underline-colored headers, padding colors,
//! alternate-row colors, cell/row color blending, obfuscation, truncation,
//! `withColor (false)`, `forceColor` and the left margin.
use super::*;
use crate::shared::color::ColorId;

/// table.t.cpp: `UnitTest t (3)`.
const TABLE_T_ASSERTIONS: usize = 3;

/// The C++ `UnitTest` (test/test.cpp), recording instead of printing.
struct UnitTest {
    counter: usize,
    failures: Vec<String>,
}

impl UnitTest {
    fn new() -> Self {
        UnitTest {
            counter: 0,
            failures: Vec::new(),
        }
    }

    fn ok(&mut self, expression: bool, name: &str) {
        self.counter += 1;
        if !expression {
            self.failures
                .push(format!("not ok {} - {}", self.counter, name));
        }
    }

    /// Fails on any failed assertion, or when the number of executed
    /// assertions differs from the C++ file's.
    fn finish(self, expected: usize) {
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
        assert_eq!(
            self.counter, expected,
            "assertion count differs from the C++ test"
        );
    }
}

/// C++ `Color (const std::string&)`.
fn c(spec: &str) -> Color {
    Color::parse(spec).unwrap()
}

const LONG_TEXT: &str = "bottom center, containing sufficient text that \
                         wrapping will occur because it exceeds all \
                         reasonable values for default width.  Even in a \
                         very wide terminal window.  Just look at the \
                         lengths we must go to, to get passing unit tests \
                         and not flaky tests.";

/// table.t.cpp `main`, line by line.
#[test]
fn table_t() {
    let mut t = UnitTest::new();

    // Create colors.
    let header_color = Color::from_ids(ColorId::Yellow, ColorId::Nocolor, false, false, false);
    let odd_color = c("on gray1");
    let even_color = c("on gray0");

    // Now render a string-only grid.
    let single_cell = c("bold white on red");

    let mut t1 = Table::new();
    t1.width(80);
    t1.left_margin(4);
    t1.extra_padding(0);
    t1.intra_padding(1);
    t1.color_header(header_color);
    t1.color_odd(odd_color);
    t1.color_even(even_color);
    t1.intra_color_odd(odd_color);
    t1.intra_color_even(even_color);

    t1.add_with("Header1", true, true);
    t1.add_with("Header2", true, true);
    t1.add_with("Header3", false, true);

    let mut row = t1.add_row();
    t1.set(row, 0, "top left");
    t1.set(row, 1, "top center");
    t1.set(row, 2, "top right");

    row = t1.add_row();
    t1.set_colored(row, 0, "bottom left", single_cell);
    t1.set(row, 1, LONG_TEXT);
    t1.set(row, 2, "bottom right");

    print!("{}", t1.render());
    t.ok(t1.lines() > 4, "Table::lines > 4");

    // Test color behavior - with color enabled (default)
    let mut t3 = Table::new();
    t3.width(80);
    t3.add_with("Header", true, true);
    row = t3.add_row();
    t3.set_colored(row, 0, "cell with color", single_cell);
    let colored_output = t3.render();
    t.ok(
        colored_output.contains("\x1b["),
        "Colored output contains ANSI escape codes",
    );

    // Test color behavior - with color disabled
    let mut t4 = Table::new();
    t4.width(80);
    t4.with_color(false); // Disable color
    t4.add_with("Header", true, true);
    row = t4.add_row();
    t4.set_colored(row, 0, "cell without color", single_cell);
    let uncolored_output = t4.render();
    t.ok(
        !uncolored_output.contains("\x1b["),
        "Uncolored output does not contain ANSI escape codes",
    );

    // Chessboard example
    let mut t2 = Table::new();
    t2.width(32);
    t2.left_margin(4);
    t2.extra_padding(0);
    t2.intra_padding(0);

    for _ in 0..8 {
        t2.add_with("", true, true);
    }

    let blue = c("on bright blue");
    let white = c("on bright white");

    for row in 0..8 {
        t2.add_row();

        for col in 0..8 {
            if (row + col) % 2 != 0 {
                t2.set_colored(row, col, "  ", blue);
            } else {
                t2.set_colored(row, col, "  ", white);
            }
        }
    }

    print!("{}", t2.render());

    t.finish(TABLE_T_ASSERTIONS);
}

/// The tables whose C++ renderings are in [`GOLDEN`], in the same order.
fn scenarios() -> Vec<(String, Table)> {
    let mut v: Vec<(String, Table)> = Vec::new();
    let header_color = Color::from_ids(ColorId::Yellow, ColorId::Nocolor, false, false, false);
    let odd_color = c("on gray1");
    let even_color = c("on gray0");
    let single_cell = c("bold white on red");

    // table.t.cpp t1.
    let mut t1 = Table::new();
    t1.width(80);
    t1.left_margin(4);
    t1.extra_padding(0);
    t1.intra_padding(1);
    t1.color_header(header_color);
    t1.color_odd(odd_color);
    t1.color_even(even_color);
    t1.intra_color_odd(odd_color);
    t1.intra_color_even(even_color);
    t1.add_with("Header1", true, true);
    t1.add_with("Header2", true, true);
    t1.add_with("Header3", false, true);
    let r = t1.add_row();
    t1.set(r, 0, "top left");
    t1.set(r, 1, "top center");
    t1.set(r, 2, "top right");
    let r = t1.add_row();
    t1.set_colored(r, 0, "bottom left", single_cell);
    t1.set(r, 1, LONG_TEXT);
    t1.set(r, 2, "bottom right");
    v.push(("t1".into(), t1));

    // table.t.cpp t3, t4.
    let mut t3 = Table::new();
    t3.width(80);
    t3.add_with("Header", true, true);
    let r = t3.add_row();
    t3.set_colored(r, 0, "cell with color", single_cell);
    v.push(("t3".into(), t3));
    let mut t4 = Table::new();
    t4.width(80);
    t4.with_color(false);
    t4.add_with("Header", true, true);
    let r = t4.add_row();
    t4.set_colored(r, 0, "cell without color", single_cell);
    v.push(("t4".into(), t4));

    // table.t.cpp chessboard.
    let mut t2 = Table::new();
    t2.width(32);
    t2.left_margin(4);
    t2.extra_padding(0);
    t2.intra_padding(0);
    for _ in 0..8 {
        t2.add_with("", true, true);
    }
    let (blue, white) = (c("on bright blue"), c("on bright white"));
    for row in 0..8 {
        t2.add_row();
        for col in 0..8 {
            t2.set_colored(
                row,
                col,
                "  ",
                if (row + col) % 2 != 0 { blue } else { white },
            );
        }
    }
    v.push(("chessboard".into(), t2));

    // Dash underline under a colored header, extra/intra padding colors, margin.
    let mut t = Table::new();
    t.width(60);
    t.left_margin(2);
    t.extra_padding(2);
    t.intra_padding(3);
    t.color_header(c("bold blue"));
    t.underline_headers();
    t.extra_color_odd(c("on red"));
    t.extra_color_even(c("on green"));
    t.intra_color_odd(c("on cyan"));
    t.intra_color_even(c("on magenta"));
    t.add_with("Name", true, true);
    t.add_with("Value", false, true);
    for i in 0..4 {
        let r = t.add_row();
        t.set(r, 0, &format!("item {i}"));
        t.set_int(r, 1, i * 37);
    }
    v.push(("underline_padding_colors".into(), t));

    // Alternate colors as in Taskwarrior's setHeaderUnderline, explicit
    // odd/even rows, cell colors blended into row colors (16, 256, 24 bit).
    let mut t = Table::new();
    t.width(40);
    let alt = c("on gray4");
    t.color_odd(alt);
    t.intra_color_odd(alt);
    t.color_header(c("underline yellow"));
    t.add_with("A", true, true);
    t.add_with("B", true, true);
    let r = t.add_row_even();
    t.set(r, 0, "e1");
    t.set_colored(r, 1, "x", c("red"));
    let r = t.add_row_odd();
    t.set_colored(r, 0, "o1", c("bold"));
    t.set(r, 1, "y");
    let r = t.add_row_odd();
    t.set(r, 0, "o2");
    t.set_colored(r, 1, "z", c("color12"));
    let r = t.add_row();
    t.set(r, 0, "row3");
    t.set_colored(r, 1, "w", c("0x102030"));
    let r = t.add_row();
    t.set(r, 0, "row4");
    t.set_color(r, 1, c("on blue"));
    v.push(("alternate_blend".into(), t));

    // Obfuscation keeps color codes and spaces.
    let mut t = Table::new();
    t.width(20);
    t.obfuscate();
    t.color_odd(c("on gray2"));
    t.add_with("Desc", true, true);
    t.add_with("N", false, true);
    let r = t.add_row();
    t.set_colored(r, 0, "secret words here and more text", c("red"));
    t.set_int(r, 1, 42);
    let r = t.add_row();
    t.set(r, 0, "ünïcödé 日本語");
    t.set_int(r, 1, -7);
    v.push(("obfuscate".into(), t));

    // Line and row truncation.
    for tl in [0, 1, 2, 3, 5] {
        for tr in [0, 1, 2] {
            let mut t = Table::new();
            t.width(12);
            t.add_with("Head", true, true);
            for _ in 0..3 {
                let r = t.add_row();
                t.set(r, 0, "aaa bbb ccc ddd");
            }
            t.truncate_lines(tl);
            t.truncate_rows(tr);
            v.push((format!("truncate_l{tl}_r{tr}"), t));
        }
    }

    // Unwrapped and right-aligned columns.
    let mut t = Table::new();
    t.width(15);
    t.add_with("Left", true, false);
    t.add_with("Right", false, false);
    t.add_with("W", true, true);
    let r = t.add_row();
    t.set(r, 0, "a long unwrapped value");
    t.set(r, 1, "rv");
    t.set(r, 2, "wrap me please");
    v.push(("nowrap".into(), t));

    // Wide and combining characters.
    let mut t = Table::new();
    t.width(14);
    t.add_with("名前", true, true);
    t.add_with("説明", false, true);
    let r = t.add_row();
    t.set(r, 0, "日本語のテキスト");
    t.set(r, 1, "これは とても 長い 説明 です");
    let r = t.add_row();
    t.set(r, 0, "e\u{301}t\u{e9}");
    t.set(r, 1, "combining");
    v.push(("wide".into(), t));

    // Spreading the overage between the minimal and ideal widths.
    for w in [10, 20, 25, 30, 33, 40, 100] {
        let mut t = Table::new();
        t.width(w);
        t.add_with("One", true, true);
        t.add_with("Two", true, true);
        t.add_with("Three", false, true);
        let r = t.add_row();
        t.set(r, 0, "alpha beta gamma");
        t.set(r, 1, "x");
        t.set(r, 2, "delta epsilon zeta eta");
        v.push((format!("spread_w{w}"), t));
    }

    // Multi-line cells, cells containing color codes.
    let mut t = Table::new();
    t.width(50);
    t.underline_headers();
    t.add_with("Lines", true, true);
    t.add_with("Colored", true, true);
    let r = t.add_row();
    t.set(r, 0, "first line\nsecond\n\nfourth");
    t.set(r, 1, "\x1b[31mred text\x1b[0m plain");
    v.push(("multiline".into(), t));

    // withColor (false) removes every color and forces dash underlines.
    let mut t = Table::new();
    t.width(30);
    t.with_color(false);
    t.force_color();
    t.color_header(c("underline"));
    t.color_odd(c("on red"));
    t.extra_padding(1);
    t.extra_color_odd(c("on red"));
    t.intra_color_odd(c("on red"));
    t.add_with("X", true, true);
    t.add_with("Y", false, true);
    let r = t.add_row();
    t.set_colored(r, 0, "a", c("green"));
    t.set_int(r, 1, 1);
    let r = t.add_row();
    t.set(r, 0, "b");
    t.set_int_colored(r, 1, 22, c("blue"));
    v.push(("without_color".into(), t));

    // rc.fontunderline style: an underline-colored header, no dashes.
    let mut t = Table::new();
    t.width(30);
    t.color_header(c("underline"));
    t.left_margin(1);
    t.add_with("Project", true, true);
    t.add_with("Count", false, true);
    let r = t.add_row();
    t.set(r, 0, "home");
    t.set_int(r, 1, 3);
    v.push(("underline_color_header".into(), t));

    // No rows; a word longer than the width; no columns.
    let mut t = Table::new();
    t.width(20);
    t.add_with("Only", true, true);
    t.add_with("Header", false, true);
    v.push(("no_rows".into(), t));
    let mut u = Table::new();
    u.width(6);
    u.add_with("H", true, true);
    let r = u.add_row();
    u.set(r, 0, "abcdefghijklmnop qr");
    v.push(("long_word".into(), u));
    let mut w = Table::new();
    w.width(6);
    w.underline_headers();
    v.push(("no_columns".into(), w));

    // Headers wrapping to equal heights in a colored header.
    let mut t = Table::new();
    t.width(12);
    t.color_header(c("red"));
    t.intra_padding(2);
    t.add_with("aa bb", true, true);
    t.add_with("cc dd", false, true);
    let r = t.add_row();
    t.set(r, 0, "x");
    t.set(r, 1, "y");
    v.push(("wrapped_headers".into(), t));

    // Row colors take over the intra padding; blank fill lines keep the
    // blended cell color; extra padding colors by oddness.
    let mut t = Table::new();
    t.width(40);
    t.color_odd(c("on blue"));
    t.color_even(c("on yellow"));
    t.intra_color_odd(c("on red"));
    t.intra_color_even(c("on green"));
    t.extra_padding(1);
    t.extra_color_odd(c("on white"));
    t.extra_color_even(c("on black"));
    t.add_with("C1", true, true);
    t.add_with("C2", true, true);
    let r = t.add_row();
    t.set(r, 0, "short");
    t.set_colored(r, 1, "two words wrap", c("bold red"));
    let r = t.add_row();
    t.set(r, 0, "a\nb\nc");
    t.set(r, 1, "d");
    t.width(14);
    v.push(("row_colors_fill".into(), t));

    v
}

#[test]
fn render_matches_cpp() {
    let tables = scenarios();
    assert_eq!(tables.len(), GOLDEN.len());
    for ((name, mut table), &(golden_name, out, lines, rows)) in tables.into_iter().zip(GOLDEN) {
        assert_eq!(name, golden_name);
        assert_eq!(table.render(), out, "{name}: output");
        assert_eq!(table.lines(), lines, "{name}: lines ()");
        assert_eq!(table.rows(), rows, "{name}: rows ()");
    }
}

#[test]
fn single_empty_header_column_wraps_and_trims() {
    let mut t = Table::new();
    t.width(20);
    t.add("");
    let row = t.add_row();
    t.set(row, 0, "one two three four five six seven");
    assert_eq!(t.render(), "one two three four\nfive six seven\n");
    assert_eq!(t.lines(), 2);
    assert_eq!(t.rows(), 1);
}

#[test]
fn narrow_width_uses_minimal() {
    let mut t = Table::new();
    t.width(0);
    t.add("");
    let row = t.add_row();
    t.set(row, 0, "aa bbbb c");
    assert_eq!(t.render(), "aa\nbbbb\nc\n");
}

#[test]
fn row_oddness() {
    let mut t = Table::new();
    t.add("x");
    assert_eq!(t.add_row(), 0);
    assert_eq!(t.add_row_even(), 1);
    assert_eq!(t.add_row_odd(), 2);
    assert_eq!(t.add_row(), 3);
    assert_eq!(t.oddness, [true, false, true, false]);
}

/// Deviation cases of `render`: the C++ is undefined behaviour here.
#[test]
fn undefined_behaviour_fallbacks() {
    // Headers wrapping to different heights (only possible through embedded
    // newlines, as a header's full width is part of its column's minimum):
    // the C++ reads `headers[1][1]` and `headers[1][2]` of a one-line header;
    // here they are blank.
    let mut t = Table::new();
    t.width(80);
    t.add("a\nb\nc");
    t.add("d");
    let row = t.add_row();
    t.set(row, 0, "x");
    t.set(row, 1, "y");
    assert_eq!(t.render(), "a\nb\nc\nx   y\n");

    // overage == 0 with zero minimal widths: C++ indexes an empty `widths`.
    let mut t = Table::new();
    t.width(0);
    t.add("");
    let row = t.add_row();
    t.set(row, 0, "");
    assert_eq!(t.render(), "");
}

/// Output of upstream libshared (d899eba6) `Table::render` for each scenario
/// of [`scenarios`], generated by compiling the C++ `Table.cpp`, `Color.cpp`,
/// `format.cpp`, `shared.cpp` and `utf8.cpp` with a driver that builds the
/// same tables: (name, rendered output, `lines ()`, `rows ()`).
const GOLDEN: &[(&str, &str, i32, i32)] = &[
        ("t1", "    \x1b[33mHeader1    \x1b[0m \x1b[33mHeader2                                            \x1b[0m \x1b[33m     Header3\x1b[0m\n    \x1b[48;5;233mtop left   \x1b[0m\x1b[48;5;233m \x1b[0m\x1b[48;5;233mtop center                                         \x1b[0m\x1b[48;5;233m \x1b[0m\x1b[48;5;233m   top right\x1b[0m\n    \x1b[1;38;5;7;48;5;1mbottom left\x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232mbottom center, containing sufficient text that     \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232mbottom right\x1b[0m\n    \x1b[1;38;5;7;48;5;1m           \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232mwrapping will occur because it exceeds all         \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232m            \x1b[0m\n    \x1b[1;38;5;7;48;5;1m           \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232mreasonable values for default width.  Even in a    \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232m            \x1b[0m\n    \x1b[1;38;5;7;48;5;1m           \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232mvery wide terminal window.  Just look at the       \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232m            \x1b[0m\n    \x1b[1;38;5;7;48;5;1m           \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232mlengths we must go to, to get passing unit tests   \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232m            \x1b[0m\n    \x1b[1;38;5;7;48;5;1m           \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232mand not flaky tests.                               \x1b[0m\x1b[48;5;232m \x1b[0m\x1b[48;5;232m            \x1b[0m\n", 8, 2),
        ("t3", "Header\n\x1b[1;37;41mcell with color\x1b[0m\n", 2, 1),
        ("t4", "Header\n------------------\ncell without color\n", 2, 1),
        ("chessboard", "    \x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\n    \x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\n    \x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\n    \x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\n    \x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\n    \x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\n    \x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\n    \x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\x1b[104m  \x1b[0m\x1b[107m  \x1b[0m\n", 8, 8),
        ("underline_padding_colors", "    \x1b[1;34mName  \x1b[0m   \x1b[1;34mValue\x1b[0m\n    \x1b[1;34m------\x1b[0m   \x1b[1;34m-----\x1b[0m\n  \x1b[41m  \x1b[0mitem 0\x1b[46m   \x1b[0m    0\x1b[41m  \x1b[0m\n  \x1b[42m  \x1b[0mitem 1\x1b[45m   \x1b[0m   37\x1b[42m  \x1b[0m\n  \x1b[41m  \x1b[0mitem 2\x1b[46m   \x1b[0m   74\x1b[41m  \x1b[0m\n  \x1b[42m  \x1b[0mitem 3\x1b[45m   \x1b[0m  111\x1b[42m  \x1b[0m\n", 5, 4),
        ("alternate_blend", "\x1b[4;33mA   \x1b[0m \x1b[4;33mB\x1b[0m\ne1   \x1b[31mx\x1b[0m\n\x1b[1;48;5;236mo1  \x1b[0m\x1b[48;5;236m \x1b[0m\x1b[48;5;236my\x1b[0m\n\x1b[48;5;236mo2  \x1b[0m\x1b[48;5;236m \x1b[0m\x1b[38;5;12;48;5;236mz\x1b[0m\nrow3 \x1b[38;2;16;32;48mw\x1b[0m\n\x1b[48;5;236mrow4\x1b[0m\x1b[48;5;236m \x1b[0m\x1b[48;5;4m \x1b[0m\n", 6, 5),
        ("obfuscate", "Desc               N\n\x1b[38;5;1;48;5;234mxxxxxx xxxxx xxxx\x1b[0m\x1b[48;5;234m \x1b[0m\x1b[48;5;234mxx\x1b[0m\n\x1b[38;5;1;48;5;234mxxx xxxx xxxx    \x1b[0m\x1b[48;5;234m \x1b[0m\x1b[48;5;234m  \x1b[0m\nxxxxxxx xxx    xx\n", 4, 2),
        ("truncate_l0_r0", "Head\naaa bbb ccc\nddd\naaa bbb ccc\nddd\naaa bbb ccc\nddd\n", 7, 3),
        ("truncate_l0_r1", "Head\naaa bbb ccc\nddd\n", 3, 3),
        ("truncate_l0_r2", "Head\naaa bbb ccc\nddd\naaa bbb ccc\nddd\n", 5, 3),
        ("truncate_l1_r0", "Head\n", 1, 3),
        ("truncate_l1_r1", "Head\n", 1, 3),
        ("truncate_l1_r2", "Head\n", 1, 3),
        ("truncate_l2_r0", "Head\naaa bbb ccc\n", 2, 3),
        ("truncate_l2_r1", "Head\naaa bbb ccc\n", 2, 3),
        ("truncate_l2_r2", "Head\naaa bbb ccc\n", 2, 3),
        ("truncate_l3_r0", "Head\naaa bbb ccc\nddd\n", 3, 3),
        ("truncate_l3_r1", "Head\naaa bbb ccc\nddd\n", 3, 3),
        ("truncate_l3_r2", "Head\naaa bbb ccc\nddd\n", 3, 3),
        ("truncate_l5_r0", "Head\naaa bbb ccc\nddd\naaa bbb ccc\nddd\n", 5, 3),
        ("truncate_l5_r1", "Head\naaa bbb ccc\nddd\n", 3, 3),
        ("truncate_l5_r2", "Head\naaa bbb ccc\nddd\naaa bbb ccc\nddd\n", 5, 3),
        ("nowrap", "Left      Right W\na long unwrapped value    rv wrap\n                me\n                please\n", 4, 1),
        ("wide", "名前                  説明\n日本語のテキスト    これは\n                    とても\n                 長い 説明\n                      です\nété              combining\n", 6, 2),
        ("spread_w10", "One   Two   Three\nalpha x     delta\nbeta      epsilon\ngamma        zeta\n              eta\n", 5, 1),
        ("spread_w20", "One     Two    Three\nalpha   x      delta\nbeta         epsilon\ngamma       zeta eta\n", 4, 1),
        ("spread_w25", "One       Two       Three\nalpha     x         delta\nbeta              epsilon\ngamma            zeta eta\n", 4, 1),
        ("spread_w30", "One          Two         Three\nalpha beta   x   delta epsilon\ngamma                 zeta eta\n", 3, 1),
        ("spread_w33", "One           Two           Three\nalpha beta    x     delta epsilon\ngamma                    zeta eta\n", 3, 1),
        ("spread_w40", "One              Two               Three\nalpha beta gamma x    delta epsilon zeta\n                                     eta\n", 3, 1),
        ("spread_w100", "One              Two                  Three\nalpha beta gamma x   delta epsilon zeta eta\n", 2, 1),
        ("multiline", "Lines      Colored\n---------- --------------\nfirst line \x1b[31mred\nsecond     text\x1b[0m plain\n\nfourth\n", 5, 1),
        ("without_color", " X  Y\n - --\n a  1\n b 22\n", 3, 2),
        ("underline_color_header", " \x1b[4mProject\x1b[0m \x1b[4mCount\x1b[0m\n home        3\n", 2, 1),
        ("no_rows", "Only Header\n", 1, 0),
        ("long_word", "H\nabcdefghijklmnop\nqr\n", 3, 1),
        ("no_columns", "\n", 0, 0),
        ("wrapped_headers", "\x1b[31maa bb\x1b[0m  \x1b[31mcc dd\x1b[0m\nx          y\n", 2, 1),
        ("row_colors_fill", " C1    C2\n\x1b[47m \x1b[0m\x1b[44mshort\x1b[0m\x1b[44m \x1b[0m\x1b[1;31;44mtwo   \x1b[0m\x1b[47m \x1b[0m\n\x1b[47m \x1b[0m\x1b[44m     \x1b[0m\x1b[44m \x1b[0m\x1b[1;31;44mwords \x1b[0m\x1b[47m \x1b[0m\n\x1b[47m \x1b[0m\x1b[44m     \x1b[0m\x1b[44m \x1b[0m\x1b[1;31;44mwrap  \x1b[0m\x1b[47m \x1b[0m\n\x1b[40m \x1b[0m\x1b[43ma    \x1b[0m\x1b[43m \x1b[0m\x1b[43md     \x1b[0m\x1b[40m \x1b[0m\n\x1b[40m \x1b[0m\x1b[43mb    \x1b[0m\x1b[43m \x1b[0m\x1b[43m      \x1b[0m\x1b[40m \x1b[0m\n\x1b[40m \x1b[0m\x1b[43mc    \x1b[0m\x1b[43m \x1b[0m\x1b[43m      \x1b[0m\x1b[40m \x1b[0m\n", 7, 2),
];
