//! Rust translation of `src/columns/ColDue.cpp`.
//!
//! Constructor metadata and the label-linking `setStyle`; `validate` and
//! `modify` are inherited from `ColumnTypeDate`. Rendering is inherited
//! from `ColumnTypeDate` and arrives with milestone 6.

use crate::columns::col_type_date::ColumnTypeDate;
use crate::columns::column::{column_base, Column, ModifyEnv};
use crate::error::Result;
use crate::shared::configuration::Configuration;
use crate::task::Task;

/// C++ `class ColumnDue : public ColumnTypeDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDue {
    /// The inherited `ColumnTypeDate`.
    pub parent: ColumnTypeDate,
}

impl ColumnDue {
    /// C++ `ColumnDue::ColumnDue ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeDate::new(config);
        parent.base.name = "due".to_string();
        parent.base.modifiable = true;
        parent.base.label = "Due".to_string();
        ColumnDue { parent }
    }
}

impl Column for ColumnDue {
    column_base!(parent.base);

    /// C++ `void ColumnDue::setStyle (const std::string& value)`: overridden so
    /// that style <----> label are linked.
    fn set_style(&mut self, value: &str, _config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "countdown" && base.label == "Due" {
            base.label = "Count".to_string();
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeDate::validate_date(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDate::modify_date(self, task, value, env)
    }
}
