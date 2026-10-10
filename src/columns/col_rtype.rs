//! Rust translation of `src/columns/ColRType.cpp`.
//!
//! Constructor metadata, the label-linking `setStyle`, `measure` /
//! `render` (the `default` and `indicator` styles) and `validate`; `modify`
//! is inherited from `ColumnTypeString` (the column is not modifiable, so
//! `Task::modify` never calls it).

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, label_prefix, render_string_right, strings, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::task::Task;

/// C++ `class ColumnRType : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRType {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
}

impl Default for ColumnRType {
    fn default() -> Self {
        ColumnRType::new()
    }
}

impl ColumnRType {
    /// C++ `ColumnRType::ColumnRType ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "rtype".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = "Recurrence type".to_string();
        parent.base.modifiable = false;
        parent.base.styles = strings(&["default", "indicator"]);
        parent.base.examples = strings(&["periodic", "chained"]);
        ColumnRType { parent }
    }
}

impl Column for ColumnRType {
    column_base!(parent.base);

    /// C++ `void ColumnRType::setStyle (const std::string& value)`:
    /// overridden so that style <----> label are linked (cut to the width of
    /// `rc.rtype.indicator`).
    fn set_style(&mut self, value: &str, config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "indicator" && base.label == "Recurrence type" {
            base.label = label_prefix(&base.label, config.get("rtype.indicator").len());
        }
        Ok(())
    }

    /// C++ `void ColumnRType::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`: the byte length of the value (`default`) or
    /// 1 (`indicator`); `(0, 0)` when the task has no `rtype`.
    fn measure(&self, task: &mut Task, _env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0;
        let name = self.name();
        let style = self.style();
        if task.has(name) {
            if style == "default" {
                minimum = task.get_ref(name).len();
            } else if style == "indicator" {
                minimum = 1;
            }
        }
        Ok((minimum, minimum))
    }

    /// C++ `void ColumnRType::render (std::vector<std::string>& lines, Task&
    /// task, int width, Color& color)`: the value (`default`) or its
    /// upper-cased first letter (`indicator`, `P` / `C`), right-justified.
    ///
    /// Deviation: C++ upper-cases the first byte; a non-ASCII first
    /// character is kept whole here (a lone byte is not a valid Rust
    /// string). An empty value gives `"\0"`, as C++ `get (_name)[0]` does.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        _env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let name = self.name();
        let style = self.style();
        if task.has(name) {
            if style == "default" {
                render_string_right(lines, width, color, task.get_ref(name));
            } else if style == "indicator" {
                let first = task.get_ref(name).chars().next().unwrap_or('\0');
                let value = first.to_ascii_uppercase().to_string();
                render_string_right(lines, width, color, &value);
            }
        }
        Ok(())
    }

    /// C++ `bool ColumnRType::validate (const std::string& input) const`:
    /// `periodic` or `chained`.
    fn validate(&self, input: &str) -> bool {
        input == "periodic" || input == "chained"
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }
}

#[cfg(test)]
mod tests {
    //! `measure` / `render` of every `rtype` style against a
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

    fn with(name: &str, value: &str) -> Task {
        let mut task = Task::new();
        task.set(name, value);
        task
    }

    #[test]
    fn default_style() {
        let mut env = FakeRenderEnv::with_config(&[("rtype.indicator", "R")]);
        let none = Color::new();
        assert_eq!(
            run("rtype", &mut with("rtype", "periodic"), 10, &none, &mut env),
            ((8, 8), vec!["  periodic".to_string()])
        );
        assert_eq!(
            run("rtype.default", &mut with("rtype", "chained"), 7, &none, &mut env),
            ((7, 7), vec!["chained".to_string()])
        );
        let red = Color::parse("red").unwrap();
        assert_eq!(
            run("rtype", &mut with("rtype", "chained"), 8, &red, &mut env).1,
            vec!["\x1b[31m chained\x1b[0m".to_string()]
        );
    }

    #[test]
    fn indicator_style() {
        let mut env = FakeRenderEnv::with_config(&[("rtype.indicator", "R")]);
        let none = Color::new();
        assert_eq!(
            run("rtype.indicator", &mut with("rtype", "periodic"), 1, &none, &mut env),
            ((1, 1), vec!["P".to_string()])
        );
        assert_eq!(
            run("rtype.indicator", &mut with("rtype", "chained"), 3, &none, &mut env),
            ((1, 1), vec!["  C".to_string()])
        );
        // C++ `std::string::operator[] (0)` on an empty value is the NUL, of
        // display width 0, so it is padded.
        assert_eq!(run("rtype.indicator", &mut with("rtype", ""), 1, &none, &mut env), ((1, 1), vec![" \0".to_string()]));
    }

    #[test]
    fn no_rtype() {
        let mut env = FakeRenderEnv::with_config(&[("rtype.indicator", "R")]);
        for style in ["default", "indicator"] {
            assert_eq!(run(&format!("rtype.{style}"), &mut Task::new(), 3, &Color::new(), &mut env), ((0, 0), vec![]));
        }
    }
}
