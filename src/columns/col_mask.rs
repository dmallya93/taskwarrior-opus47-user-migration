//! Rust translation of `src/columns/ColMask.cpp`.
//!
//! Constructor metadata and `measure` / `render` (the raw value,
//! left-justified); `validate` and `modify` are inherited from `ColumnTypeString`.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, render_string_left, strings, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::task::Task;

/// C++ `class ColumnMask : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMask {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
}

impl Default for ColumnMask {
    fn default() -> Self {
        ColumnMask::new()
    }
}

impl ColumnMask {
    /// C++ `ColumnMask::ColumnMask ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "mask".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = "Mask".to_string();
        parent.base.modifiable = false;
        parent.base.styles = strings(&["default"]);
        parent.base.examples = strings(&["++++---"]);
        ColumnMask { parent }
    }
}

impl Column for ColumnMask {
    column_base!(parent.base);

    /// C++ `void ColumnMask::measure (Task& task, unsigned int& minimum, unsigned
    /// int& maximum)`: the byte length of the value, `(0, 0)` when the task
    /// has no `mask`.
    fn measure(&self, task: &mut Task, _env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0;
        if task.has(self.name()) {
            minimum = task.get_ref(self.name()).len();
        }
        Ok((minimum, minimum))
    }

    /// C++ `void ColumnMask::render (std::vector<std::string>& lines, Task& task,
    /// int width, Color& color)`: the value, left-justified.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        _env: &mut dyn RenderEnv,
    ) -> Result<()> {
        if task.has(self.name()) {
            render_string_left(lines, width, color, task.get_ref(self.name()));
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }
}

#[cfg(test)]
mod tests {
    //! `measure` / `render` of the `mask` column against a
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
        let mut env = FakeRenderEnv::with_config(&[]);
        let none = Color::new();
        assert_eq!(
            run("mask", &mut with("mask", "++-X"), 6, &none, &mut env),
            ((4, 4), vec!["++-X  ".to_string()])
        );
        assert_eq!(
            run("mask.default", &mut with("mask", "+++"), 3, &none, &mut env),
            ((3, 3), vec!["+++".to_string()])
        );
        let red = Color::parse("red").unwrap();
        assert_eq!(run("mask", &mut with("mask", "-"), 2, &red, &mut env).1, vec!["\x1b[31m- \x1b[0m".to_string()]);
        assert_eq!(run("mask", &mut Task::new(), 2, &none, &mut env), ((0, 0), vec![]));
    }
}
