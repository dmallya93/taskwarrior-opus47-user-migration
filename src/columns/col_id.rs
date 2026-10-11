//! Rust translation of `src/columns/ColID.cpp`.
//!
//! Constructor metadata and the `measure` / `render` overrides; `validate`
//! and `modify` are inherited from `ColumnTypeNumeric`.

use crate::columns::col_type_numeric::ColumnTypeNumeric;
use crate::columns::column::{
    column_base, render_integer, render_string_right, strings, Column, ModifyEnv, RenderEnv,
};
use crate::error::Result;
use crate::shared::color::Color;
use crate::task::Task;

/// C++ `class ColumnID : public ColumnTypeNumeric`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnID {
    /// The inherited `ColumnTypeNumeric`.
    pub parent: ColumnTypeNumeric,
}

impl Default for ColumnID {
    fn default() -> Self {
        ColumnID::new()
    }
}

impl ColumnID {
    /// C++ `ColumnID::ColumnID ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeNumeric::new();
        parent.base.name = "id".to_string();
        parent.base.style = "number".to_string();
        parent.base.label = "ID".to_string();
        parent.base.modifiable = false;
        parent.base.styles = strings(&["number"]);
        parent.base.examples = strings(&["123"]);
        ColumnID { parent }
    }
}

impl Column for ColumnID {
    column_base!(parent.base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeNumeric::validate_numeric(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeNumeric::modify_numeric(self, task, value, env)
    }

    /// C++ `void ColumnID::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`: the number of digits of the ID (1 for 0 and
    /// negative IDs, as the C++ `< 10` fast path gives).
    fn measure(&self, task: &mut Task, _env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let id = task.id;
        let length = if id < 10 {
            1 // Fast
        } else if id < 100 {
            2 // Fast
        } else if id < 1000 {
            3 // Fast
        } else if id < 10000 {
            4 // Fast
        } else if id < 100000 {
            5 // Fast
        } else {
            1 + (id as f64).log10() as usize // Slow
        };
        Ok((length, length))
    }

    /// C++ `void ColumnID::render (std::vector<std::string>& lines, Task&
    /// task, int width, Color& color)`: completed and deleted tasks have no
    /// ID and show `-`.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        _env: &mut dyn RenderEnv,
    ) -> Result<()> {
        // Completed and deleted tasks have no ID.
        if task.id != 0 {
            render_integer(lines, width, color, task.id);
        } else {
            render_string_right(lines, width, color, "-");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::column::FakeRenderEnv;

    fn measure(id: i32) -> (usize, usize) {
        let mut task = Task::new();
        task.id = id;
        ColumnID::new()
            .measure(&mut task, &mut FakeRenderEnv::with_config(&[]))
            .unwrap()
    }

    #[test]
    fn measure_digits() {
        assert_eq!(measure(0), (1, 1));
        assert_eq!(measure(-5), (1, 1));
        assert_eq!(measure(9), (1, 1));
        assert_eq!(measure(10), (2, 2));
        assert_eq!(measure(999), (3, 3));
        assert_eq!(measure(1000), (4, 4));
        assert_eq!(measure(99999), (5, 5));
        assert_eq!(measure(100000), (6, 6));
        assert_eq!(measure(1234567), (7, 7));
    }

    #[test]
    fn render_number_or_dash() {
        let mut env = FakeRenderEnv::with_config(&[]);
        let mut lines = Vec::new();
        let mut task = Task::new();
        task.id = 12;
        ColumnID::new()
            .render(&mut lines, &mut task, 4, &Color::new(), &mut env)
            .unwrap();
        task.id = 0;
        ColumnID::new()
            .render(&mut lines, &mut task, 4, &Color::new(), &mut env)
            .unwrap();
        assert_eq!(lines, vec!["  12", "   -"]);
    }
}
