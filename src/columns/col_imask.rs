//! Rust translation of `src/columns/ColIMask.cpp`.
//!
//! Constructor metadata and `measure` / `render` (the raw value,
//! right-justified); `validate` and `modify` are inherited from `ColumnTypeNumeric`.

use crate::columns::col_type_numeric::ColumnTypeNumeric;
use crate::columns::column::{column_base, render_string_right, strings, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::task::Task;

/// C++ `class ColumnIMask : public ColumnTypeNumeric`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnIMask {
    /// The inherited `ColumnTypeNumeric`.
    pub parent: ColumnTypeNumeric,
}

impl Default for ColumnIMask {
    fn default() -> Self {
        ColumnIMask::new()
    }
}

impl ColumnIMask {
    /// C++ `ColumnIMask::ColumnIMask ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeNumeric::new();
        parent.base.name = "imask".to_string();
        parent.base.style = "number".to_string();
        parent.base.label = "Mask Index".to_string();
        parent.base.modifiable = false;
        parent.base.styles = strings(&["number"]);
        parent.base.examples = strings(&["12"]);
        ColumnIMask { parent }
    }
}

impl Column for ColumnIMask {
    column_base!(parent.base);

    /// C++ `void ColumnIMask::measure (Task& task, unsigned int& minimum, unsigned
    /// int& maximum)`: the byte length of the value, `(0, 0)` when the task
    /// has no `imask`.
    fn measure(&self, task: &mut Task, _env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0;
        if task.has(self.name()) {
            minimum = task.get_ref(self.name()).len();
        }
        Ok((minimum, minimum))
    }

    /// C++ `void ColumnIMask::render (std::vector<std::string>& lines, Task& task,
    /// int width, Color& color)`: the value, right-justified.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        _env: &mut dyn RenderEnv,
    ) -> Result<()> {
        if task.has(self.name()) {
            render_string_right(lines, width, color, task.get_ref(self.name()));
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeNumeric::validate_numeric(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeNumeric::modify_numeric(self, task, value, env)
    }
}

#[cfg(test)]
mod tests {
    //! `measure` / `render` of the `imask` column against a
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
    fn number_style() {
        let mut env = FakeRenderEnv::with_config(&[]);
        let none = Color::new();
        assert_eq!(run("imask", &mut with("imask", "12"), 4, &none, &mut env), ((2, 2), vec!["  12".to_string()]));
        assert_eq!(run("imask.number", &mut with("imask", "3"), 1, &none, &mut env), ((1, 1), vec!["3".to_string()]));
        let red = Color::parse("red").unwrap();
        assert_eq!(
            run("imask", &mut with("imask", "7"), 2, &red, &mut env).1,
            vec!["\x1b[31m 7\x1b[0m".to_string()]
        );
        assert_eq!(run("imask", &mut Task::new(), 2, &none, &mut env), ((0, 0), vec![]));
    }
}
