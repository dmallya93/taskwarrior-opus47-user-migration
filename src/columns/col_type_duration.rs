//! Rust translation of `src/columns/ColTypeDuration.cpp`.
//!
//! `ColumnTypeDuration`: the base of the duration UDA columns. Rendering
//! (none of its own in C++) arrives with milestone 6.

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
    pub fn modify_duration(this: &dyn Column, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        let name = this.name();

        // Try to evaluate 'value'.  It might work.
        let evaluated_value = match env.evaluate_dom_expression(value) {
            Ok(v) => v,
            Err(_) => Variant::from_string(value),
        };

        // The duration is first parsed, then stored inside the variant as a `time_t`
        let label = MODIFICATION_LABEL;
        if evaluated_value.type_() == variant::Type::Duration {
            env.debug(&format!("{label}{name} <-- {evaluated_value} <-- '{value}'"));
            task.set(name, &evaluated_value.to_string());
        } else {
            return Err(TwError::Message(format("The duration value '{1}' is not supported.", &[&value])));
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
