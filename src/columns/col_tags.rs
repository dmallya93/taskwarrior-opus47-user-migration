//! Rust translation of `src/columns/ColTags.cpp`.
//!
//! Constructor metadata, the label-linking `setStyle` and `modify`;
//! `validate` is inherited from `ColumnTypeString`. The `measure` /
//! `render` overrides arrive with milestone 6.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, label_prefix, strings, Column, ModifyEnv, MODIFICATION_LABEL};
use crate::error::Result;
use crate::lexer::{Lexer, Type};
use crate::shared::configuration::Configuration;
use crate::shared::shared::split;
use crate::task::Task;

/// C++ `class ColumnTags : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTags {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
    /// C++ `_hyphenate` (always `false`).
    pub hyphenate: bool,
}

impl ColumnTags {
    /// C++ `ColumnTags::ColumnTags ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "tags".to_string();
        parent.base.style = "list".to_string();
        parent.base.label = "Tags".to_string();
        parent.base.styles = strings(&["list", "indicator", "count"]);
        parent.base.examples = vec!["home @chore next".to_string(), config.get("tag.indicator"), "[2]".to_string()];
        ColumnTags { parent, hyphenate: false }
    }
}

impl Column for ColumnTags {
    column_base!(parent.base);

    /// C++ `void ColumnTags::setStyle (const std::string& value)`:
    /// overridden so that style <----> label are linked (the `indicator`
    /// label is cut to the width of `rc.tag.indicator`).
    fn set_style(&mut self, value: &str, config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "indicator" && base.label == "Tags" {
            base.label = label_prefix(&base.label, config.get("tag.indicator").len());
        } else if base.style == "count" && base.label == "Tags" {
            base.label = "Tag".to_string();
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    /// C++ `void ColumnTags::modify (Task& task, const std::string& value)`:
    /// replaces the tags with the comma-separated list in `value` (evaluated
    /// first when it starts with a DOM reference), mentioning special tags.
    ///
    /// # Errors
    /// Any Eval error.
    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        let label = MODIFICATION_LABEL;
        let mut tags: Vec<String> = Vec::new();

        // If it's a DOM ref, eval it first.
        let mut lexer = Lexer::new(value);
        let mut dom_ref = String::new();
        let mut type_ = Type::word;
        let commasep = if lexer.token(&mut dom_ref, &mut type_) && type_ == Type::dom {
            env.evaluate_dom_expression(value)?.to_string()
        } else {
            value.to_string()
        };

        for tag in split(&commasep, ',') {
            env.debug(&format!("{label}tags <-- '{tag}'"));
            env.feedback_special_tags(task, &tag);
            tags.push(tag);
        }

        task.set_tags(&tags);
        Ok(())
    }
}
