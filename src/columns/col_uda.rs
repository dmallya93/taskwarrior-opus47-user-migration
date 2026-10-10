//! Rust translation of `src/columns/ColUDA.cpp`.
//!
//! The user-defined attribute columns `ColumnUDAString`,
//! `ColumnUDANumeric`, `ColumnUDADate`, `ColumnUDADuration` and
//! `ColumnUDAUUID`: constructor metadata and `validate` (the allowed
//! `uda.<name>.values`, or a UUID). `modify` is inherited from the type
//! column, which calls back into these `validate` overrides where C++ does
//! (`ColumnTypeString::modify`). The `measure` / `render` overrides arrive
//! with milestone 6. [`crate::columns::column::uda`] sets `_name`,
//! `_label` and `_values`.

use crate::columns::col_type_date::ColumnTypeDate;
use crate::columns::col_type_duration::ColumnTypeDuration;
use crate::columns::col_type_numeric::ColumnTypeNumeric;
use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::col_uuid::ColumnUUID;
use crate::columns::column::{column_base, strings, Column, ModifyEnv};
use crate::error::Result;
use crate::lexer::{Lexer, Type};
use crate::shared::configuration::Configuration;
use crate::task::Task;

/// The `validate` shared by the string, numeric, date and duration UDA
/// columns: no restrictions without values, otherwise an exact match.
fn validate_values(values: &[String], value: &str) -> bool {
    // No restrictions.
    if values.is_empty() {
        return true;
    }

    // Look for exact match value.
    // Fail if not found.
    values.iter().any(|i| i == value)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDAString : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDAString {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
    /// C++ `_values`.
    pub values: Vec<String>,
    /// C++ `_hyphenate` (always `true`).
    pub hyphenate: bool,
}

impl Default for ColumnUDAString {
    fn default() -> Self {
        ColumnUDAString::new()
    }
}

impl ColumnUDAString {
    /// C++ `ColumnUDAString::ColumnUDAString ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "<uda>".to_string(); // Gets overwritten at runtime.
        parent.base.style = "default".to_string();
        parent.base.label = String::new();
        parent.base.modifiable = true;
        parent.base.uda = true;
        parent.base.styles = strings(&["default", "indicator"]);
        ColumnUDAString { parent, values: Vec::new(), hyphenate: true }
    }
}

impl Column for ColumnUDAString {
    column_base!(parent.base);

    /// C++ `bool ColumnUDAString::validate (const std::string& value) const`.
    fn validate(&self, value: &str) -> bool {
        validate_values(&self.values, value)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDANumeric : public ColumnTypeNumeric`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDANumeric {
    /// The inherited `ColumnTypeNumeric`.
    pub parent: ColumnTypeNumeric,
    /// C++ `_values`.
    pub values: Vec<String>,
}

impl Default for ColumnUDANumeric {
    fn default() -> Self {
        ColumnUDANumeric::new()
    }
}

impl ColumnUDANumeric {
    /// C++ `ColumnUDANumeric::ColumnUDANumeric ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeNumeric::new();
        parent.base.name = "<uda>".to_string();
        parent.base.type_ = "numeric".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = String::new();
        parent.base.uda = true;
        parent.base.styles = strings(&["default", "indicator"]);
        ColumnUDANumeric { parent, values: Vec::new() }
    }
}

impl Column for ColumnUDANumeric {
    column_base!(parent.base);

    /// C++ `bool ColumnUDANumeric::validate (const std::string& value) const`.
    fn validate(&self, value: &str) -> bool {
        validate_values(&self.values, value)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeNumeric::modify_numeric(self, task, value, env)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDADate : public ColumnTypeDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDADate {
    /// The inherited `ColumnTypeDate`.
    pub parent: ColumnTypeDate,
    /// C++ `_values`.
    pub values: Vec<String>,
}

impl ColumnUDADate {
    /// C++ `ColumnUDADate::ColumnUDADate ()` (the `ColumnTypeDate` base
    /// constructor reads `rc.dateformat` for its examples).
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeDate::new(config);
        parent.base.name = "<uda>".to_string();
        parent.base.type_ = "date".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = String::new();
        parent.base.uda = true;
        parent.base.styles = strings(&["default", "indicator"]);
        ColumnUDADate { parent, values: Vec::new() }
    }
}

impl Column for ColumnUDADate {
    column_base!(parent.base);

    /// C++ `bool ColumnUDADate::validate (const std::string& value) const`.
    fn validate(&self, value: &str) -> bool {
        validate_values(&self.values, value)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDate::modify_date(self, task, value, env)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDADuration : public ColumnTypeDuration`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDADuration {
    /// The inherited `ColumnTypeDuration`.
    pub parent: ColumnTypeDuration,
    /// C++ `_values`.
    pub values: Vec<String>,
}

impl Default for ColumnUDADuration {
    fn default() -> Self {
        ColumnUDADuration::new()
    }
}

impl ColumnUDADuration {
    /// C++ `ColumnUDADuration::ColumnUDADuration ()`.
    pub fn new() -> Self {
        let mut parent = ColumnTypeDuration::new();
        parent.base.name = "<uda>".to_string();
        parent.base.type_ = "duration".to_string();
        parent.base.style = "default".to_string();
        parent.base.label = String::new();
        parent.base.uda = true;
        parent.base.styles = strings(&["default", "indicator"]);
        ColumnUDADuration { parent, values: Vec::new() }
    }
}

impl Column for ColumnUDADuration {
    column_base!(parent.base);

    /// C++ `bool ColumnUDADuration::validate (const std::string& value) const`.
    fn validate(&self, value: &str) -> bool {
        validate_values(&self.values, value)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeDuration::modify_duration(self, task, value, env)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class ColumnUDAUUID : public ColumnUUID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnUDAUUID {
    /// The inherited `ColumnUUID`.
    pub parent: ColumnUUID,
}

impl Default for ColumnUDAUUID {
    fn default() -> Self {
        ColumnUDAUUID::new()
    }
}

impl ColumnUDAUUID {
    /// C++ `ColumnUDAUUID::ColumnUDAUUID ()`.
    pub fn new() -> Self {
        let mut parent = ColumnUUID::new();
        let base = &mut parent.parent.base;
        base.name = "<uda>".to_string();
        base.type_ = "uuid".to_string();
        base.style = "long".to_string();
        base.label = String::new();
        base.modifiable = true;
        base.uda = true;
        base.styles = strings(&["long", "short"]);
        base.examples = strings(&["f30cb9c3-3fc0-483f-bfb2-3bf134f00694", "f30cb9c3"]);
        ColumnUDAUUID { parent }
    }
}

impl Column for ColumnUDAUUID {
    column_base!(parent.parent.base);

    /// C++ `bool ColumnUDAUUID::validate (const std::string& input) const`:
    /// a complete UUID (`Lexer::isUUID` with an end boundary).
    fn validate(&self, input: &str) -> bool {
        let mut lex = Lexer::new(input);
        let mut token = String::new();
        let mut type_ = Type::word;
        lex.is_uuid(&mut token, &mut type_, true)
    }

    /// Inherited from `ColumnTypeString::modify` (through `ColumnUUID`),
    /// which uses this column's `validate`.
    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }
}
