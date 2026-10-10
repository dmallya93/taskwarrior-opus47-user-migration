//! Rust translation of `src/columns/ColTemplate.cpp`.
//!
//! Constructor metadata and `measure` / `render` (the `long` and `short`
//! UUID styles); `validate` and `modify` are inherited from
//! `ColumnTypeString`.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, render_string_left, strings, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::task::Task;

/// C++ `class ColumnTemplate : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTemplate {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
}

impl Default for ColumnTemplate {
    fn default() -> Self {
        ColumnTemplate::new()
    }
}

impl ColumnTemplate {
    /// C++ `ColumnTemplate::ColumnTemplate ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "template".to_string();
        parent.base.style = "long".to_string();
        parent.base.label = "Template task".to_string();
        parent.base.modifiable = false;
        parent.base.styles = strings(&["long", "short"]);
        parent.base.examples = strings(&["f30cb9c3-3fc0-483f-bfb2-3bf134f00694", "f30cb9c3"]);
        ColumnTemplate { parent }
    }
}

/// C++ `value.substr (0, 8)` (byte-based, clamped).
///
/// Deviation: a cut inside a multi-byte character becomes U+FFFD; C++ keeps
/// the partial bytes. UUIDs are ASCII, so this never happens in practice.
fn substr8(value: &str) -> String {
    let bytes = value.as_bytes();
    String::from_utf8_lossy(&bytes[..bytes.len().min(8)]).into_owned()
}

impl Column for ColumnTemplate {
    column_base!(parent.base);

    /// C++ `void ColumnTemplate::measure (Task& task, unsigned int& minimum, unsigned
    /// int& maximum)`: 36 (`long`) or 8 (`short`), whatever the value;
    /// `(0, 0)` when the task has no `template`.
    fn measure(&self, task: &mut Task, _env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0;
        let style = self.style();
        if task.has(self.name()) {
            if style == "default" || style == "long" {
                minimum = 36;
            } else if style == "short" {
                minimum = 8;
            }
        }
        Ok((minimum, minimum))
    }

    /// C++ `void ColumnTemplate::render (std::vector<std::string>& lines, Task& task,
    /// int width, Color& color)`: the full UUID (`long`) or its first 8
    /// bytes (`short`), left-justified.
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
            // f30cb9c3-3fc0-483f-bfb2-3bf134f00694  default
            // f30cb9c3                              short
            if style == "default" || style == "long" {
                render_string_left(lines, width, color, task.get_ref(name));
            } else if style == "short" {
                render_string_left(lines, width, color, &substr8(task.get_ref(name)));
            }
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
    //! `measure` / `render` of every `template` style against a
    //! [`FakeRenderEnv`].
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};

    const UUID: &str = "f30cb9c3-3fc0-483f-bfb2-3bf134f00694";

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
    fn long_style() {
        let mut env = FakeRenderEnv::with_config(&[]);
        let none = Color::new();
        assert_eq!(run("template", &mut with("template", UUID), 36, &none, &mut env), ((36, 36), vec![UUID.to_string()]));
        assert_eq!(
            run("template.long", &mut with("template", UUID), 38, &none, &mut env),
            ((36, 36), vec![format!("{UUID}  ")])
        );
        // The width is fixed, whatever the value.
        assert_eq!(run("template.long", &mut with("template", "abc"), 5, &none, &mut env), ((36, 36), vec!["abc  ".to_string()]));
        let red = Color::parse("red").unwrap();
        assert_eq!(
            run("template", &mut with("template", UUID), 36, &red, &mut env).1,
            vec![format!("\x1b[31m{UUID}\x1b[0m")]
        );
    }

    #[test]
    fn short_style() {
        let mut env = FakeRenderEnv::with_config(&[]);
        let none = Color::new();
        assert_eq!(
            run("template.short", &mut with("template", UUID), 8, &none, &mut env),
            ((8, 8), vec!["f30cb9c3".to_string()])
        );
        assert_eq!(
            run("template.short", &mut with("template", UUID), 10, &none, &mut env),
            ((8, 8), vec!["f30cb9c3  ".to_string()])
        );
        assert_eq!(run("template.short", &mut with("template", "abc"), 8, &none, &mut env), ((8, 8), vec!["abc     ".to_string()]));
    }

    #[test]
    fn no_value() {
        let mut env = FakeRenderEnv::with_config(&[]);
        for style in ["long", "short"] {
            assert_eq!(run(&format!("template.{style}"), &mut Task::new(), 8, &Color::new(), &mut env), ((0, 0), vec![]));
        }
    }

    #[test]
    fn substr8_clamps() {
        assert_eq!(substr8(UUID), "f30cb9c3");
        assert_eq!(substr8("abc"), "abc");
        assert_eq!(substr8(""), "");
    }
}
