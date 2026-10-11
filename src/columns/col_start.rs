//! Rust translation of `src/columns/ColStart.cpp`.
//!
//! Constructor metadata (adds the `active` style), the label-linking
//! `setStyle` and the `measure` / `render` overrides (the `active`
//! indicator; every other style delegates to the `ColumnTypeDate`
//! helpers); `validate` and `modify` are inherited from `ColumnTypeDate`.

use crate::columns::col_type_date::ColumnTypeDate;
use crate::columns::column::{column_base, render_string_right, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::utf8::utf8_width;
use crate::task::Task;

/// C++ `class ColumnStart : public ColumnTypeDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnStart {
    /// The inherited `ColumnTypeDate`.
    pub parent: ColumnTypeDate,
}

impl ColumnStart {
    /// C++ `ColumnStart::ColumnStart ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeDate::new(config);
        parent.base.name = "start".to_string();
        parent.base.label = "Started".to_string();
        parent.base.styles.push("active".to_string());
        parent.base.examples.push(config.get("active.indicator"));
        ColumnStart { parent }
    }
}

impl Column for ColumnStart {
    column_base!(parent.base);

    /// C++ `void ColumnStart::setStyle (const std::string& value)`:
    /// overridden so that style <----> label are linked.
    fn set_style(&mut self, value: &str, _config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "active" && base.label == "Started" {
            base.label = "A".to_string();
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeDate::validate_date(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDate::modify_date(self, task, value, env)
    }

    /// C++ `void ColumnStart::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`: the width of `rc.active.indicator` for the
    /// `active` style (also when the task has ended), otherwise
    /// [`ColumnTypeDate::measure_date`]; `(0, 0)` without a start date.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0;
        if task.has(self.name()) {
            if self.style() == "active" {
                minimum = utf8_width(&env.config().get("active.indicator")) as usize;
            } else {
                return Ok(ColumnTypeDate::measure_date(self, task, env));
            }

            // TODO Throw on bad format.
        }
        Ok((minimum, minimum))
    }

    /// C++ `void ColumnStart::render (std::vector<std::string>& lines, Task&
    /// task, int width, Color& color)`: for the `active` style,
    /// `rc.active.indicator` right-justified unless the task has an `end`;
    /// otherwise [`ColumnTypeDate::render_date`].
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        if task.has(self.name()) {
            if self.style() == "active" {
                if !task.has("end") {
                    render_string_right(lines, width, color, &env.config().get("active.indicator"));
                }
            } else {
                ColumnTypeDate::render_date(self, lines, task, width, color, env);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! `measure` / `render` of every `start` style against a
    //! [`FakeRenderEnv`].
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};
    use crate::shared::localtime;
    use crate::test_util::{env_lock, EnvGuard};

    /// 2023-11-14T22:13:20Z.
    const FIXED: i64 = 1_700_000_000;

    fn env() -> FakeRenderEnv {
        FakeRenderEnv::with_config(&[("dateformat", "Y-M-D"), ("active.indicator", "*")])
    }

    /// `(measure, render lines)` of `column` in report `list`.
    fn run(
        column: &str,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut FakeRenderEnv,
    ) -> ((usize, usize), Vec<String>) {
        let c = factory(column, "list", &env.config.clone()).unwrap();
        let m = c.measure(task, env).unwrap();
        let mut lines = Vec::new();
        c.render(&mut lines, task, width, color, env).unwrap();
        (m, lines)
    }

    #[test]
    fn active_indicator() {
        let mut env = env();
        let none = Color::new();
        let mut task = Task::new();
        task.set_int("start", FIXED);
        assert_eq!(run("start.active", &mut task, 1, &none, &mut env), ((1, 1), vec!["*".to_string()]));
        assert_eq!(run("start.active", &mut task, 3, &none, &mut env), ((1, 1), vec!["  *".to_string()]));

        let red = Color::parse("red").unwrap();
        assert_eq!(run("start.active", &mut task, 2, &red, &mut env).1, vec!["\x1b[31m *\x1b[0m".to_string()]);

        // Multi-byte indicator: the width is in display columns.
        env.config.set("active.indicator", "\u{25b6}\u{25b6}");
        assert_eq!(
            run("start.active", &mut task, 3, &none, &mut env),
            ((2, 2), vec![" \u{25b6}\u{25b6}".to_string()])
        );

        // Ended: still measured, but nothing rendered.
        task.set_int("end", FIXED);
        assert_eq!(run("start.active", &mut task, 2, &none, &mut env), ((2, 2), vec![]));

        // Not started: nothing at all.
        let mut empty = Task::new();
        assert_eq!(run("start.active", &mut empty, 2, &none, &mut env), ((0, 0), vec![]));
    }

    #[test]
    fn date_styles_delegate() {
        let _lock = env_lock();
        let tz = EnvGuard::save(&["TZ"]);
        std::env::set_var("TZ", "UTC");
        localtime::tzset();

        let mut env = env();
        let none = Color::new();
        let mut task = Task::new();
        task.set_int("start", FIXED);
        assert_eq!(run("start", &mut task, 12, &none, &mut env), ((10, 10), vec!["2023-11-14  ".to_string()]));
        assert_eq!(run("start.epoch", &mut task, 10, &none, &mut env), ((10, 10), vec!["1700000000".to_string()]));
        assert_eq!(
            run("start.iso", &mut task, 16, &none, &mut env),
            ((16, 16), vec!["20231114T221320Z".to_string()])
        );
        // Date styles do not care about `end`.
        task.set_int("end", FIXED);
        assert_eq!(run("start.epoch", &mut task, 10, &none, &mut env).1, vec!["1700000000".to_string()]);
        let mut empty = Task::new();
        assert_eq!(run("start", &mut empty, 10, &none, &mut env), ((0, 0), vec![]));

        drop(tz);
        localtime::tzset();
    }
}
