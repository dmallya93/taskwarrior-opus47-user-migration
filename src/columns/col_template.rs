//! Rust translation of `src/columns/ColTemplate.cpp`.
//!
//! Constructor metadata; `validate` and
//! `modify` are inherited from `ColumnTypeString`. The `measure` / `render`
//! overrides arrive with milestone 6.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, strings, Column, ModifyEnv};
use crate::error::Result;
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

impl Column for ColumnTemplate {
    column_base!(parent.base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }
}
