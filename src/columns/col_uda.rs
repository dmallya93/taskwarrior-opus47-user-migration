//! Rust translation of `src/columns/ColUDA.cpp`.
//!
//! The user-defined attribute columns `ColumnUDAString`,
//! `ColumnUDANumeric`, `ColumnUDADate`, `ColumnUDADuration` and
//! `ColumnUDAUUID`: constructor metadata and `validate` (the allowed
//! `uda.<name>.values`, or a UUID). `modify` is inherited from the type
//! column, which calls back into these `validate` overrides where C++ does
//! (`ColumnTypeString::modify`). `measure` / `render` are the C++
//! overrides: a `default` style per type and an `indicator` style showing
//! `rc.uda.<name>.indicator` (default `U`); `ColumnUDAUUID` inherits them
//! from `ColumnUUID`. [`crate::columns::column::uda`] sets `_name`,
//! `_label` and `_values`.

use crate::columns::col_type_date::ColumnTypeDate;
use crate::columns::col_type_duration::ColumnTypeDuration;
use crate::columns::col_type_numeric::ColumnTypeNumeric;
use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::col_uuid::ColumnUUID;
use crate::columns::column::{
    column_base, render_string_left, render_string_right, strings, Column, ModifyEnv, RenderEnv,
};
use crate::error::Result;
use crate::lexer::{Lexer, Type};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::datetime::Datetime;
use crate::shared::duration::Duration;
use crate::shared::shared::{longest_line, longest_word, wrap_text};
use crate::shared::utf8::utf8_width;
use crate::task::Task;

/// The `validate` shared by the string, numeric, date and duration UDA
/// columns: no restrictions without values, otherwise an exact match.
fn validate_values(values: &[String], value: &str) -> bool {
    // No restrictions.
    if values.is_empty() {
        return true;
    }

    // Look for exact match value.
    // Fail if not found.
    values.iter().any(|i| i == value)
}

/// The `indicator` style text every UDA `measure` / `render` reads:
/// `rc.uda.<name>.indicator`, or `U` when that is empty.
fn indicator(config: &Configuration, name: &str) -> String {
    let indicator = config.get(&format!("uda.{name}.indicator"));
    if indicator.is_empty() {
        "U".to_string()
    } else {
        indicator
    }
}

/// C `strtoll (value.c_str (), nullptr, 10)`, as `ColumnUDADate` converts
/// the stored epoch.
fn c_strtoll(text: &str) -> i64 {
    // An interior NUL ends the C string; parse the prefix.
    let prefix = text.split('\0').next().unwrap_or("");
    let Ok(text) = std::ffi::CString::new(prefix) else {
        return 0;
    };
    // SAFETY: `text` is a valid NUL-terminated C string for the duration of
    // the call, and a null `endptr` is allowed.
    unsafe { libc::strtoll(text.as_ptr(), std::ptr::null_mut(), 10) }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDAString : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDAString {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
    /// C++ `_values`.
    pub values: Vec<String>,
    /// C++ `_hyphenate` (always `true`).
    pub hyphenate: bool,
}

impl Default for ColumnUDAString {
    fn default() -> Self {
        ColumnUDAString::new()
    }
}

impl ColumnUDAString {
    /// C++ `ColumnUDAString::ColumnUDAString ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "<uda>".to_string(); // Gets overwritten at runtime.
        parent.base.style = "default".to_string();
        parent.base.label = String::new();
        parent.base.modifiable = true;
        parent.base.uda = true;
        parent.base.styles = strings(&["default", "indicator"]);
        ColumnUDAString { parent, values: Vec::new(), hyphenate: true }
    }
}

impl Column for ColumnUDAString {
    column_base!(parent.base);

    /// C++ `bool ColumnUDAString::validate (const std::string& value) const`.
    fn validate(&self, value: &str) -> bool {
        validate_values(&self.values, value)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }

