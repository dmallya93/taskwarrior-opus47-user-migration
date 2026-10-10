//! Rust translation of `src/columns/ColRType.cpp`.
//!
//! Constructor metadata, the label-linking `setStyle` and `validate`;
//! `modify` is inherited from `ColumnTypeString` (the column is not
//! modifiable, so `Task::modify` never calls it). The `measure` / `render`
//! overrides arrive with milestone 6.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, label_prefix, strings, Column, ModifyEnv};
use crate::error::Result;
use crate::shared::configuration::Configuration;
use crate::task::Task;

/// C++ `class ColumnRType : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRType {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
}

impl Default for ColumnRType {
    fn default() -> Self {
        ColumnRType::new()
    }
}

impl ColumnRType {
    /// C++ `ColumnRType::ColumnRType ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "rtype".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = "Recurrence type".to_string();
        parent.base.modifiable = false;
        parent.base.styles = strings(&["default", "indicator"]);
        parent.base.examples = strings(&["periodic", "chained"]);
        ColumnRType { parent }
    }
}

impl Column for ColumnRType {
    column_base!(parent.base);

    /// C++ `void ColumnRType::setStyle (const std::string& value)`:
    /// overridden so that style <----> label are linked (cut to the width of
    /// `rc.rtype.indicator`).
    fn set_style(&mut self, value: &str, config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "indicator" && base.label == "Recurrence type" {
            base.label = label_prefix(&base.label, config.get("rtype.indicator").len());
        }
        Ok(())
    }

    /// C++ `bool ColumnRType::validate (const std::string& input) const`:
    /// `periodic` or `chained`.
    fn validate(&self, input: &str) -> bool {
        input == "periodic" || input == "chained"
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }
}
