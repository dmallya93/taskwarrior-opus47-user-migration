//! Rust translation of `src/columns/ColID.cpp`.
//!
//! Constructor metadata; `validate` and
//! `modify` are inherited from `ColumnTypeNumeric`. The `measure` / `render`
//! overrides arrive with milestone 6.

use crate::columns::col_type_numeric::ColumnTypeNumeric;
use crate::columns::column::{column_base, strings, Column, ModifyEnv};
use crate::error::Result;
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
}