    /// C++ `void ColumnUDAString::measure (Task& task, unsigned int&
    /// minimum, unsigned int& maximum)`: `default` is the longest line /
    /// longest word of the colour-stripped value, `indicator` the width of
    /// the indicator; `(0, 0)` without the attribute.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0usize;
        let mut maximum = 0usize;
        let name = self.name();
        if task.has(name) {
            let style = self.style();
            if style == "default" {
                let value = task.get(name);
                if !value.is_empty() {
                    let stripped = Color::strip(&value);
                    maximum = longest_line(&stripped) as usize;
                    minimum = longest_word(&stripped) as usize;
                }
            } else if style == "indicator" {
                let indicator = indicator(env.config(), name);
                minimum = utf8_width(&indicator) as usize;
                maximum = minimum;
            }
        }
        Ok((minimum, maximum))
    }

    /// C++ `void ColumnUDAString::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)`: `default` wraps the value to
    /// `width` (hyphenating) and left-justifies each line; `indicator`
    /// right-justifies the indicator.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let name = self.name();
        if task.has(name) {
            let style = self.style();
            if style == "default" {
                let value = task.get(name);
                let mut raw = Vec::new();
                wrap_text(&mut raw, &value, width as i32, self.hyphenate);

                for i in &raw {
                    render_string_left(lines, width, color, i);
                }
            } else if style == "indicator" {
                let indicator = indicator(env.config(), name);
                render_string_right(lines, width, color, &indicator);
            }
        }
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDANumeric : public ColumnTypeNumeric`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDANumeric {
    /// The inherited `ColumnTypeNumeric`.
    pub parent: ColumnTypeNumeric,
    /// C++ `_values`.
    pub values: Vec<String>,
}

impl Default for ColumnUDANumeric {
    fn default() -> Self {
        ColumnUDANumeric::new()
    }
}

impl ColumnUDANumeric {
    /// C++ `ColumnUDANumeric::ColumnUDANumeric ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeNumeric::new();
        parent.base.name = "<uda>".to_string();
        parent.base.type_ = "numeric".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = String::new();
        parent.base.uda = true;
        parent.base.styles = strings(&["default", "indicator"]);
        ColumnUDANumeric { parent, values: Vec::new() }
    }
}

impl Column for ColumnUDANumeric {
    column_base!(parent.base);

    /// C++ `bool ColumnUDANumeric::validate (const std::string& value) const`.
    fn validate(&self, value: &str) -> bool {
        validate_values(&self.values, value)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeNumeric::modify_numeric(self, task, value, env)
    }

    /// C++ `void ColumnUDANumeric::measure (Task& task, unsigned int&
    /// minimum, unsigned int& maximum)`: `default` is the byte length of the
    /// stored value, `indicator` the width of the indicator.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0usize;
        let name = self.name();
        if task.has(name) {
            let style = self.style();
            if style == "default" {
                let value = task.get(name);
                if !value.is_empty() {
                    minimum = value.len();
                }
            } else if style == "indicator" {
                minimum = utf8_width(&indicator(env.config(), name)) as usize;
            }
        }
        Ok((minimum, minimum))
    }

    /// C++ `void ColumnUDANumeric::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)`: the stored value or the
    /// indicator, right-justified.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let name = self.name();
        if task.has(name) {
            let style = self.style();
            if style == "default" {
                let value = task.get(name);
                render_string_right(lines, width, color, &value);
            } else if style == "indicator" {
                render_string_right(lines, width, color, &indicator(env.config(), name));
            }
        }
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDADate : public ColumnTypeDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDADate {
    /// The inherited `ColumnTypeDate`.
    pub parent: ColumnTypeDate,
    /// C++ `_values`.
    pub values: Vec<String>,
}

impl ColumnUDADate {
    /// C++ `ColumnUDADate::ColumnUDADate ()` (the `ColumnTypeDate` base
    /// constructor reads `rc.dateformat` for its examples).
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeDate::new(config);
        parent.base.name = "<uda>".to_string();
        parent.base.type_ = "date".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = String::new();
        parent.base.uda = true;
        parent.base.styles = strings(&["default", "indicator"]);
        ColumnUDADate { parent, values: Vec::new() }
    }
}

impl Column for ColumnUDADate {
    column_base!(parent.base);

    /// C++ `bool ColumnUDADate::validate (const std::string& value) const`.
    fn validate(&self, value: &str) -> bool {
        validate_values(&self.values, value)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDate::modify_date(self, task, value, env)
    }

