//! Rust translation of `src/columns/ColStart.cpp`.
//!
//! Constructor metadata (adds the `active` style) and the label-linking
//! `setStyle`; `validate` and `modify` are inherited from `ColumnTypeDate`.
//! The `measure` / `render` overrides arrive with milestone 6.

use crate::columns::col_type_date::ColumnTypeDate;
use crate::columns::column::{column_base, Column, ModifyEnv};
use crate::error::Result;
use crate::shared::configuration::Configuration;
use crate::task::Task;

/// C++ `class ColumnStart : public ColumnTypeDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnStart {
    /// The inherited `ColumnTypeDate`.
    pub parent: ColumnTypeDate,
}

impl ColumnStart {
    /// C++ `ColumnStart::ColumnStart ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeDate::new(config);
        parent.base.name = "start".to_string();
        parent.base.label = "Started".to_string();
        parent.base.styles.push("active".to_string());
        parent.base.examples.push(config.get("active.indicator"));
        ColumnStart { parent }
    }
}

impl Column for ColumnStart {
    column_base!(parent.base);

    /// C++ `void ColumnStart::setStyle (const std::string& value)`:
    /// overridden so that style <----> label are linked.
    fn set_style(&mut self, value: &str, _config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "active" && base.label == "Started" {
            base.label = "A".to_string();
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
