//! Rust translation of `src/columns/ColTypeDate.cpp`.
//!
//! `ColumnTypeDate`: the base of every date column (constructor metadata,
//! `validate` and `modify`). `measure` / `render` arrive with milestone 6.

use crate::columns::column::{column_base, strings, Column, ColumnBase, ModifyEnv, MODIFICATION_LABEL};
use crate::error::{Result, TwError};
use crate::shared::configuration::Configuration;
use crate::shared::datetime::{Datetime, EPOCH_MAX_VALUE, EPOCH_MIN_VALUE};
use crate::shared::duration::Duration;
use crate::shared::format::{format, format_double, format_value};
use crate::task::Task;
use crate::variant::{self, Variant};

/// C++ `class ColumnTypeDate : public Column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTypeDate {
    /// The inherited `Column` members.
    pub base: ColumnBase,
}

impl ColumnTypeDate {
    /// C++ `ColumnTypeDate::ColumnTypeDate ()`. The examples are built from
    /// "now - 125 s" and `rc.dateformat`.
    pub fn new(config: &Configuration) -> Self {
        let mut base = ColumnBase::new();
        base.name = String::new();
        base.type_ = "date".to_string();
        base.style = "formatted".to_string();
        base.label = String::new();
        base.styles = strings(&["formatted", "julian", "epoch", "iso", "age", "relative", "remaining", "countdown"]);

        let mut now = Datetime::new();
        now -= 125; // So that "age" is non-zero.
        base.examples = vec![
            now.to_string(&config.get("dateformat")),
            format_double(now.to_julian(), 13, 12),
            now.to_epoch_string(),
            now.to_iso(),
            Duration::from_seconds(Datetime::new() - now).format_vague(true),
            format!("-{}", Duration::from_seconds(Datetime::new() - now).format_vague(true)),
            String::new(),
            Duration::from_seconds(Datetime::new() - now).format(),
        ];

        ColumnTypeDate { base }
    }

    /// C++ `bool ColumnTypeDate::validate (const std::string& input) const`:
    /// any non-empty value.
    pub fn validate_date(input: &str) -> bool {
        !input.is_empty()
    }

    /// C++ `void ColumnTypeDate::modify (Task& task, const std::string&
    /// value)` for the column `this`: evaluates `value` (falling back to the
    /// literal string when evaluation fails), casts the result to a date
    /// (a duration is added to now) and stores the epoch.
    ///
    /// # Errors
    /// The `Variant::cast` error, `'{1}' is not a valid date in the '{2}'
    /// format.` (with `Variant::dateFormat`) when the date is 0, and `'{1}'
    /// is not a valid date.` outside `[EPOCH_MIN_VALUE, EPOCH_MAX_VALUE)`.
    pub fn modify_date(this: &dyn Column, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        let name = this.name();

        // Try to evaluate 'value'.  It might work.
        let mut evaluated_value = match env.evaluate_dom_expression(value) {
            Ok(v) => v,
            Err(_) => Variant::from_string(value),
        };

        // If v is duration, we need to convert it to date (and implicitly add now),
        // else store as date.
        let label = MODIFICATION_LABEL;
        if evaluated_value.type_() == variant::Type::Duration {
            env.debug(&format!(
                "{label}{name} <-- '{}' <-- '{evaluated_value}' <-- '{value}'",
                format("{1}", &[&format_value(&evaluated_value.get_duration())])
            ));
            evaluated_value.cast(variant::Type::Date)?;
        } else {
            evaluated_value.cast(variant::Type::Date)?;
            env.debug(&format!(
                "{label}{name} <-- '{}' <-- '{evaluated_value}' <-- '{value}'",
                format("{1}", &[&evaluated_value.get_date()])
            ));
        }

        // If a date doesn't parse (2/29/2014) then it evaluates to zero.
        if !value.is_empty() && evaluated_value.get_date() == 0 {
            return Err(TwError::Message(format(
                "'{1}' is not a valid date in the '{2}' format.",
                &[&value, &Variant::date_format()],
            )));
        }

        let epoch = evaluated_value.get_date();
        if !(EPOCH_MIN_VALUE..EPOCH_MAX_VALUE).contains(&epoch) {
            return Err(TwError::Message(format("'{1}' is not a valid date.", &[&value])));
        }
        task.set_int(name, epoch);
        Ok(())
    }
}

impl Column for ColumnTypeDate {
    column_base!(base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeDate::validate_date(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDate::modify_date(self, task, value, env)
    }
}
