//! Rust translation of `src/columns/ColMask.cpp`.
//!
//! Constructor metadata; `validate` and
//! `modify` are inherited from `ColumnTypeString`. The `measure` / `render`
//! overrides arrive with milestone 6.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, strings, Column, ModifyEnv};
use crate::error::Result;
use crate::task::Task;

/// C++ `class ColumnMask : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMask {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
}

impl Default for ColumnMask {
    fn default() -> Self {
        ColumnMask::new()
    }
}

impl ColumnMask {
    /// C++ `ColumnMask::ColumnMask ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "mask".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = "Mask".to_string();
        parent.base.modifiable = false;
        parent.base.styles = strings(&["default"]);
        parent.base.examples = strings(&["++++---"]);
        ColumnMask { parent }
    }
}

impl Column for ColumnMask {
    column_base!(parent.base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }
}
