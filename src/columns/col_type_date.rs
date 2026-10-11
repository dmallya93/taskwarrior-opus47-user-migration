//! Rust translation of `src/columns/ColTypeDate.cpp`.
//!
//! `ColumnTypeDate`: the base of every date column (constructor metadata,
//! `measure`, `render`, `validate` and `modify`). Derived columns delegate
//! to the associated helpers ([`ColumnTypeDate::measure_date`],
//! [`ColumnTypeDate::render_date`], ...) passing themselves as `this`.

use crate::columns::column::{
    column_base, render_string_left, render_string_right, strings, Column, ColumnBase, ModifyEnv,
    RenderEnv, MODIFICATION_LABEL,
};
use crate::error::{Result, TwError};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::datetime::{Datetime, EPOCH_MAX_VALUE, EPOCH_MIN_VALUE};
use crate::shared::duration::Duration;
use crate::shared::format::{format, format_double, format_value};
use crate::task::Task;
use crate::variant::{self, Variant};

/// C++ `class ColumnTypeDate : public Column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTypeDate {
    /// The inherited `Column` members.
    pub base: ColumnBase,
}

impl ColumnTypeDate {
    /// C++ `ColumnTypeDate::ColumnTypeDate ()`. The examples are built from
    /// "now - 125 s" and `rc.dateformat`.
    pub fn new(config: &Configuration) -> Self {
        let mut base = ColumnBase::new();
        base.name = String::new();
        base.type_ = "date".to_string();
        base.style = "formatted".to_string();
        base.label = String::new();
        base.styles = strings(&[
            "formatted",
            "julian",
            "epoch",
            "iso",
            "age",
            "relative",
            "remaining",
            "countdown",
        ]);

        let mut now = Datetime::new();
        now -= 125; // So that "age" is non-zero.
        base.examples = vec![
            now.to_string(&config.get("dateformat")),
            format_double(now.to_julian(), 13, 12),
            now.to_epoch_string(),
            now.to_iso(),
            Duration::from_seconds(Datetime::new() - now).format_vague(true),
            format!(
                "-{}",
                Duration::from_seconds(Datetime::new() - now).format_vague(true)
            ),
            String::new(),
            Duration::from_seconds(Datetime::new() - now).format(),
        ];

        ColumnTypeDate { base }
    }

    /// The output date format of a `formatted` date column in `report`, as
    /// `ColumnTypeDate::measure` / `render` determine it from a hierarchy of
    /// definitions: `rc.report.<report>.dateformat`, then
    /// `rc.dateformat.report`, then `rc.dateformat`.
    pub fn report_dateformat(config: &Configuration, report: &str) -> String {
        let mut format = config.get(&format!("report.{report}.dateformat"));
        if format.is_empty() {
            format = config.get("dateformat.report");
        }
        if format.is_empty() {
            format = config.get("dateformat");
        }
        format
    }

    /// C++ `void ColumnTypeDate::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)` for the column `this` (its `name`, `style`
    /// and report are used): `(minimum, maximum)`, both equal, `(0, 0)` when
    /// the task has no such date or the style renders nothing.
    ///
    /// "Now" is `Datetime ()` (libc `time`), taken per style as in C++.
    /// `Datetime::length` is an `int` assigned to `unsigned int` in C++, so
    /// a negative length would wrap the same way here.
    pub fn measure_date(this: &dyn Column, task: &Task, env: &mut dyn RenderEnv) -> (usize, usize) {
        let name = this.name();
        let style = this.style();
        let mut minimum = 0usize;
        if task.has(name) {
            let date = Datetime::from_epoch(task.get_date(name));

            if style == "default" || style == "formatted" {
                let format = ColumnTypeDate::report_dateformat(env.config(), &this.base().report);
                minimum = Datetime::length(&format) as u32 as usize;
            } else if style == "countdown" {
                let now = Datetime::new();
                minimum = Duration::from_seconds(date - now).format_vague(true).len();
            } else if style == "julian" {
                minimum = format_double(date.to_julian(), 13, 12).len();
            } else if style == "epoch" {
                minimum = date.to_epoch_string().len();
            } else if style == "iso" {
                minimum = date.to_iso().len();
            } else if style == "age" {
                let now = Datetime::new();
                if now > date {
                    minimum = Duration::from_seconds(now - date).format_vague(true).len();
                } else {
                    minimum = Duration::from_seconds(date - now).format_vague(true).len() + 1;
                }
            } else if style == "relative" {
                let now = Datetime::new();
                if now < date {
                    minimum = Duration::from_seconds(date - now).format_vague(true).len();
                } else {
                    minimum = Duration::from_seconds(now - date).format_vague(true).len() + 1;
                }
            } else if style == "remaining" {
                let now = Datetime::new();
                if date > now {
                    minimum = Duration::from_seconds(date - now).format_vague(true).len();
                }
            }
        }
        (minimum, minimum)
    }

