//! Rust translation of `src/columns/ColUrgency.cpp`.
//!
//! Constructor metadata; `validate` and
//! `modify` are inherited from `ColumnTypeNumeric`. The `measure` / `render`
//! overrides arrive with milestone 6.

use crate::columns::col_type_numeric::ColumnTypeNumeric;
use crate::columns::column::{column_base, strings, Column, ModifyEnv};
use crate::error::Result;
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
}
