//! Rust translation of `src/columns/ColEntry.cpp`.
//!
//! Constructor metadata and the label-linking `setStyle`; `validate` and
//! `modify` are inherited from `ColumnTypeDate`. `measure` /
//! `render` are inherited from `ColumnTypeDate` (C++ `ColEntry.h` declares
//! neither) and delegate to its helpers.

use crate::columns::col_type_date::ColumnTypeDate;
use crate::columns::column::{column_base, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::task::Task;

/// C++ `class ColumnEntry : public ColumnTypeDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnEntry {
    /// The inherited `ColumnTypeDate`.
    pub parent: ColumnTypeDate,
}

impl ColumnEntry {
    /// C++ `ColumnEntry::ColumnEntry ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeDate::new(config);
        parent.base.name = "entry".to_string();
        parent.base.modifiable = true;
        parent.base.label = "Added".to_string();
        ColumnEntry { parent }
    }
}

impl Column for ColumnEntry {
    column_base!(parent.base);

    /// C++ `void ColumnEntry::setStyle (const std::string& value)`: overridden so
    /// that style <----> label are linked.
    fn set_style(&mut self, value: &str, _config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "age" && base.label == "Added" {
            base.label = "Age".to_string();
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeDate::validate_date(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDate::modify_date(self, task, value, env)
    }

    fn measure(&self, task: &mut Task, env: &mut dyn RenderEnv) -> Result<(usize, usize)> {
        Ok(ColumnTypeDate::measure_date(self, task, env))
    }

    fn render(
        &self,
        lines: &mut Vec<String>,
        task: &mut Task,
        width: usize,
        color: &Color,
        env: &mut dyn RenderEnv,
    ) -> Result<()> {
        ColumnTypeDate::render_date(self, lines, task, width, color, env);
        Ok(())
    }
}