    /// C++ `void ColumnTypeDate::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)` for the column `this`: one
    /// line in the column's style, nothing when the task has no such date
    /// (or a `remaining` date is past).
    pub fn render_date(
        this: &dyn Column,
        lines: &mut Vec<String>,
        task: &Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) {
        let name = this.name();
        let style = this.style();
        if task.has(name) {
            let date = Datetime::from_epoch(task.get_date(name));

            if style == "default" || style == "formatted" {
                let format = ColumnTypeDate::report_dateformat(env.config(), &this.base().report);
                render_string_left(lines, width, color, &date.to_string(&format));
            } else if style == "countdown" {
                let now = Datetime::new();
                render_string_right(
                    lines,
                    width,
                    color,
                    &Duration::from_seconds(date - now).format_vague(true),
                );
            } else if style == "julian" {
                render_string_right(
                    lines,
                    width,
                    color,
                    &format_double(date.to_julian(), 13, 12),
                );
            } else if style == "epoch" {
                render_string_right(lines, width, color, &date.to_epoch_string());
            } else if style == "iso" {
                render_string_left(lines, width, color, &date.to_iso());
            } else if style == "age" {
                let now = Datetime::new();
                if now > date {
                    render_string_right(
                        lines,
                        width,
                        color,
                        &Duration::from_seconds(now - date).format_vague(true),
                    );
                } else {
                    render_string_right(
                        lines,
                        width,
                        color,
                        &format!("-{}", Duration::from_seconds(date - now).format_vague(true)),
                    );
                }
            } else if style == "relative" {
                let now = Datetime::new();
                if now < date {
                    render_string_right(
                        lines,
                        width,
                        color,
                        &Duration::from_seconds(date - now).format_vague(true),
                    );
                } else {
                    render_string_right(
                        lines,
                        width,
                        color,
                        &format!("-{}", Duration::from_seconds(now - date).format_vague(true)),
                    );
                }
            } else if style == "remaining" {
                let now = Datetime::new();
                if date > now {
                    render_string_right(
                        lines,
                        width,
                        color,
                        &Duration::from_seconds(date - now).format_vague(true),
                    );
                }
            }
        }
    }

    /// C++ `bool ColumnTypeDate::validate (const std::string& input) const`:
    /// any non-empty value.
    pub fn validate_date(input: &str) -> bool {
        !input.is_empty()
    }

