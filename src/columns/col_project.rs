//! Rust translation of `src/columns/ColProject.cpp`.
//!
//! Constructor metadata, `modify` and the `measure` / `render` overrides
//! (`full` / `parent` / `indented`); `validate` is inherited from
//! `ColumnTypeString`.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{
    column_base, render_string_left, strings, Column, ModifyEnv, RenderEnv, MODIFICATION_LABEL,
};
use crate::error::{Result, TwError};
use crate::lexer::{Lexer, Type};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::shared::{longest_word, wrap_text};
use crate::shared::utf8::utf8_width;
use crate::task::Task;
use crate::util::indent_project;

/// C++ `class ColumnProject : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnProject {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
    /// C++ `_hyphenate` (`rc.hyphenate`).
    pub hyphenate: bool,
}

impl ColumnProject {
    /// C++ `ColumnProject::ColumnProject ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "project".to_string();
        parent.base.style = "full".to_string();
        parent.base.label = "Project".to_string();
        parent.base.styles = strings(&["full", "parent", "indented"]);
        parent.base.examples = strings(&["home.garden", "home", "  home.garden"]);
        ColumnProject {
            parent,
            hyphenate: config.get_boolean("hyphenate"),
        }
    }
}

impl ColumnProject {
    /// The project as `measure` / `render` show it: the part before the
    /// first `.` for `parent`, `indentProject (project, "  ", '.')` for
    /// `indented`, otherwise unchanged.
    fn styled(&self, mut project: String) -> String {
        let style = self.style();
        if style == "parent" {
            if let Some(period) = project.find('.') {
                project.truncate(period);
            }
        } else if style == "indented" {
            project = indent_project(&project, "  ", '.');
        }
        project
    }
}

impl Column for ColumnProject {
    column_base!(parent.base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    /// C++ `void ColumnProject::modify (Task& task, const std::string&
    /// value)`: a value starting with a DOM reference is evaluated; when
    /// evaluation fails with `The value is not an expression.` (or the value
    /// is not a DOM reference) it is stored as given. No validation.
    ///
    /// # Errors
    /// Any other Eval error.
    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        let label = MODIFICATION_LABEL;
        let name = self.name();

        // Only if it's a DOM ref, eval it first.
        let mut lexer = Lexer::new(value);
        let mut dom_ref = String::new();
        let mut type_ = Type::word;
        if lexer.token(&mut dom_ref, &mut type_) && type_ == Type::dom {
            match env.evaluate_dom_expression(value) {
                Ok(v) => {
                    task.set(name, &v.to_string());
                    env.debug(&format!("{label}{name} <-- '{v}' <-- '{value}'"));
                }
                // If the expression failed because it didn't look like an expression,
                // simply store it as-is.
                Err(TwError::Message(e)) if e == "The value is not an expression." => {
                    task.set(name, value);
                    env.debug(&format!("{label}{name} <-- '{value}'"));
                }
                Err(e) => return Err(e),
            }
        } else {
            task.set(name, value);
            env.debug(&format!("{label}{name} <-- '{value}'"));
        }

        Ok(())
    }

    /// C++ `void ColumnProject::measure (Task& task, unsigned int& minimum,
    /// unsigned int& maximum)`: the longest word and the display width of
    /// the styled project, `(0, 0)` without one.
    fn measure(&self, task: &mut Task, _env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        let mut minimum = 0;
        let mut maximum = 0;
        if task.has(self.name()) {
            let project = self.styled(task.get(self.name()));
            minimum = longest_word(&project) as usize;
            maximum = utf8_width(&project) as usize;
        }
        Ok((minimum, maximum))
    }

    /// C++ `void ColumnProject::render (std::vector<std::string>& lines,
    /// Task& task, int width, Color& color)`: the styled project wrapped at
    /// `width` (`rc.hyphenate`).
    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        _env: &mut dyn RenderEnv,
    ) -> Result<()> {
        if task.has(self.name()) {
            let project = self.styled(task.get(self.name()));
            let mut raw: Vec<String> = Vec::new();
            wrap_text(&mut raw, &project, width as i32, self.hyphenate);
            for i in &raw {
                render_string_left(lines, width, color, i);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};

    fn run(style: &str, project: &str, width: usize) -> ((usize, usize), Vec<String>) {
        let mut config = Configuration::new();
        config.set("hyphenate", 0); // C++ `set (key, false)` is the int overload.
        let column = factory(&format!("project.{style}"), "list", &config).unwrap();
        let mut env = FakeRenderEnv::with_config(&[]);
        let mut task = Task::new();
        if !project.is_empty() {
            task.set("project", project);
        }
        let m = column.measure(&mut task, &mut env).unwrap();
        let mut lines = Vec::new();
        column
            .render(&mut lines, &mut task, width, &Color::new(), &mut env)
            .unwrap();
        (m, lines)
    }

    #[test]
    fn full() {
        assert_eq!(
            run("full", "home.garden", 13),
            ((11, 11), vec!["home.garden  ".to_string()])
        );
        assert_eq!(
            run("default", "home.garden", 11),
            ((11, 11), vec!["home.garden".to_string()])
        );
        // No project: nothing measured or rendered.
        assert_eq!(run("full", "", 5), ((0, 0), vec![]));
    }

    #[test]
    fn parent() {
        assert_eq!(
            run("parent", "home.garden.shed", 6),
            ((4, 4), vec!["home  ".to_string()])
        );
        assert_eq!(run("parent", "home", 4), ((4, 4), vec!["home".to_string()]));
    }

    #[test]
    fn indented() {
        assert_eq!(
            run("indented", "home.garden", 8),
            ((6, 8), vec!["  garden".to_string()])
        );
        assert_eq!(
            run("indented", "a.b.c", 5),
            ((1, 5), vec!["    c".to_string()])
        );
        assert_eq!(
            run("indented", "home", 4),
            ((4, 4), vec!["home".to_string()])
        );
    }

    #[test]
    fn wraps_at_width() {
        // No spaces: hard break at the width (no hyphen with hyphenate off).
        assert_eq!(
            run("full", "abcdefgh", 4),
            ((8, 8), vec!["abcd".to_string(), "efgh".to_string()])
        );
    }
}
