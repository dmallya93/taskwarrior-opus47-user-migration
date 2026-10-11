//! Rust translation of `src/columns/ColUrgency.cpp`.
//!
//! Constructor metadata and the `measure` / `render` overrides (`real` /
//! `integer`); `validate` and `modify` are inherited from
//! `ColumnTypeNumeric`. `task.urgency ()` goes through
//! [`RenderEnv::urgency`].

use crate::columns::col_type_numeric::ColumnTypeNumeric;
use crate::columns::column::{column_base, render_double, render_integer, strings, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::format::{format_float, format_value};
use crate::task::Task;

/// C++ `class ColumnUrgency : public ColumnTypeNumeric`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUrgency {
    /// The inherited `ColumnTypeNumeric`.
    pub parent: ColumnTypeNumeric,
}

impl Default for ColumnUrgency {
    fn default() -> Self {
        ColumnUrgency::new()
    }
}

impl ColumnUrgency {
    /// C++ `ColumnUrgency::ColumnUrgency ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeNumeric::new();
        parent.base.name = "urgency".to_string();
        parent.base.style = "real".to_string();
        parent.base.label = "Urgency".to_string();
        parent.base.modifiable = false;
        parent.base.styles = strings(&["real", "integer"]);
        parent.base.examples = strings(&["4.6", "4"]);
        ColumnUrgency { parent }
    }
}

impl Column for ColumnUrgency {
    column_base!(parent.base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeNumeric::validate_numeric(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeNumeric::modify_numeric(self, task, value, env)
    }

    /// C++ `void ColumnUrgency::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`: the length of `format (task.urgency (), 4,
    /// 3)` (the `float` overload, as `urgency ()` returns `float`) or of
    /// `format ((int) task.urgency ())`.
    ///
    /// # Errors
    /// Those of `Task::urgency`.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let style = self.style();
        if style == "default" || style == "real" {
            let len = format_float(env.urgency(task)?, 4, 3).len();
            Ok((len, len))
        } else if style == "integer" {
            let len = format_value(&(env.urgency(task)? as i32)).len();
            Ok((len, len))
        } else {
            Ok((0, 0))
        }
    }

    /// C++ `void ColumnUrgency::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)`: `renderDouble` of the `float`
    /// urgency promoted to `double`, or `renderInteger` of its truncation.
    ///
    /// # Errors
    /// Those of `Task::urgency`.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let style = self.style();
        if style == "default" || style == "real" {
            render_double(lines, width, color, env.urgency(task)? as f64);
        } else if style == "integer" {
            render_integer(lines, width, color, env.urgency(task)? as i32);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};
    use crate::shared::configuration::Configuration;

    fn run(style: &str, urgency: f32, width: usize) -> ((usize, usize), Vec<String>) {
        let column = factory(&format!("urgency.{style}"), "list", &Configuration::new()).unwrap();
        let mut env = FakeRenderEnv::with_config(&[]);
        env.urgency = urgency;
        let mut task = Task::new();
        let m = column.measure(&mut task, &mut env).unwrap();
        let mut lines = Vec::new();
        column.render(&mut lines, &mut task, width, &Color::new(), &mut env).unwrap();
        (m, lines)
    }

    #[test]
    fn real() {
        assert_eq!(run("real", 4.6, 4), ((4, 4), vec![" 4.6".to_string()]));
        // "%g" at 3 significant digits, padded to 4; rendered in width 5.
        assert_eq!(run("default", 12.345, 5), ((4, 4), vec![" 12.3".to_string()]));
        assert_eq!(run("real", 0.0, 4), ((4, 4), vec!["   0".to_string()]));
        assert_eq!(run("real", 0.75, 4), ((4, 4), vec!["0.75".to_string()]));
        assert_eq!(run("real", -1.5, 4), ((4, 4), vec!["-1.5".to_string()]));
    }

    #[test]
    fn integer() {
        assert_eq!(run("integer", 4.6, 3), ((1, 1), vec!["  4".to_string()]));
        assert_eq!(run("integer", -12.9, 3), ((3, 3), vec!["-12".to_string()]));
    }
}