    /// C++ `void ColumnTypeDate::modify (Task& task, const std::string&
    /// value)` for the column `this`: evaluates `value` (falling back to the
    /// literal string when evaluation fails), casts the result to a date
    /// (a duration is added to now) and stores the epoch.
    ///
    /// # Errors
    /// The `Variant::cast` error, `'{1}' is not a valid date in the '{2}'
    /// format.` (with `Variant::dateFormat`) when the date is 0, and `'{1}'
    /// is not a valid date.` outside `[EPOCH_MIN_VALUE, EPOCH_MAX_VALUE)`.
    pub fn modify_date(
        this: &dyn Column,
        task: &mut Task,
        value: &str,
        env: &mut dyn ModifyEnv,
    ) -> Result<()> {
        let name = this.name();

        // Try to evaluate 'value'.  It might work.
        let mut evaluated_value = match env.evaluate_dom_expression(value) {
            Ok(v) => v,
            Err(_) => Variant::from_string(value),
        };

        // If v is duration, we need to convert it to date (and implicitly add now),
        // else store as date.
        let label = MODIFICATION_LABEL;
        if evaluated_value.type_() == variant::Type::Duration {
            env.debug(&format!(
                "{label}{name} <-- '{}' <-- '{evaluated_value}' <-- '{value}'",
                format("{1}", &[&format_value(&evaluated_value.get_duration())])
            ));
            evaluated_value.cast(variant::Type::Date)?;
        } else {
            evaluated_value.cast(variant::Type::Date)?;
            env.debug(&format!(
                "{label}{name} <-- '{}' <-- '{evaluated_value}' <-- '{value}'",
                format("{1}", &[&evaluated_value.get_date()])
            ));
        }

        // If a date doesn't parse (2/29/2014) then it evaluates to zero.
        if !value.is_empty() && evaluated_value.get_date() == 0 {
            return Err(TwError::Message(format(
                "'{1}' is not a valid date in the '{2}' format.",
                &[&value, &Variant::date_format()],
            )));
        }

        let epoch = evaluated_value.get_date();
        if !(EPOCH_MIN_VALUE..EPOCH_MAX_VALUE).contains(&epoch) {
            return Err(TwError::Message(format(
                "'{1}' is not a valid date.",
                &[&value],
            )));
        }
        task.set_int(name, epoch);
        Ok(())
    }
}

