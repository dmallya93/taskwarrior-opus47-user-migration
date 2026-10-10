//! Rust translation of `src/columns/ColWait.cpp`.
//!
//! Constructor metadata; `validate` and
//! `modify` are inherited from `ColumnTypeDate`. `measure` /
//! `render` are inherited from `ColumnTypeDate` (C++ `ColWait.h` declares
//! neither) and delegate to its helpers.

use crate::columns::col_type_date::ColumnTypeDate;
use crate::columns::column::{column_base, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::task::Task;

/// C++ `class ColumnWait : public ColumnTypeDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnWait {
    /// The inherited `ColumnTypeDate`.
    pub parent: ColumnTypeDate,
}

impl ColumnWait {
    /// C++ `ColumnWait::ColumnWait ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeDate::new(config);
        parent.base.name = "wait".to_string();
        parent.base.label = "Wait".to_string();
        ColumnWait { parent }
    }
}

impl Column for ColumnWait {
    column_base!(parent.base);

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
