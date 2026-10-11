//! Rust translation of `src/columns/ColTypeNumeric.cpp`.
//!
//! `ColumnTypeNumeric`: the base of every numeric column. C++ declares no
//! `measure` / `render` of its own, so it keeps the no-op [`Column`]
//! defaults (`measure` -> `(0, 0)`, `render` -> no lines).

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
    pub fn modify_numeric(
        this: &dyn Column,
        task: &mut Task,
        value: &str,
        env: &mut dyn ModifyEnv,
    ) -> Result<()> {
        let name = this.name();

        // Try to evaluate 'value'.  It might work.
        let mut evaluated_value = match env.evaluate_dom_expression(value) {
            Ok(v) => v,
            Err(_) => Variant::from_string(value),
        };

        let label = MODIFICATION_LABEL;
        env.debug(&format!(
            "{label}{name} <-- '{}' <-- '{value}'",
            evaluated_value.get_string()
        ));

        // Convert the value of the expression to the correct type if needed
        match evaluated_value.type_() {
            // Expected variants - no conversion
            variant::Type::Integer | variant::Type::Real => {}
            // Convertible variants - convert to int
            variant::Type::Date | variant::Type::Duration => {
                evaluated_value.cast(variant::Type::Integer)?
            }
            // Non-convertible variants
            variant::Type::String => {
                return Err(TwError::Message(format(
                    "The value '{1}' is not a valid numeric value.",
                    &[&evaluated_value.get_string()],
                )));
            }
            other => {
                return Err(TwError::Message(format(
                    "Unexpected variant type: '{1}'",
                    &[&(other as i32)],
                )));
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

#[cfg(test)]
mod tests {
    //! C++ `ColumnTypeNumeric::validate` / `modify` on the bare type column.
    use super::*;
    use crate::columns::column::FakeModifyEnv;
    use crate::shared::datetime::Datetime;

    fn column() -> ColumnTypeNumeric {
        let mut c = ColumnTypeNumeric::new();
        c.base.name = "n".to_string();
        c
    }

    fn modified(value: &str) -> Result<String> {
        let mut task = Task::new();
        ColumnTypeNumeric::modify_numeric(
            &column(),
            &mut task,
            value,
            &mut FakeModifyEnv::default(),
        )?;
        Ok(task.get("n"))
    }

    #[test]
    fn constructor_and_validate() {
        let c = ColumnTypeNumeric::new();
        assert_eq!(c.type_(), "numeric");
        assert!(c.modifiable());
        assert!(
            ColumnTypeNumeric::validate_numeric("1") && ColumnTypeNumeric::validate_numeric("x")
        );
        assert!(!ColumnTypeNumeric::validate_numeric(""));
        assert!(c.validate("1") && !c.validate(""));
    }

    #[test]
    fn modify_integer_real_and_expressions() {
        assert_eq!(modified("42").unwrap(), "42");
        assert_eq!(modified("-7").unwrap(), "-7");
        assert_eq!(modified("3.25").unwrap(), "3.25");
        assert_eq!(modified("6*7").unwrap(), "42");
        let mut env = FakeModifyEnv::default();
        let mut task = Task::new();
        column().modify(&mut task, "1+2", &mut env).unwrap();
        assert_eq!(task.get("n"), "3");
        // C++ `get_string ()` is the raw `_string`, which Eval leaves at the
        // left operand's literal text.
        assert_eq!(
            env.debug,
            vec![format!("{MODIFICATION_LABEL}n <-- '1' <-- '1+2'")]
        );
    }

    #[test]
    fn modify_casts_dates_and_durations_to_integer() {
        assert_eq!(modified("1h").unwrap(), "3600");
        assert_eq!(modified("P1D").unwrap(), "86400");
        let epoch = Datetime::from_string("2030-01-01", "").unwrap().to_epoch();
        assert_eq!(modified("2030-01-01").unwrap(), epoch.to_string());
    }

    #[test]
    fn modify_rejects_strings() {
        assert_eq!(
            modified("abc").unwrap_err().to_string(),
            "The value 'abc' is not a valid numeric value."
        );
        // Eval failure falls back to the literal string.
        assert_eq!(
            modified("(").unwrap_err().to_string(),
            "The value '(' is not a valid numeric value."
        );
        assert_eq!(
            modified("").unwrap_err().to_string(),
            "The value '' is not a valid numeric value."
        );
    }
}
