//! Rust translation of `src/columns/ColUUID.cpp`.
//!
//! Constructor metadata and the `measure` / `render` overrides (`long` /
//! `short`); `validate` and `modify` are inherited from `ColumnTypeString`.
//! `ColumnUDAUUID` reuses these through its `parent`.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{
    column_base, render_string_left, strings, Column, ModifyEnv, RenderEnv,
};
use crate::error::Result;
use crate::shared::color::Color;
use crate::task::Task;

/// C++ `class ColumnUUID : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUUID {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
}

impl Default for ColumnUUID {
    fn default() -> Self {
        ColumnUUID::new()
    }
}

impl ColumnUUID {
    /// C++ `ColumnUUID::ColumnUUID ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "uuid".to_string();
        parent.base.style = "long".to_string();
        parent.base.label = "UUID".to_string();
        parent.base.modifiable = false;
        parent.base.styles = strings(&["long", "short"]);
        parent.base.examples = strings(&["f30cb9c3-3fc0-483f-bfb2-3bf134f00694", "f30cb9c3"]);
        ColumnUUID { parent }
    }
}

impl Column for ColumnUUID {
    column_base!(parent.base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }

    /// C++ `void ColumnUUID::measure (Task&, unsigned int& minimum, unsigned
    /// int& maximum)`: 36 for `long`, 8 for `short`.
    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        ColumnUUID::measure_uuid(self, task, env)
    }

    /// C++ `void ColumnUUID::render (std::vector<std::string>& lines, Task&
    /// task, int width, Color& color)`.
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        ColumnUUID::render_uuid(self, lines, task, width, color, env)
    }
}

impl ColumnUUID {
    /// C++ `ColumnUUID::measure` for the most-derived column `this` (its
    /// `_style`), so `ColumnUDAUUID` can delegate to it.
    pub fn measure_uuid(
        this: &dyn Column,
        _task: &Task,
        _env: &mut dyn RenderEnv,
    ) -> Result<(usize, usize)> {
        // Mandatory attribute, no need to check the value.
        let style = this.style();
        if style == "default" || style == "long" {
            Ok((36, 36))
        } else if style == "short" {
            Ok((8, 8))
        } else {
            Ok((0, 0))
        }
    }

    /// C++ `ColumnUUID::render` for the most-derived column `this` (its
    /// `_name` and `_style`). The `short` cut is C++ `substr (0, 8)` in
    /// bytes (lossy should it split a character, which a UUID never does).
    pub fn render_uuid(
        this: &dyn Column,
        lines: &mut Vec<String>,
        task: &Task,
        width: usize,
        color: &Color,
        _env: &mut dyn RenderEnv,
    ) -> Result<()> {
        // No need to check the presence of UUID - all tasks have one.
        // f30cb9c3-3fc0-483f-bfb2-3bf134f00694  default
        // f30cb9c3                              short
        let style = this.style();
        if style == "default" || style == "long" {
            render_string_left(lines, width, color, &task.get(this.name()));
        } else if style == "short" {
            let value = task.get(this.name());
            let bytes = value.as_bytes();
            render_string_left(
                lines,
                width,
                color,
                &String::from_utf8_lossy(&bytes[..bytes.len().min(8)]),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};
    use crate::shared::configuration::Configuration;

    const UUID: &str = "f30cb9c3-3fc0-483f-bfb2-3bf134f00694";

    fn run(style: &str, width: usize) -> ((usize, usize), Vec<String>) {
        let column = factory(&format!("uuid.{style}"), "list", &Configuration::new()).unwrap();
        let mut env = FakeRenderEnv::with_config(&[]);
        let mut task = Task::new();
        task.set("uuid", UUID);
        let m = column.measure(&mut task, &mut env).unwrap();
        let mut lines = Vec::new();
        column
            .render(&mut lines, &mut task, width, &Color::new(), &mut env)
            .unwrap();
        (m, lines)
    }

    #[test]
    fn long_and_short() {
        assert_eq!(run("long", 36), ((36, 36), vec![UUID.to_string()]));
        assert_eq!(run("default", 38), ((36, 36), vec![format!("{UUID}  ")]));
        assert_eq!(run("short", 10), ((8, 8), vec!["f30cb9c3  ".to_string()]));
    }
}
