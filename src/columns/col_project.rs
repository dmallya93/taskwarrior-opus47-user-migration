//! Rust translation of `src/columns/ColProject.cpp`.
//!
//! Constructor metadata and `modify`; `validate` is inherited from
//! `ColumnTypeString`. The `measure` / `render` overrides arrive with
//! milestone 6.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, strings, Column, ModifyEnv, MODIFICATION_LABEL};
use crate::error::{Result, TwError};
use crate::lexer::{Lexer, Type};
use crate::shared::configuration::Configuration;
use crate::task::Task;

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
        ColumnProject { parent, hyphenate: config.get_boolean("hyphenate") }
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
}
