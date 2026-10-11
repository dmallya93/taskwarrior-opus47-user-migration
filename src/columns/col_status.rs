//! Rust translation of `src/columns/ColStatus.cpp`.
//!
//! Constructor metadata, the label-linking `setStyle` and the `measure` /
//! `render` overrides (`long` / `short`); `validate` and `modify` are
//! inherited from `ColumnTypeString`.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{
    column_base, render_string_left, strings, Column, ModifyEnv, RenderEnv,
};
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::utf8::utf8_width;
use crate::task::{Status, Task};

/// C++ `class ColumnStatus : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnStatus {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
}

impl Default for ColumnStatus {
    fn default() -> Self {
        ColumnStatus::new()
    }
}

impl ColumnStatus {
    /// C++ `ColumnStatus::ColumnStatus ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "status".to_string();
        parent.base.style = "long".to_string();
        parent.base.label = "Status".to_string();
        parent.base.styles = strings(&["long", "short"]);
        parent.base.examples = strings(&["Pending", "P"]);
        ColumnStatus { parent }
    }
}

impl Column for ColumnStatus {
    column_base!(parent.base);

    /// C++ `void ColumnStatus::setStyle (const std::string& value)`: overridden
    /// so that style <----> label are linked.
    fn set_style(&mut self, value: &str, _config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "short" && base.label == "Status" {
            base.label = "St".to_string();
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }

    /// C++ `void ColumnStatus::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`.
    ///
    /// # Errors
    /// The `getStatus` error for an invalid stored status.
    fn measure(&self, task: &mut Task, _env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let status = task.get_status()?;
        let style = self.style();
        let width = if style == "default" || style == "long" {
            match status {
                Status::Pending => utf8_width("Pending"),
                Status::Deleted => utf8_width("Deleted"),
                Status::Waiting => utf8_width("Waiting"),
                Status::Completed => utf8_width("Completed"),
                Status::Recurring => utf8_width("Recurring"),
            }
        } else if style == "short" {
            1
        } else {
            0
        } as usize;
        Ok((width, width))
    }

    /// C++ `void ColumnStatus::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)`.
    ///
    /// # Errors
    /// The `getStatus` error for an invalid stored status.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        _env: &mut dyn RenderEnv,
    ) -> Result<()> {
        let status = task.get_status()?;
        let style = self.style();
        let value = if style == "default" || style == "long" {
            match status {
                Status::Pending => "Pending",
                Status::Completed => "Completed",
                Status::Deleted => "Deleted",
                Status::Waiting => "Waiting",
                Status::Recurring => "Recurring",
            }
        } else if style == "short" {
            match status {
                Status::Pending => "P",
                Status::Completed => "C",
                Status::Deleted => "D",
                Status::Waiting => "W",
                Status::Recurring => "R",
            }
        } else {
            ""
        };

        render_string_left(lines, width, color, value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};

    fn run(style: &str, status: &str, width: usize) -> ((usize, usize), Vec<String>) {
        let column = factory(&format!("status.{style}"), "list", &Configuration::new()).unwrap();
        let mut env = FakeRenderEnv::with_config(&[]);
        let mut task = Task::new();
        if !status.is_empty() {
            task.set("status", status);
        }
        let m = column.measure(&mut task, &mut env).unwrap();
        let mut lines = Vec::new();
        column
            .render(&mut lines, &mut task, width, &Color::new(), &mut env)
            .unwrap();
        (m, lines)
    }

    #[test]
    fn long() {
        assert_eq!(run("long", "", 9), ((7, 7), vec!["Pending  ".to_string()]));
        assert_eq!(
            run("default", "completed", 9),
            ((9, 9), vec!["Completed".to_string()])
        );
        assert_eq!(
            run("long", "deleted", 7),
            ((7, 7), vec!["Deleted".to_string()])
        );
        assert_eq!(
            run("long", "recurring", 9),
            ((9, 9), vec!["Recurring".to_string()])
        );
        // A pending task with a future wait is virtually waiting.
        let column = factory("status.long", "list", &Configuration::new()).unwrap();
        let mut env = FakeRenderEnv::with_config(&[]);
        let mut task = Task::new();
        task.set("status", "pending");
        task.set("wait", "4102444800"); // 2100-01-01
        assert_eq!(column.measure(&mut task, &mut env).unwrap(), (7, 7));
        let mut lines = Vec::new();
        column
            .render(&mut lines, &mut task, 7, &Color::new(), &mut env)
            .unwrap();
        assert_eq!(lines, vec!["Waiting"]);
    }

    #[test]
    fn short() {
        assert_eq!(run("short", "pending", 2), ((1, 1), vec!["P ".to_string()]));
        assert_eq!(
            run("short", "completed", 1),
            ((1, 1), vec!["C".to_string()])
        );
        assert_eq!(run("short", "deleted", 1), ((1, 1), vec!["D".to_string()]));
        assert_eq!(
            run("short", "recurring", 1),
            ((1, 1), vec!["R".to_string()])
        );
    }

    #[test]
    fn invalid_status_is_an_error() {
        let column = factory("status", "list", &Configuration::new()).unwrap();
        let mut env = FakeRenderEnv::with_config(&[]);
        let mut task = Task::new();
        task.set("status", "bogus");
        assert!(column.measure(&mut task, &mut env).is_err());
        assert!(column
            .render(&mut Vec::new(), &mut task, 7, &Color::new(), &mut env)
            .is_err());
    }
}
