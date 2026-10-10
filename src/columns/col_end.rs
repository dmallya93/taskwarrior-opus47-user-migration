//! Rust translation of `src/columns/ColEnd.cpp`.
//!
//! Constructor metadata; `validate` and
//! `modify` are inherited from `ColumnTypeDate`. Rendering is inherited
//! from `ColumnTypeDate` and arrives with milestone 6.

use crate::columns::col_type_date::ColumnTypeDate;
use crate::columns::column::{column_base, Column, ModifyEnv};
use crate::error::Result;
use crate::shared::configuration::Configuration;
use crate::task::Task;

/// C++ `class ColumnEnd : public ColumnTypeDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnEnd {
    /// The inherited `ColumnTypeDate`.
    pub parent: ColumnTypeDate,
}

impl ColumnEnd {
    /// C++ `ColumnEnd::ColumnEnd ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeDate::new(config);
        parent.base.name = "end".to_string();
        parent.base.label = "Completed".to_string();
        ColumnEnd { parent }
    }
}

impl Column for ColumnEnd {
    column_base!(parent.base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeDate::validate_date(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDate::modify_date(self, task, value, env)
    }
}