impl Column for ColumnTypeDate {
    column_base!(base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeDate::validate_date(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDate::modify_date(self, task, value, env)
    }

    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        Ok(ColumnTypeDate::measure_date(self, task, env))
    }

    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        ColumnTypeDate::render_date(self, lines, task, width, color, env);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! `measure` / `render` of every date style, through the delegating
    //! `ColumnDue` / `ColumnEntry` and a [`FakeRenderEnv`].
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};
    use crate::shared::localtime;
    use crate::test_util::{env_lock, EnvGuard};

    /// 2023-11-14T22:13:20Z.
    const FIXED: i64 = 1_700_000_000;

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
            Utc {
                env: Some(env),
                _lock: lock,
            }
        }
    }

    impl Drop for Utc {
        fn drop(&mut self) {
            // Restore TZ and re-run tzset while the lock is still held.
            drop(self.env.take());
            localtime::tzset();
        }
    }

    fn env() -> FakeRenderEnv {
        FakeRenderEnv::with_config(&[
            ("dateformat", "Y-M-D"),
            ("dateformat.report", ""),
            ("report.list.dateformat", ""),
        ])
    }

    fn task_with(name: &str, epoch: i64) -> Task {
        let mut task = Task::new();
        task.set_int(name, epoch);
        task
    }

    /// `(measure, render lines)` of `column` (`<name>.<style>`) in report
    /// `list` at `width`.
    fn run(
        column: &str,
        task: &mut Task,
        width: usize,
        env: &mut FakeRenderEnv,
    ) -> ((usize, usize), Vec<String>) {
        let c = factory(column, "list", &env.config.clone()).unwrap();
        let m = c.measure(task, env).unwrap();
        let mut lines = Vec::new();
        c.render(&mut lines, task, width, &Color::new(), env)
            .unwrap();
        (m, lines)
    }

    #[test]
    fn absolute_styles() {
        let _utc = Utc::new();
        let mut env = env();
        let mut task = task_with("due", FIXED);

        assert_eq!(
            run("due", &mut task, 12, &mut env),
            ((10, 10), vec!["2023-11-14  ".to_string()])
        );
        assert_eq!(
            run("due.formatted", &mut task, 10, &mut env),
            ((10, 10), vec!["2023-11-14".to_string()])
        );
        assert_eq!(
            run("due.julian", &mut task, 15, &mut env),
            ((13, 13), vec!["  2460263.42593".to_string()])
        );
        assert_eq!(
            run("due.epoch", &mut task, 12, &mut env),
            ((10, 10), vec!["  1700000000".to_string()])
        );
        assert_eq!(
            run("due.iso", &mut task, 18, &mut env),
            ((16, 16), vec!["20231114T221320Z  ".to_string()])
        );

        // No such attribute: nothing measured or rendered, in any style.
        let mut empty = Task::new();
        for style in [
            "formatted",
            "julian",
            "epoch",
            "iso",
            "age",
            "relative",
            "remaining",
            "countdown",
        ] {
            assert_eq!(
                run(&format!("due.{style}"), &mut empty, 10, &mut env),
                ((0, 0), vec![]),
                "{style}"
            );
        }
    }

    #[test]
    fn dateformat_hierarchy() {
        let _utc = Utc::new();
        let mut task = task_with("entry", FIXED);

        // rc.dateformat
        let mut env = env();
        assert_eq!(
            run("entry", &mut task, 10, &mut env),
            ((10, 10), vec!["2023-11-14".to_string()])
        );
        // rc.dateformat.report beats rc.dateformat.
        env.config.set("dateformat.report", "D/M/Y H:N");
        assert_eq!(
            run("entry", &mut task, 16, &mut env),
            ((16, 16), vec!["14/11/2023 22:13".to_string()])
        );
        // rc.report.<report>.dateformat beats both.
        env.config.set("report.list.dateformat", "m/d");
        assert_eq!(
            run("entry", &mut task, 6, &mut env),
            ((5, 5), vec!["11/14 ".to_string()])
        );
        // Another report's format is not used.
        env.config.set("report.list.dateformat", "");
        env.config.set("report.next.dateformat", "m/d");
        assert_eq!(run("entry", &mut task, 16, &mut env).0, (16, 16));
        // Datetime::length counts the widest rendering of each letter.
        env.config.set("dateformat.report", "a b");
        assert_eq!(
            run("entry", &mut task, 7, &mut env),
            ((7, 7), vec!["Tue Nov".to_string()])
        );
    }

    #[test]
    fn relative_styles() {
        let _utc = Utc::new();
        let mut env = env();
        let now = localtime::time_now();
        // Half a day of slack keeps the vague value stable while the test runs.
        let past = now - 10 * 86400 - 43200;
        let future = now + 10 * 86400 + 43200;

        let mut task = task_with("due", past);
        assert_eq!(
            run("due.age", &mut task, 7, &mut env),
            ((5, 5), vec!["  10d  ".to_string()])
        );
        assert_eq!(
            run("due.relative", &mut task, 7, &mut env),
            ((6, 6), vec![" -10d  ".to_string()])
        );
        assert_eq!(
            run("due.remaining", &mut task, 7, &mut env),
            ((0, 0), vec![])
        );
        assert_eq!(
            run("due.countdown", &mut task, 7, &mut env),
            ((6, 6), vec![" -10d  ".to_string()])
        );

        let mut task = task_with("due", future);
        assert_eq!(
            run("due.age", &mut task, 7, &mut env),
            ((6, 6), vec![" -10d  ".to_string()])
        );
        assert_eq!(
            run("due.relative", &mut task, 7, &mut env),
            ((5, 5), vec!["  10d  ".to_string()])
        );
        assert_eq!(
            run("due.remaining", &mut task, 7, &mut env),
            ((5, 5), vec!["  10d  ".to_string()])
        );
        assert_eq!(
            run("due.countdown", &mut task, 7, &mut env),
            ((5, 5), vec!["  10d  ".to_string()])
        );

        // Larger spans: weeks, months and years, unpadded "min".
        let mut task = task_with("due", now - 30 * 86400 - 43200);
        assert_eq!(
            run("due.age", &mut task, 5, &mut env),
            ((4, 4), vec![" 4w  ".to_string()])
        );
        let mut task = task_with("due", now - 120 * 86400 - 43200);
        assert_eq!(
            run("due.age", &mut task, 5, &mut env),
            ((4, 4), vec![" 4mo ".to_string()])
        );
        let mut task = task_with("due", now - 800 * 86400);
        assert_eq!(
            run("due.age", &mut task, 6, &mut env),
            ((6, 6), vec!["2.2y  ".to_string()])
        );
        let mut task = task_with("due", now - 30 * 60 - 20);
        assert_eq!(
            run("due.age", &mut task, 5, &mut env),
            ((5, 5), vec!["30min".to_string()])
        );
    }

    #[test]
    fn delegating_date_columns() {
        let _utc = Utc::new();
        let mut env = env();
        for name in [
            "due",
            "end",
            "entry",
            "modified",
            "scheduled",
            "until",
            "wait",
        ] {
            let mut task = task_with(name, FIXED);
            assert_eq!(
                run(&format!("{name}.epoch"), &mut task, 10, &mut env),
                ((10, 10), vec!["1700000000".to_string()]),
                "{name}"
            );
            // Another date attribute is not this column's.
            let mut other = task_with(if name == "due" { "end" } else { "due" }, FIXED);
            assert_eq!(
                run(name, &mut other, 10, &mut env),
                ((0, 0), vec![]),
                "{name}"
            );
        }
        // The base itself renders too.
        let mut base = ColumnTypeDate::new(&env.config);
        base.base.name = "due".to_string();
        let mut task = task_with("due", FIXED);
        assert_eq!(base.measure(&mut task, &mut env).unwrap(), (10, 10));
    }

    #[test]
    fn formatted_colour_and_length() {
        let _utc = Utc::new();
        let mut env = env();
        let mut task = task_with("due", FIXED);
        let c = factory("due", "list", &env.config.clone()).unwrap();
        let red = Color::parse("red").unwrap();
        let mut lines = Vec::new();
        c.render(&mut lines, &mut task, 11, &red, &mut env).unwrap();
        assert_eq!(lines, vec!["\x1b[31m2023-11-14 \x1b[0m".to_string()]);

        // The width is exactly `Datetime::length`, also for bytes that are
        // not format letters (each counted by `mk_wcwidth`).
        env.config.set("dateformat", "Y \u{2013}");
        let m = c.measure(&mut task, &mut env).unwrap();
        let expected = Datetime::length("Y \u{2013}") as u32 as usize;
        assert_eq!(m, (expected, expected));
    }

    fn date_column() -> ColumnTypeDate {
        let mut c = ColumnTypeDate::new(&Configuration::new());
        c.base.name = "when".to_string();
        c
    }

    fn modified(value: &str) -> Result<String> {
        let mut task = Task::new();
        let mut env = crate::columns::column::FakeModifyEnv::default();
        ColumnTypeDate::modify_date(&date_column(), &mut task, value, &mut env)?;
        Ok(task.get("when"))
    }

    #[test]
    fn constructor_and_validate() {
        let c = ColumnTypeDate::new(&Configuration::new());
        assert_eq!(
            (c.name(), c.type_(), c.style(), c.label()),
            ("", "date", "formatted", "")
        );
        assert!(c.modifiable() && !c.is_uda() && !c.is_fixed_width());
        assert_eq!(c.styles().len(), 8);
        assert_eq!(c.examples().len(), 8);
        assert!(ColumnTypeDate::validate_date("x") && !ColumnTypeDate::validate_date(""));
        assert!(c.validate("2030-01-01") && !c.validate(""));
    }

    #[test]
    fn modify_epoch_range() {
        let _utc = Utc::new();
        // [EPOCH_MIN_VALUE, EPOCH_MAX_VALUE): 1980-01-01T00:00:00Z is the
        // first valid date.
        assert_eq!(
            modified("1980-01-01T00:00:00").unwrap(),
            EPOCH_MIN_VALUE.to_string()
        );
        assert_eq!(
            modified("1979-12-31T23:59:59").unwrap_err().to_string(),
            "'1979-12-31T23:59:59' is not a valid date."
        );
        assert_eq!(modified("2023-11-14T22:13:20").unwrap(), FIXED.to_string());
        // An in-range epoch literal is a date.
        assert_eq!(modified(&FIXED.to_string()).unwrap(), FIXED.to_string());
    }

    #[test]
    fn modify_unparseable_and_empty() {
        // A string that is neither a date nor a duration evaluates to 0.
        let err = modified("abc").unwrap_err().to_string();
        assert_eq!(
            err,
            format!(
                "'abc' is not a valid date in the '{}' format.",
                Variant::date_format()
            )
        );
        // An empty value skips that check, then fails the range check.
        assert_eq!(
            modified("").unwrap_err().to_string(),
            "'' is not a valid date."
        );
    }

    #[test]
    fn modify_duration_is_relative_to_now() {
        let before = localtime::time_now();
        let stored: i64 = modified("1h").unwrap().parse().unwrap();
        let after = localtime::time_now();
        assert!(stored >= before + 3600 && stored <= after + 3600);
    }
}
