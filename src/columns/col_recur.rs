//! Rust translation of `src/columns/ColRecur.cpp`.
//!
//! Constructor metadata, the label-linking `setStyle`, `measure` /
//! `render` (the `duration` and `indicator` styles) and `modify`;
//! `validate` is inherited from `ColumnTypeString`.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{
    column_base, label_prefix, render_string_right, Column, ModifyEnv, RenderEnv,
    MODIFICATION_LABEL,
};
use crate::error::{Result, TwError};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::duration::Duration;
use crate::shared::format::format;
use crate::shared::utf8::utf8_width;
use crate::task::Task;
use crate::variant::{self, Variant};

/// C++ `class ColumnRecur : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRecur {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
}

impl ColumnRecur {
    /// C++ `ColumnRecur::ColumnRecur ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "recur".to_string();
        parent.base.style = "duration".to_string();
        parent.base.label = "Recur".to_string();
        parent.base.modifiable = true;
        parent.base.styles = vec!["duration".to_string(), "indicator".to_string()];
        parent.base.examples = vec!["weekly".to_string(), config.get("recurrence.indicator")];
        ColumnRecur { parent }
    }
}

impl Column for ColumnRecur {
    column_base!(parent.base);

    /// C++ `void ColumnRecur::setStyle (const std::string& value)`:
    /// overridden so that style <----> label are linked (cut to the width of
    /// `rc.recurrence.indicator`).
    fn set_style(&mut self, value: &str, config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "indicator" && base.label == "Recur" {
            base.label = label_prefix(&base.label, config.get("recurrence.indicator").len());
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    /// C++ `void ColumnRecur::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`: the length of the ISO form of the recurrence
    /// period (`duration`), or the width of `rc.recurrence.indicator`
    /// (`indicator`); `(0, 0)` when the task has no `recur`.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0;
        let name = self.name();
        let style = self.style();
        if task.has(name) {
            if style == "default" || style == "duration" {
                minimum = Duration::from_string(task.get_ref(name)).format_iso().len();
            } else if style == "indicator" {
                minimum = utf8_width(&env.config().get("recurrence.indicator")) as usize;
            }
        }
        Ok((minimum, minimum))
    }

    /// C++ `void ColumnRecur::render (std::vector<std::string>& lines, Task&
    /// task, int width, Color& color)`: the ISO form of the recurrence
    /// period (`duration`) or `rc.recurrence.indicator` (`indicator`),
    /// right-justified.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let name = self.name();
        let style = self.style();
        if task.has(name) {
            if style == "default" || style == "duration" {
                render_string_right(
                    lines,
                    width,
                    color,
                    &Duration::from_string(task.get_ref(name)).format_iso(),
                );
            } else if style == "indicator" {
                render_string_right(
                    lines,
                    width,
                    color,
                    &env.config().get("recurrence.indicator"),
                );
            }
        }
        Ok(())
    }

    /// C++ `void ColumnRecur::modify (Task& task, const std::string& value)`:
    /// the duration is stored in raw form, but it must still be valid, and
    /// therefore is parsed (evaluated) first.
    ///
    /// # Errors
    /// `The duration value '{1}' is not supported.`
    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        let name = self.name();

        // Try to evaluate 'value'.  It might work.
        let evaluated_value = match env.evaluate_dom_expression(value) {
            Ok(v) => v,
            Err(_) => Variant::from_string(value),
        };

        if evaluated_value.type_() == variant::Type::Duration {
            // Store the raw value, for 'recur'.
            let label = MODIFICATION_LABEL;
            env.debug(&format!("{label}{name} <-- '{value}'"));
            task.set(name, value);
        } else {
            return Err(TwError::Message(format(
                "The duration value '{1}' is not supported.",
                &[&value],
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! `measure` / `render` of every `recur` style against a
    //! [`FakeRenderEnv`].
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};

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

    fn recurring(period: &str) -> Task {
        let mut task = Task::new();
        task.set("recur", period);
        task
    }

    #[test]
    fn duration_style() {
        let mut env = FakeRenderEnv::with_config(&[("recurrence.indicator", "R")]);
        let none = Color::new();
        assert_eq!(
            run("recur", &mut recurring("weekly"), 4, &none, &mut env),
            ((3, 3), vec![" P7D".to_string()])
        );
        assert_eq!(
            run(
                "recur.duration",
                &mut recurring("monthly"),
                4,
                &none,
                &mut env
            ),
            ((4, 4), vec!["P30D".to_string()])
        );
        assert_eq!(
            run("recur", &mut recurring("2h"), 4, &none, &mut env),
            ((4, 4), vec!["PT2H".to_string()])
        );
        // An unparsable period is a zero duration.
        assert_eq!(
            run("recur", &mut recurring("bogus"), 4, &none, &mut env),
            ((4, 4), vec!["PT0S".to_string()])
        );
        let red = Color::parse("red").unwrap();
        assert_eq!(
            run("recur", &mut recurring("daily"), 4, &red, &mut env).1,
            vec!["\x1b[31m P1D\x1b[0m".to_string()]
        );
    }

    #[test]
    fn indicator_style() {
        let mut env = FakeRenderEnv::with_config(&[("recurrence.indicator", "R")]);
        let none = Color::new();
        assert_eq!(
            run(
                "recur.indicator",
                &mut recurring("weekly"),
                1,
                &none,
                &mut env
            ),
            ((1, 1), vec!["R".to_string()])
        );
        assert_eq!(
            run(
                "recur.indicator",
                &mut recurring("weekly"),
                3,
                &none,
                &mut env
            ),
            ((1, 1), vec!["  R".to_string()])
        );
        env.config.set("recurrence.indicator", "\u{21bb}");
        assert_eq!(
            run(
                "recur.indicator",
                &mut recurring("weekly"),
                2,
                &none,
                &mut env
            ),
            ((1, 1), vec![" \u{21bb}".to_string()])
        );
    }

    #[test]
    fn not_recurring() {
        let mut env = FakeRenderEnv::with_config(&[("recurrence.indicator", "R")]);
        for style in ["duration", "indicator"] {
            assert_eq!(
                run(
                    &format!("recur.{style}"),
                    &mut Task::new(),
                    3,
                    &Color::new(),
                    &mut env
                ),
                ((0, 0), vec![])
            );
        }
    }
}
