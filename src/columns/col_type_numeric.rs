//! Rust translation of `src/columns/ColTypeNumeric.cpp`.
//!
//! `ColumnTypeNumeric`: the base of every numeric column. Rendering (none
//! of its own in C++) arrives with milestone 6.

use crate::columns::column::{column_base, Column, ColumnBase, ModifyEnv, MODIFICATION_LABEL};
use crate::error::{Result, TwError};
use crate::shared::format::format;
use crate::task::Task;
use crate::variant::{self, Variant};

/// C++ `class ColumnTypeNumeric : public Column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTypeNumeric {
    /// The inherited `Column` members.
    pub base: ColumnBase,
}

impl Default for ColumnTypeNumeric {
    fn default() -> Self {
        ColumnTypeNumeric::new()
    }
}

impl ColumnTypeNumeric {
    /// C++ `ColumnTypeNumeric::ColumnTypeNumeric () { _type = "numeric"; }`.
    pub fn new() -> Self {
        let mut base = ColumnBase::new();
        base.type_ = "numeric".to_string();
        ColumnTypeNumeric { base }
    }

    /// C++ `bool ColumnTypeNumeric::validate (const std::string& input)
    /// const`: any non-empty value.
    pub fn validate_numeric(input: &str) -> bool {
        !input.is_empty()
    }

    /// C++ `void ColumnTypeNumeric::modify (Task& task, const std::string&
    /// value)` for the column `this`: evaluates `value` (falling back to the
    /// literal string when evaluation fails), converts dates and durations
    /// to integers and stores the number.
    ///
    /// # Errors
    /// `The value '{1}' is not a valid numeric value.` for a string result,
    /// `Unexpected variant type: '{1}'` (the numeric type) for a boolean.
    pub fn modify_numeric(this: &dyn Column, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        let name = this.name();

        // Try to evaluate 'value'.  It might work.
        let mut evaluated_value = match env.evaluate_dom_expression(value) {
            Ok(v) => v,
            Err(_) => Variant::from_string(value),
        };

        let label = MODIFICATION_LABEL;
        env.debug(&format!("{label}{name} <-- '{}' <-- '{value}'", evaluated_value.get_string()));

        // Convert the value of the expression to the correct type if needed
        match evaluated_value.type_() {
            // Expected variants - no conversion
            variant::Type::Integer | variant::Type::Real => {}
            // Convertible variants - convert to int
            variant::Type::Date | variant::Type::Duration => evaluated_value.cast(variant::Type::Integer)?,
            // Non-convertible variants
            variant::Type::String => {
                return Err(TwError::Message(format(
                    "The value '{1}' is not a valid numeric value.",
                    &[&evaluated_value.get_string()],
                )));
            }
            other => {
                return Err(TwError::Message(format("Unexpected variant type: '{1}'", &[&(other as i32)])));
            }
        }

        task.set(name, &evaluated_value.to_string());
        Ok(())
    }
}

impl Column for ColumnTypeNumeric {
    column_base!(base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeNumeric::validate_numeric(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeNumeric::modify_numeric(self, task, value, env)
    }
}