    /// C++ `void ColumnUDADate::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`: `default` is `Datetime::length` of the
    /// report date format (a negative `int` wraps through `unsigned int` as
    /// in C++), `indicator` the width of the indicator.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0usize;
        let name = self.name();
        if task.has(name) {
            let style = self.style();
            if style == "default" {
                let value = task.get(name);
                if !value.is_empty() {
                    // Determine the output date format, which uses a hierarchy of definitions.
                    //   rc.report.<report>.dateformat
                    //   rc.dateformat.report
                    //   rc.dateformat
                    let format = ColumnTypeDate::report_dateformat(env.config(), &self.base().report);
                    minimum = Datetime::length(&format) as u32 as usize;
                }
            } else if style == "indicator" {
                minimum = utf8_width(&indicator(env.config(), name)) as usize;
            }
        }
        Ok((minimum, minimum))
    }

    /// C++ `void ColumnUDADate::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)`: `default` is the date
    /// (`strtoll` of the stored value) in the report date format,
    /// left-justified; `indicator` the indicator, right-justified.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let name = self.name();
        if task.has(name) {
            let style = self.style();
            if style == "default" {
                let value = task.get(name);

                // Determine the output date format, which uses a hierarchy of definitions.
                //   rc.report.<report>.dateformat
                //   rc.dateformat.report
                //   rc.dateformat.
                let format = ColumnTypeDate::report_dateformat(env.config(), &self.base().report);

                render_string_left(lines, width, color, &Datetime::from_epoch(c_strtoll(&value)).to_string(&format));
            } else if style == "indicator" {
                render_string_right(lines, width, color, &indicator(env.config(), name));
            }
        }
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDADuration : public ColumnTypeDuration`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDADuration {
    /// The inherited `ColumnTypeDuration`.
    pub parent: ColumnTypeDuration,
    /// C++ `_values`.
    pub values: Vec<String>,
}

impl Default for ColumnUDADuration {
    fn default() -> Self {
        ColumnUDADuration::new()
    }
}

impl ColumnUDADuration {
    /// C++ `ColumnUDADuration::ColumnUDADuration ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeDuration::new();
        parent.base.name = "<uda>".to_string();
        parent.base.type_ = "duration".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = String::new();
        parent.base.uda = true;
        parent.base.styles = strings(&["default", "indicator"]);
        ColumnUDADuration { parent, values: Vec::new() }
    }
}

impl Column for ColumnUDADuration {
    column_base!(parent.base);

    /// C++ `bool ColumnUDADuration::validate (const std::string& value) const`.
    fn validate(&self, value: &str) -> bool {
        validate_values(&self.values, value)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDuration::modify_duration(self, task, value, env)
    }

    /// C++ `void ColumnUDADuration::measure (Task& task, unsigned int&
    /// minimum, unsigned int& maximum)`: `default` is the length of the
    /// value as `Duration::formatISO`, `indicator` the width of the
    /// indicator.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0usize;
        let name = self.name();
        if task.has(name) {
            let style = self.style();
            if style == "default" {
                let value = task.get(name);
                if !value.is_empty() {
                    minimum = Duration::from_string(&value).format_iso().len();
                }
            } else if style == "indicator" {
                // C++ re-checks `task.has (_name)` here (always true).
                minimum = utf8_width(&indicator(env.config(), name)) as usize;
            }
        }
        Ok((minimum, minimum))
    }

    /// C++ `void ColumnUDADuration::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)`: the value as
    /// `Duration::formatISO`, or the indicator, right-justified.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let name = self.name();
        if task.has(name) {
            let style = self.style();
            if style == "default" {
                let value = task.get(name);
                render_string_right(lines, width, color, &Duration::from_string(&value).format_iso());
            } else if style == "indicator" {
                render_string_right(lines, width, color, &indicator(env.config(), name));
            }
        }
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDAUUID : public ColumnUUID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDAUUID {
    /// The inherited `ColumnUUID`.
    pub parent: ColumnUUID,
}

impl Default for ColumnUDAUUID {
    fn default() -> Self {
        ColumnUDAUUID::new()
    }
}

impl ColumnUDAUUID {
    /// C++ `ColumnUDAUUID::ColumnUDAUUID ()`.
    pub fn new() -> Self {
        let mut parent = ColumnUUID::new();
        let base = &mut parent.parent.base;
        base.name = "<uda>".to_string();
        base.type_ = "uuid".to_string();
        base.style = "long".to_string();
        base.label = String::new();
        base.modifiable = true;
        base.uda = true;
        base.styles = strings(&["long", "short"]);
        base.examples = strings(&["f30cb9c3-3fc0-483f-bfb2-3bf134f00694", "f30cb9c3"]);
        ColumnUDAUUID { parent }
    }
}

