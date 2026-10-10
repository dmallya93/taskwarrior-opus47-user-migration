//! Rust translation of `src/columns/ColStatus.cpp`.
//!
//! Constructor metadata and the label-linking `setStyle`; `validate` and
//! `modify` are inherited from `ColumnTypeString`. The `measure` / `render`
//! overrides arrive with milestone 6.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, strings, Column, ModifyEnv};
use crate::error::Result;
use crate::shared::configuration::Configuration;
use crate::task::Task;

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
}
