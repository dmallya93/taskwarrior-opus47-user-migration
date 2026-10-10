//! Rust translation of `src/columns/ColDescription.cpp`.
//!
//! Constructor metadata and the render settings it captures (`_hyphenate`,
//! `_dateformat`, `_indent`), and the `measure` / `render` overrides for
//! every style (`combined`, `desc`, `oneline`, `truncated`, `count`,
//! `truncated_count`); `validate` and `modify` are inherited from
//! `ColumnTypeString`.
//!
//! The unit tests port `test/col_description_test.cpp` (assertion-count
//! checked) plus per-style cases.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, render_string_left, strings, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::datetime::Datetime;
use crate::shared::format::format_value;
use crate::shared::shared::{longest_word, wrap_text};
use crate::shared::utf8::utf8_width;
use crate::task::Task;

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

/// C++ `text.substr (0, n)` with the `int` length `n` converted to
/// `size_t`: a negative `n` becomes `npos` (the whole string). The cut is in
/// bytes, as in C++.
///
/// Deviation: when the cut falls inside a multi-byte UTF-8 character, C++
/// emits the partial bytes; a Rust `String` cannot hold them, so the partial
/// character becomes U+FFFD (lossy conversion).
fn byte_prefix(text: &str, n: i64) -> String {
    if n < 0 {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let end = (n as usize).min(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// C++ `Datetime dt (strtoll (i.first.substr (11).c_str (), nullptr, 10));`
/// for an `annotation_<epoch>` attribute name.
fn annotation_date(name: &str) -> Datetime {
    Datetime::from_epoch(c_strtoll(name.get(11..).unwrap_or("")))
}

/// C++ `class ColumnDescription : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDescription {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
    /// C++ `_hyphenate` (`rc.hyphenate`).
    pub hyphenate: bool,
    /// C++ `_dateformat` (`rc.dateformat.annotation`, else `rc.dateformat`).
    pub dateformat: String,
    /// C++ `_indent` (`rc.indent.annotation`, at least 0).
    pub indent: i32,
}

impl ColumnDescription {
    /// C++ `ColumnDescription::ColumnDescription ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "description".to_string();
        parent.base.style = "combined".to_string();
        parent.base.label = "Description".to_string();
        parent.base.modifiable = true;
        parent.base.styles = strings(&["combined", "desc", "oneline", "truncated", "count", "truncated_count"]);

        let mut dateformat = config.get("dateformat.annotation");
        if dateformat.is_empty() {
            dateformat = config.get("dateformat");
        }

        let t = Datetime::new().to_string(&dateformat);
        let d = "Move your clothes down on to the lower peg";
        let a1 = "Immediately before your lunch";
        let a2 = "If you are playing in the match this afternoon";
        let a3 = "Before you write your letter home";
        let a4 = "If you're not getting your hair cut";
        // C++ `d.substr (0, 20)` (ASCII).
        let d20 = &d[..20];
        parent.base.examples = vec![
            format!("{d}\n  {t} {a1}\n  {t} {a2}\n  {t} {a3}\n  {t} {a4}"),
            d.to_string(),
            format!("{d} {t} {a1} {t} {a2} {t} {a3} {t} {a4}"),
            format!("{d20}..."),
            format!("{d} [4]"),
            format!("{d20}... [4]"),
        ];

        ColumnDescription {
            parent,
            hyphenate: config.get_boolean("hyphenate"),
            dateformat,
            indent: config.get_integer("indent.annotation").max(0),
        }
    }
}

impl Column for ColumnDescription {
    column_base!(parent.base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }

    /// C++ `void ColumnDescription::measure (Task& task, unsigned int&
    /// minimum, unsigned int& maximum)`: set the minimum and maximum widths
    /// for the value. The C++ `unsigned int` arithmetic (wrapping) is kept.
    fn measure(&self, task: &mut Task, _env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let description = task.get(self.name());
        let style = self.style();
        let mut minimum: u32 = 0;
        let mut maximum: u32 = 0;

        // The text
        // <indent> <date> <anno>
        // ...
        if style == "default" || style == "combined" {
            minimum = longest_word(&description) as u32;
            maximum = utf8_width(&description);

            if task.annotation_count != 0 {
                let min_anno = self.indent.wrapping_add(Datetime::length(&self.dateformat)) as u32;
                if min_anno > minimum {
                    minimum = min_anno;
                }

                for value in task.get_annotations().values() {
                    let len = min_anno.wrapping_add(1).wrapping_add(utf8_width(value));
                    if len > maximum {
                        maximum = len;
                    }
                }
            }
        }
        // Just the text
        else if style == "desc" {
            maximum = utf8_width(&description);
            minimum = longest_word(&description) as u32;
        }
        // The text <date> <anno> ...
        else if style == "oneline" {
            minimum = longest_word(&description) as u32;
            maximum = utf8_width(&description);

            if task.annotation_count != 0 {
                let min_anno = Datetime::length(&self.dateformat) as u32;
                for value in task.get_annotations().values() {
                    maximum = maximum.wrapping_add(min_anno.wrapping_add(1).wrapping_add(utf8_width(value)));
                }
            }
        }
        // The te...
        else if style == "truncated" {
            minimum = 4;
            maximum = utf8_width(&description);
        }
        // The text [2]
        else if style == "count" {
            // <description> + ' ' + '[' + <count> + ']'
            maximum = utf8_width(&description) + 1 + 1 + format_value(&task.annotation_count).len() as u32 + 1;
            minimum = longest_word(&description) as u32;
        }
        // The te... [2]
        else if style == "truncated_count" {
            minimum = 4;
            maximum = utf8_width(&description) + 1 + 1 + format_value(&task.annotation_count).len() as u32 + 1;
        }

        Ok((minimum as usize, maximum as usize))
    }

    /// C++ `void ColumnDescription::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)`.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        _env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let mut description = task.get(self.name());
        let style = self.style();
        let iwidth = width as i32;

        // This is a description
        // <date> <anno>
        // ...
        if style == "default" || style == "combined" {
            if iwidth <= 0 {
                return Ok(());
            }

            let annotations = task.get_annotations();
            // Keep separator newlines so trailing blank lines survive wrapping each part separately.
            if !annotations.is_empty() {
                description.push('\n');
            }

            let mut raw: Vec<String> = Vec::new();
            wrap_text(&mut raw, &description, iwidth, self.hyphenate);

            for i in &raw {
                render_string_left(lines, width, color, i);
            }

            // Reports can shrink columns below their measured minimum. Leave room for text.
            let indent = self.indent.min(iwidth - 1);
            let prefix = " ".repeat(indent.max(0) as usize);
            let mut remaining = annotations.len();
            for (name, value) in &annotations {
                let dt = annotation_date(name);
                let mut annotation = format!("{} {}", dt.to_string(&self.dateformat), value);
                remaining -= 1;
                if remaining != 0 {
                    annotation.push('\n');
                }

                raw.clear();
                wrap_text(&mut raw, &annotation, iwidth - indent, self.hyphenate);
                for line in &raw {
                    render_string_left(lines, width, color, &format!("{prefix}{line}"));
                }
            }
        }
        // This is a description
        else if style == "desc" {
            let mut raw: Vec<String> = Vec::new();
            wrap_text(&mut raw, &description, iwidth, self.hyphenate);

            for i in &raw {
                render_string_left(lines, width, color, i);
            }
        }
        // This is a description <date> <anno> ...
        else if style == "oneline" {
            if task.annotation_count != 0 {
                for (name, value) in &task.get_annotations() {
                    let dt = annotation_date(name);
                    description.push(' ');
                    description.push_str(&dt.to_string(&self.dateformat));
                    description.push(' ');
                    description.push_str(value);
                }
            }

            let mut raw: Vec<String> = Vec::new();
            wrap_text(&mut raw, &description, iwidth, self.hyphenate);

            for i in &raw {
                render_string_left(lines, width, color, i);
            }
        }
        // This is a des...
        else if style == "truncated" {
            let len = utf8_width(&description) as i32;
            if len > iwidth {
                let cut = byte_prefix(&description, iwidth as i64 - 3);
                render_string_left(lines, width, color, &format!("{cut}..."));
            } else {
                render_string_left(lines, width, color, &description);
            }
        }
        // This is a description [2]
        else if style == "count" {
            if task.annotation_count != 0 {
                description.push_str(&format!(" [{}]", format_value(&task.annotation_count)));
            }

            let mut raw: Vec<String> = Vec::new();
            wrap_text(&mut raw, &description, iwidth, self.hyphenate);

            for i in &raw {
                render_string_left(lines, width, color, i);
            }
        }
        // This is a des... [2]
        else if style == "truncated_count" {
            let mut len = utf8_width(&description) as i32;

            let mut annos_count = String::new();
            let mut len_annos: i32 = 0;
            if task.annotation_count != 0 {
                annos_count = format!(" [{}]", format_value(&task.annotation_count));
                len_annos = utf8_width(&annos_count) as i32;
                len += len_annos;
            }

            if len > iwidth {
                let cut = byte_prefix(&description, iwidth as i64 - len_annos as i64 - 3);
                render_string_left(lines, width, color, &format!("{cut}...{annos_count}"));
            } else {
                render_string_left(lines, width, color, &format!("{description}{annos_count}"));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Port of `test/col_description_test.cpp` (#3914: annotation wrapping
    //! at column widths, independently of report width allocation) and
    //! `measure` / `render` of every description style.
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};
    use crate::shared::localtime;
    use crate::shared::shared::{join, rtrim};
    use crate::test_util::{env_lock, EnvGuard};
    use std::collections::BTreeMap;

    /// 2025-09-08T12:00:00Z, the first annotation timestamp of the C++ test.
    const STAMP: i64 = 1_757_332_800;

    /// Holds [`env_lock`] with `TASKDATA`/`TASKRC` unset and `TZ=UTC`,
    /// restoring them (and calling `tzset`) before the lock is released.
    struct Utc {
        env: Option<EnvGuard>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl Utc {
        fn new() -> Self {
            let lock = env_lock();
            let env = EnvGuard::save(&["TZ", "TASKDATA", "TASKRC"]);
            std::env::remove_var("TASKDATA");
            std::env::remove_var("TASKRC");
            std::env::set_var("TZ", "UTC");
            localtime::tzset();
            Utc { env: Some(env), _lock: lock }
        }
    }

    impl Drop for Utc {
        fn drop(&mut self) {
            drop(self.env.take());
            localtime::tzset();
        }
    }

    /// The C++ `UnitTest` (test/test.cpp), recording instead of printing.
    struct UnitTest {
        counter: usize,
        failures: Vec<String>,
    }

    impl UnitTest {
        fn ok(&mut self, expression: bool, name: &str) {
            self.counter += 1;
            if !expression {
                self.failures.push(format!("not ok {} - {}", self.counter, name));
            }
        }

        fn diag(&mut self, text: &str) {
            self.failures.push(format!("# {text}"));
        }
    }

    /// C++ `struct AnnotationCase`.
    struct AnnotationCase {
        name: &'static str,
        width: i32,
        indent: i32,
        description: &'static str,
        annotations: &'static [&'static str],
        expected: &'static [&'static str],
        dateformat: &'static str,
    }

    #[allow(clippy::too_many_arguments)]
    const fn case(
        name: &'static str,
        width: i32,
        indent: i32,
        description: &'static str,
        annotations: &'static [&'static str],
        expected: &'static [&'static str],
        dateformat: &'static str,
    ) -> AnnotationCase {
        AnnotationCase { name, width, indent, description, annotations, expected, dateformat }
    }

    /// `test/col_description_test.cpp` `TEST_NAME`.
    #[test]
    fn col_description_test() {
        let _utc = Utc::new();

        let cases = [
            case("default indent", 12, 2, "task", &["aa bb cc dd"], &["task", "  08 aa bb", "  cc dd"], "D"),
            case("zero indent", 12, 0, "task", &["aa bb cc dd"], &["task", "08 aa bb cc", "dd"], "D"),
            case("custom indent", 12, 4, "task", &["aa bb cc dd"], &["task", "    08 aa bb", "    cc dd"], "D"),
            case("exact fit", 16, 2, "task", &["aa bb cc dd"], &["task", "  08 aa bb cc dd"], "D"),
            case("one column short", 15, 2, "task", &["aa bb cc dd"], &["task", "  08 aa bb cc", "  dd"], "D"),
            case("indent equals width", 2, 2, "t", &["a"], &["t", " 0", " 8", " a"], "D"),
            case("indent exceeds width", 2, 100, "t", &["a"], &["t", " 0", " 8", " a"], "D"),
            case("one-column width", 1, 2, "t", &["a"], &["t", "0", "8", "a"], "D"),
            case("nonpositive width", 0, 2, "t", &["a"], &[], "D"),
            case("negative indent", 12, -2, "task", &["aa bb cc dd"], &["task", "08 aa bb cc", "dd"], "D"),
            case(
                "description wraps",
                12,
                2,
                "one two three four",
                &["aa bb cc dd"],
                &["one two", "three four", "  08 aa bb", "  cc dd"],
                "D",
            ),
            case("unannotated description wraps", 12, 2, "one two three four", &[], &["one two", "three four"], "D"),
            case(
                "explicit and boundary newlines",
                12,
                2,
                "task\n",
                &["aa\n\nbb\n", "cc\n"],
                &["task", "", "  08 aa", "", "  bb", "", "  08 cc"],
                "D",
            ),
            case("empty description", 12, 2, "", &["aa"], &["", "  08 aa"], "D"),
            case("Unicode display widths", 12, 2, "task", &["åäö 界界 éé"], &["task", "  08 åäö", "  界界 éé"], "D"),
            case("wide glyph in one content column", 3, 2, "t", &["界"], &["t", "  0", "  8", "  ."], "D"),
            case(
                "annotation date format",
                18,
                2,
                "task",
                &["aa bb cc dd"],
                &["task", "  2025-09-08 aa bb", "  cc dd"],
                "Y-M-D",
            ),
            case(
                "fallback date format",
                18,
                2,
                "task",
                &["aa bb cc dd"],
                &["task", "  2025-09-08 aa bb", "  cc dd"],
                "",
            ),
        ];

        let mut test = UnitTest { counter: 0, failures: Vec::new() };
        let mut config = Configuration::new();
        config
            .parse(crate::context::CONFIGURATION_DEFAULTS, 0, &[crate::context::task_rcdir()], "")
            .unwrap();
        config.set("dateformat", "Y-M-D");
        config.set("hyphenate", 0); // C++ `set (key, false)` is the int overload.
        let mut env = FakeRenderEnv::with_config(&[]);

        for item in &cases {
            config.set("indent.annotation", item.indent.to_string().as_str());
            config.set("dateformat.annotation", item.dateformat);
            let column = factory("description", "rrr", &config).unwrap();
            let mut task = Task::new();
            task.set("description", item.description);
            let mut annotations: BTreeMap<String, String> = BTreeMap::new();
            let mut timestamp = STAMP;
            for annotation in item.annotations {
                annotations.insert(format!("annotation_{timestamp}"), annotation.to_string());
                timestamp += 1;
            }
            task.set_annotations(&annotations);

            let color = Color::new();
            let mut lines: Vec<String> = Vec::new();
            // C++ passes `int width`; the nonpositive case is width 0 here.
            column.render(&mut lines, &mut task, item.width.max(0) as usize, &color, &mut env).unwrap();
            let mut fits = true;
            for line in lines.iter_mut() {
                fits = fits && utf8_width(line) as i32 <= item.width;
                *line = rtrim(line);
            }
            test.ok(fits, &format!("{}: fits column width", item.name));
            let expected: Vec<String> = item.expected.iter().map(|s| s.to_string()).collect();
            test.ok(lines == expected, &format!("{}: rendered lines", item.name));
            if lines != expected {
                test.diag(&format!("Expected:\n{}", join("\n", &expected)));
                test.diag(&format!("Actual:\n{}", join("\n", &lines)));
            }
        }

        assert!(test.failures.is_empty(), "{}", test.failures.join("\n"));
        // C++ `UnitTest test (cases.size () * 2)`.
        assert_eq!(test.counter, cases.len() * 2);
        assert_eq!(test.counter, 36);
    }

    /// A description column in `style`, with `dateformat.annotation=Y-M-D`,
    /// `indent.annotation=2` and hyphenation off.
    fn column(style: &str) -> Box<dyn Column> {
        let mut config = Configuration::new();
        config.set("dateformat.annotation", "Y-M-D");
        config.set("indent.annotation", "2");
        config.set("hyphenate", 0); // C++ `set (key, false)` is the int overload.
        factory(&format!("description.{style}"), "list", &config).unwrap()
    }

    /// A task with `description` and one annotation per entry of
    /// `annotations`, one second apart from [`STAMP`].
    fn task(description: &str, annotations: &[&str]) -> Task {
        let mut task = Task::new();
        task.set("description", description);
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        for (i, a) in annotations.iter().enumerate() {
            map.insert(format!("annotation_{}", STAMP + i as i64), a.to_string());
        }
        task.set_annotations(&map);
        task
    }

    fn measure(style: &str, task: &mut Task) -> (usize, usize) {
        column(style).measure(task, &mut FakeRenderEnv::with_config(&[])).unwrap()
    }

    fn render(style: &str, task: &mut Task, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        column(style).render(&mut lines, task, width, &Color::new(), &mut FakeRenderEnv::with_config(&[])).unwrap();
        lines
    }

    #[test]
    fn measure_every_style() {
        let _utc = Utc::new();
        // Longest word 5, width 13; annotation width 5; Y-M-D is 10 wide.
        let mut t = task("one two three", &["aa bb"]);
        // min max (5, 2 + 10), max max (13, 12 + 1 + 5).
        assert_eq!(measure("combined", &mut t), (12, 18));
        assert_eq!(measure("default", &mut t), (12, 18));
        assert_eq!(measure("desc", &mut t), (5, 13));
        // 13 + 10 + 1 + 5.
        assert_eq!(measure("oneline", &mut t), (5, 29));
        assert_eq!(measure("truncated", &mut t), (4, 13));
        // 13 + " [1]".
        assert_eq!(measure("count", &mut t), (5, 17));
        assert_eq!(measure("truncated_count", &mut t), (4, 17));

        // Without annotations the annotation terms vanish ("[0]" still counts).
        let mut t = task("one two three", &[]);
        assert_eq!(measure("combined", &mut t), (5, 13));
        assert_eq!(measure("oneline", &mut t), (5, 13));
        assert_eq!(measure("count", &mut t), (5, 17));
        assert_eq!(measure("truncated_count", &mut t), (4, 17));

        // Display width, not bytes.
        let mut t = task("界界 é", &[]);
        assert_eq!(measure("desc", &mut t), (4, 6));
    }

    #[test]
    fn render_combined() {
        let _utc = Utc::new();
        let mut t = task("one two", &["aa", "bb"]);
        // "one two\n" yields one line; each annotation wraps at 14 - 2.
        assert_eq!(render("combined", &mut t, 14), vec![
            "one two       ",
            "  2025-09-08  ",
            "  aa          ",
            "  2025-09-08  ",
            "  bb          ",
        ]);
        assert_eq!(render("combined", &mut t, 16), vec![
            "one two         ",
            "  2025-09-08 aa ",
            "  2025-09-08 bb ",
        ]);
        assert!(render("combined", &mut t, 0).is_empty());
    }

    #[test]
    fn render_desc_ignores_annotations() {
        let _utc = Utc::new();
        let mut t = task("one two three four", &["aa"]);
        assert_eq!(render("desc", &mut t, 12), vec!["one two     ", "three four  "]);
    }

    #[test]
    fn render_oneline() {
        let _utc = Utc::new();
        let mut t = task("task", &["aa", "bb"]);
        assert_eq!(render("oneline", &mut t, 32), vec!["task 2025-09-08 aa 2025-09-08 bb"]);
        assert_eq!(render("oneline", &mut t, 20), vec!["task 2025-09-08 aa  ", "2025-09-08 bb       "]);
    }

    #[test]
    fn render_truncated() {
        let _utc = Utc::new();
        let mut t = task("This is a description", &["aa"]);
        assert_eq!(render("truncated", &mut t, 10), vec!["This is..."]);
        assert_eq!(render("truncated", &mut t, 21), vec!["This is a description"]);
        assert_eq!(render("truncated", &mut t, 24), vec!["This is a description   "]);
        // C++ `substr (0, width - 3)` with a negative length keeps everything.
        assert_eq!(render("truncated", &mut t, 2), vec!["This is a description..."]);
        // Width is measured in columns but the cut is in bytes (C++ quirk):
        // two bytes of "åäö..." is one character.
        let mut t = task("åäöåäöåäö", &[]);
        assert_eq!(render("truncated", &mut t, 5), vec!["å... "]);
    }

    #[test]
    fn render_count() {
        let _utc = Utc::new();
        let mut t = task("task", &["aa", "bb"]);
        assert_eq!(render("count", &mut t, 10), vec!["task [2]  "]);
        let mut t = task("one two three", &["aa", "bb"]);
        assert_eq!(render("count", &mut t, 10), vec!["one two   ", "three [2] "]);
        let mut t = task("task", &[]);
        assert_eq!(render("count", &mut t, 6), vec!["task  "]);
    }

    #[test]
    fn render_truncated_count() {
        let _utc = Utc::new();
        let mut t = task("This is a description", &["aa", "bb"]);
        // 14 - 4 - 3 = 7 bytes of description.
        assert_eq!(render("truncated_count", &mut t, 14), vec!["This is... [2]"]);
        assert_eq!(render("truncated_count", &mut t, 26), vec!["This is a description [2] "]);
        let mut t = task("This is a description", &[]);
        assert_eq!(render("truncated_count", &mut t, 10), vec!["This is..."]);
        assert_eq!(render("truncated_count", &mut t, 22), vec!["This is a description "]);
    }

    #[test]
    fn render_unknown_annotation_date_is_epoch() {
        let _utc = Utc::new();
        // strtoll of a non-numeric suffix is 0.
        let mut t = Task::new();
        t.set("description", "d");
        let mut map = BTreeMap::new();
        map.insert("annotation_x".to_string(), "aa".to_string());
        t.set_annotations(&map);
        assert_eq!(render("oneline", &mut t, 15), vec!["d 1970-01-01 aa"]);
    }
}
