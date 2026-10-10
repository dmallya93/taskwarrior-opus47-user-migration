//! Rust translation of `src/columns/ColUntil.cpp`.
//!
//! Constructor metadata; `validate` and
//! `modify` are inherited from `ColumnTypeDate`. `measure` /
//! `render` are inherited from `ColumnTypeDate` (C++ `ColUntil.h` declares
//! neither) and delegate to its helpers.

use crate::columns::col_type_date::ColumnTypeDate;
use crate::columns::column::{column_base, Column, ModifyEnv, RenderEnv};
use crate::error::Result;
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::task::Task;

/// C++ `class ColumnUntil : public ColumnTypeDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUntil {
    /// The inherited `ColumnTypeDate`.
    pub parent: ColumnTypeDate,
}

impl ColumnUntil {
    /// C++ `ColumnUntil::ColumnUntil ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeDate::new(config);
        parent.base.name = "until".to_string();
        parent.base.label = "Until".to_string();
        ColumnUntil { parent }
    }
}

impl Column for ColumnUntil {
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

#[cfg(test)]
mod tests {
    //! Labels and the `ColumnTypeDate` delegation of `measure` / `render`
    //! (every style is covered in `col_type_date`).
    use super::*;
    use crate::columns::column::{factory, FakeRenderEnv};
    use crate::shared::localtime;

    #[test]
    fn labels_and_delegation() {
        let mut env = FakeRenderEnv::with_config(&[("dateformat", "Y-M-D")]);
        assert_eq!(factory("until", "list", &env.config).unwrap().label(), "Until");
        assert_eq!(factory("until.countdown", "list", &env.config).unwrap().label(), "Until");

        let mut task = Task::new();
        task.set_int("until", 1_700_000_000);
        let c = factory("until.epoch", "list", &env.config).unwrap();
        assert_eq!(c.measure(&mut task, &mut env).unwrap(), (10, 10));
        let mut lines = Vec::new();
        c.render(&mut lines, &mut task, 12, &Color::new(), &mut env).unwrap();
        assert_eq!(lines, vec!["  1700000000".to_string()]);

        // `remaining` renders nothing for a past date.
        let c = factory("until.remaining", "list", &env.config).unwrap();
        task.set_int("until", localtime::time_now() - 86400);
        assert_eq!(c.measure(&mut task, &mut env).unwrap(), (0, 0));
        let mut lines = Vec::new();
        c.render(&mut lines, &mut task, 12, &Color::new(), &mut env).unwrap();
        assert!(lines.is_empty());

        // No such date: nothing.
        let mut empty = Task::new();
        assert_eq!(c.measure(&mut empty, &mut env).unwrap(), (0, 0));
    }
}