impl Column for ColumnUDAUUID {
    column_base!(parent.parent.base);

    /// C++ `bool ColumnUDAUUID::validate (const std::string& input) const`:
    /// a complete UUID (`Lexer::isUUID` with an end boundary).
    fn validate(&self, input: &str) -> bool {
        let mut lex = Lexer::new(input);
        let mut token = String::new();
        let mut type_ = Type::word;
        lex.is_uuid(&mut token, &mut type_, true)
    }

    /// Inherited from `ColumnTypeString::modify` (through `ColumnUUID`),
    /// which uses this column's `validate`.
    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }

    /// Inherited `ColumnUUID::measure`; `self.parent` shares this column's
    /// base, so the UDA's name and style are used.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        self.parent.measure(task, env)
    }

    /// Inherited `ColumnUUID::render` (the UDA's value, `long` or the first
    /// 8 characters for `short`).
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        self.parent.render(lines, task, width, color, env)
    }
}

#[cfg(test)]
mod tests {
    //! `measure` / `render` of every UDA type in its `default` and
    //! `indicator` styles, built through the factory (as `report.X.columns`
    //! does) and rendered with a [`FakeRenderEnv`].
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};
    use crate::shared::localtime;
    use crate::test_util::{env_lock, EnvGuard};

    /// 2023-11-14T22:13:20Z.
    const FIXED: &str = "1700000000";

    const UUID: &str = "f30cb9c3-3fc0-483f-bfb2-3bf134f00694";

    /// Holds [`env_lock`] with `TZ=UTC`, restoring `TZ` (and calling
    /// `tzset`) before the lock is released.
    struct Utc {
        env: Option<EnvGuard>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl Utc {
        fn new() -> Self {
            let lock = env_lock();
            let env = EnvGuard::save(&["TZ"]);
            std::env::set_var("TZ", "UTC");
            localtime::tzset();
            Utc { env: Some(env), _lock: lock }
        }
    }

    impl Drop for Utc {
        fn drop(&mut self) {
            // Restore TZ and re-run tzset while the lock is still held.
            drop(self.env.take());
            localtime::tzset();
        }
    }

    fn env(extra: &[(&str, &str)]) -> FakeRenderEnv {
        let mut pairs = vec![
            ("uda.note.type", "string"),
            ("uda.est.type", "numeric"),
            ("uda.dd.type", "date"),
            ("uda.len.type", "duration"),
            ("uda.ref.type", "uuid"),
            ("dateformat", "Y-M-D"),
            ("dateformat.report", ""),
        ];
        pairs.extend_from_slice(extra);
        FakeRenderEnv::with_config(&pairs)
    }

    fn task_with(name: &str, value: &str) -> Task {
        let mut task = Task::new();
        task.set(name, value);
        task
    }

    /// `(measure, render lines)` of `column` (`<name>[.<style>]`) in report
    /// `list` at `width`.
    fn run(column: &str, task: &mut Task, width: usize, env: &mut FakeRenderEnv) -> ((usize, usize), Vec<String>) {
        let c = factory(column, "list", &env.config.clone()).unwrap();
        let m = c.measure(task, env).unwrap();
        let mut lines = Vec::new();
        c.render(&mut lines, task, width, &Color::new(), env).unwrap();
        (m, lines)
    }

    fn lines(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn string_default_measures_words_and_wraps() {
        let mut env = env(&[]);
        let mut task = task_with("note", "hello big world");
        assert_eq!(run("note", &mut task, 15, &mut env), ((5, 15), lines(&["hello big world"])));
        assert_eq!(run("note.default", &mut task, 9, &mut env), ((5, 15), lines(&["hello big", "world    "])));

        // Multi-line values: the longest line is the maximum; `longestWord`
        // splits on spaces only, so "ab\nlonger" (newline width 0) is 8.
        let mut task = task_with("note", "ab\nlonger line");
        assert_eq!(run("note", &mut task, 11, &mut env).0, (8, 11));

        // Colour codes are stripped for measuring.
        let mut task = task_with("note", "\x1b[31mred\x1b[0m text");
        assert_eq!(run("note", &mut task, 8, &mut env).0, (4, 8));

        // Words wider than the column are hyphenated (`_hyphenate`).
        let mut task = task_with("note", "abcdefghij");
        assert_eq!(run("note", &mut task, 4, &mut env), ((10, 10), lines(&["abc-", "def-", "ghij"])));
    }

    #[test]
    fn indicator_style_for_every_type() {
        let mut task = Task::new();
        for (name, value) in [("note", "x"), ("est", "1"), ("dd", FIXED), ("len", "PT1H")] {
            task.set(name, value);
        }

        let mut plain = env(&[]);
        let mut custom = env(&[
            ("uda.note.indicator", "N!"),
            ("uda.est.indicator", "\u{2605}"),
            ("uda.dd.indicator", "DDD"),
            ("uda.len.indicator", "\u{65e5}"),
        ]);
        for name in ["note", "est", "dd", "len"] {
            let column = format!("{name}.indicator");
            assert_eq!(run(&column, &mut task, 3, &mut plain), ((1, 1), lines(&["  U"])), "{column}");
        }
        assert_eq!(run("note.indicator", &mut task, 3, &mut custom), ((2, 2), lines(&[" N!"])));
        assert_eq!(run("est.indicator", &mut task, 2, &mut custom), ((1, 1), lines(&[" \u{2605}"])));
        assert_eq!(run("dd.indicator", &mut task, 3, &mut custom), ((3, 3), lines(&["DDD"])));
        // Display width, not bytes.
        assert_eq!(run("len.indicator", &mut task, 3, &mut custom), ((2, 2), lines(&[" \u{65e5}"])));
    }

    #[test]
    fn absent_attribute_renders_nothing() {
        let _utc = Utc::new();
        let mut env = env(&[]);
        let mut task = Task::new();
        for column in [
            "note",
            "note.indicator",
            "est",
            "est.indicator",
            "dd",
            "dd.indicator",
            "len",
            "len.indicator",
        ] {
            assert_eq!(run(column, &mut task, 5, &mut env), ((0, 0), Vec::new()), "{column}");
        }
    }

    #[test]
    fn numeric_default_is_right_justified_raw_value() {
        let mut env = env(&[]);
        let mut task = task_with("est", "12.50");
        assert_eq!(run("est", &mut task, 7, &mut env), ((5, 5), lines(&["  12.50"])));
        let mut task = task_with("est", "-3");
        assert_eq!(run("est.default", &mut task, 2, &mut env), ((2, 2), lines(&["-3"])));
    }

    #[test]
    fn date_default_uses_the_report_dateformat_hierarchy() {
        let _utc = Utc::new();
        let mut task = task_with("dd", FIXED);

        let mut env1 = env(&[]);
        assert_eq!(run("dd", &mut task, 12, &mut env1), ((10, 10), lines(&["2023-11-14  "])));

        let mut env2 = env(&[("dateformat.report", "D.M.Y H:N")]);
        assert_eq!(run("dd.default", &mut task, 16, &mut env2), ((16, 16), lines(&["14.11.2023 22:13"])));

        let mut env3 = env(&[("dateformat.report", "D.M.Y H:N"), ("report.list.dateformat", "M/D")]);
        assert_eq!(run("dd", &mut task, 5, &mut env3), ((5, 5), lines(&["11/14"])));
    }

    #[test]
    fn duration_default_is_iso() {
        let mut env = env(&[]);
        let mut task = task_with("len", "PT2H");
        assert_eq!(run("len", &mut task, 6, &mut env), ((4, 4), lines(&["  PT2H"])));
        let mut task = task_with("len", "3d");
        assert_eq!(run("len", &mut task, 3, &mut env), ((3, 3), lines(&["P3D"])));
        let mut task = task_with("len", "P1DT1H30M");
        assert_eq!(run("len", &mut task, 9, &mut env), ((9, 9), lines(&["P1DT1H30M"])));
    }

    #[test]
    fn uuid_inherits_column_uuid() {
        let mut env = env(&[]);
        let mut task = task_with("ref", UUID);
        assert_eq!(run("ref", &mut task, 36, &mut env), ((36, 36), lines(&[UUID])));
        assert_eq!(run("ref.long", &mut task, 36, &mut env), ((36, 36), lines(&[UUID])));
        assert_eq!(run("ref.short", &mut task, 9, &mut env), ((8, 8), lines(&["f30cb9c3 "])));
    }
}
