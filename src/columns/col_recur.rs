//! Rust translation of `src/columns/ColRecur.cpp`.
//!
//! Constructor metadata, the label-linking `setStyle` and `modify`;
//! `validate` is inherited from `ColumnTypeString`. The `measure` /
//! `render` overrides arrive with milestone 6.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, label_prefix, Column, ModifyEnv, MODIFICATION_LABEL};
use crate::error::{Result, TwError};
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::task::Task;
use crate::variant::{self, Variant};

/// C++ `class ColumnRecur : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRecur {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
}

impl ColumnRecur {
    /// C++ `ColumnRecur::ColumnRecur ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "recur".to_string();
        parent.base.style = "duration".to_string();
        parent.base.label = "Recur".to_string();
        parent.base.modifiable = true;
        parent.base.styles = vec!["duration".to_string(), "indicator".to_string()];
        parent.base.examples = vec!["weekly".to_string(), config.get("recurrence.indicator")];
        ColumnRecur { parent }
    }
}

impl Column for ColumnRecur {
    column_base!(parent.base);

    /// C++ `void ColumnRecur::setStyle (const std::string& value)`:
    /// overridden so that style <----> label are linked (cut to the width of
    /// `rc.recurrence.indicator`).
    fn set_style(&mut self, value: &str, config: &Configuration) -> Result<()> {
        self.base_mut().set_style(value)?;
        let base = self.base_mut();
        if base.style == "indicator" && base.label == "Recur" {
            base.label = label_prefix(&base.label, config.get("recurrence.indicator").len());
        }
        Ok(())
    }

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    /// C++ `void ColumnRecur::modify (Task& task, const std::string& value)`:
    /// the duration is stored in raw form, but it must still be valid, and
    /// therefore is parsed (evaluated) first.
    ///
    /// # Errors
    /// `The duration value '{1}' is not supported.`
    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        let name = self.name();

        // Try to evaluate 'value'.  It might work.
        let evaluated_value = match env.evaluate_dom_expression(value) {
            Ok(v) => v,
            Err(_) => Variant::from_string(value),
        };

        if evaluated_value.type_() == variant::Type::Duration {
            // Store the raw value, for 'recur'.
            let label = MODIFICATION_LABEL;
            env.debug(&format!("{label}{name} <-- '{value}'"));
            task.set(name, value);
        } else {
            return Err(TwError::Message(format("The duration value '{1}' is not supported.", &[&value])));
        }

        Ok(())
    }
}
