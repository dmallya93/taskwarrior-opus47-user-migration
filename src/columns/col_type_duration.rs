//! Rust translation of `src/columns/ColTypeDuration.cpp`.
//!
//! `ColumnTypeDuration`: the base of the duration UDA columns. C++ declares no
//! `measure` / `render` of its own, so it keeps the no-op [`Column`]
//! defaults (`measure` -> `(0, 0)`, `render` -> no lines).

use crate::columns::column::{column_base, Column, ColumnBase, ModifyEnv, MODIFICATION_LABEL};
use crate::error::{Result, TwError};
use crate::shared::format::format;
use crate::task::Task;
use crate::variant::{self, Variant};

/// C++ `class ColumnTypeDuration : public Column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTypeDuration {
    /// The inherited `Column` members.
    pub base: ColumnBase,
}

impl Default for ColumnTypeDuration {
    fn default() -> Self {
        ColumnTypeDuration::new()
    }
}

impl ColumnTypeDuration {
    /// C++ `ColumnTypeDuration::ColumnTypeDuration () { _type = "duration"; }`.
    pub fn new() -> Self {
        let mut base = ColumnBase::new();
        base.type_ = "duration".to_string();
        ColumnTypeDuration { base }
    }

    /// C++ `bool ColumnTypeDuration::validate (const std::string& input)
    /// const`: any non-empty value.
    pub fn validate_duration(input: &str) -> bool {
        !input.is_empty()
    }

    /// C++ `void ColumnTypeDuration::modify (Task& task, const std::string&
    /// value)` for the column `this`: the evaluated value must be a
    /// duration, stored in its `Variant` string form (ISO 8601).
    ///
    /// # Errors
    /// `The duration value '{1}' is not supported.`
    pub fn modify_duration(
        this: &dyn Column,
        task: &mut Task,
        value: &str,
        env: &mut dyn ModifyEnv,
    ) -> Result<()> {
        let name = this.name();

        // Try to evaluate 'value'.  It might work.
        let evaluated_value = match env.evaluate_dom_expression(value) {
            Ok(v) => v,
            Err(_) => Variant::from_string(value),
        };

        // The duration is first parsed, then stored inside the variant as a `time_t`
        let label = MODIFICATION_LABEL;
        if evaluated_value.type_() == variant::Type::Duration {
            env.debug(&format!(
                "{label}{name} <-- {evaluated_value} <-- '{value}'"
            ));
            task.set(name, &evaluated_value.to_string());
        } else {
            return Err(TwError::Message(format(
                "The duration value '{1}' is not supported.",
                &[&value],
            )));
        }

        Ok(())
    }
}

impl Column for ColumnTypeDuration {
    column_base!(base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeDuration::validate_duration(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDuration::modify_duration(self, task, value, env)
    }
}

#[cfg(test)]
mod tests {
    //! C++ `ColumnTypeDuration::validate` / `modify` on the bare type column.
    use super::*;
    use crate::columns::column::FakeModifyEnv;

    fn column() -> ColumnTypeDuration {
        let mut c = ColumnTypeDuration::new();
        c.base.name = "len".to_string();
        c
    }

    fn modified(value: &str, env: &mut FakeModifyEnv) -> Result<String> {
        let mut task = Task::new();
        ColumnTypeDuration::modify_duration(&column(), &mut task, value, env)?;
        Ok(task.get("len"))
    }

    #[test]
    fn constructor_and_validate() {
        let c = ColumnTypeDuration::new();
        assert_eq!(c.type_(), "duration");
        assert!(c.modifiable());
        assert!(
            ColumnTypeDuration::validate_duration("1d")
                && ColumnTypeDuration::validate_duration("x")
        );
        assert!(!ColumnTypeDuration::validate_duration(""));
        assert!(c.validate("1d") && !c.validate(""));
    }

    #[test]
    fn modify_stores_normalised_durations() {
        let mut env = FakeModifyEnv::default();
        assert_eq!(modified("1w", &mut env).unwrap(), "P7D");
        assert_eq!(modified("PT1H", &mut env).unwrap(), "PT1H");
        assert_eq!(modified("90min", &mut env).unwrap(), "PT1H30M");
        assert_eq!(
            env.debug,
            vec![
                format!("{MODIFICATION_LABEL}len <-- P7D <-- '1w'"),
                format!("{MODIFICATION_LABEL}len <-- PT1H <-- 'PT1H'"),
                format!("{MODIFICATION_LABEL}len <-- PT1H30M <-- '90min'"),
            ]
        );
    }

    #[test]
    fn modify_rejects_non_durations() {
        let mut env = FakeModifyEnv::default();
        for value in ["5", "abc", "", "2030-01-01", "("] {
            assert_eq!(
                modified(value, &mut env).unwrap_err().to_string(),
                format!("The duration value '{value}' is not supported."),
                "{value}"
            );
        }
        assert!(env.debug.is_empty());
        // libshared Duration: a bare number of at least 60 is seconds.
        assert_eq!(modified("3600", &mut env).unwrap(), "PT1H");
    }
}
