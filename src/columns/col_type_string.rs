//! Rust translation of `src/columns/ColTypeString.cpp`.
//!
//! `ColumnTypeString`: the base of every string-typed column. C++ declares no
//! `measure` / `render` of its own, so it keeps the no-op [`Column`]
//! defaults (`measure` -> `(0, 0)`, `render` -> no lines).

use crate::columns::column::{column_base, Column, ColumnBase, ModifyEnv, MODIFICATION_LABEL};
use crate::error::{Result, TwError};
use crate::lexer::{Lexer, Type};
use crate::shared::format::format;
use crate::task::Task;

/// C++ `#define STRING_INVALID_MOD`.
const STRING_INVALID_MOD: &str = "The '{1}' attribute does not allow a value of '{2}'.";

/// C++ `class ColumnTypeString : public Column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTypeString {
    /// The inherited `Column` members.
    pub base: ColumnBase,
}

impl Default for ColumnTypeString {
    fn default() -> Self {
        ColumnTypeString::new()
    }
}

impl ColumnTypeString {
    /// C++ `ColumnTypeString::ColumnTypeString () { _type = "string"; }`.
    pub fn new() -> Self {
        let mut base = ColumnBase::new();
        base.type_ = "string".to_string();
        ColumnTypeString { base }
    }

    /// C++ `bool ColumnTypeString::validate (const std::string& input) const`:
    /// any non-empty value.
    pub fn validate_string(input: &str) -> bool {
        !input.is_empty()
    }

    /// C++ `void ColumnTypeString::modify (Task& task, const std::string&
    /// value)`, with `this` the most-derived column (its virtual `validate`
    /// and `_name` are used).
    ///
    /// A value consisting of exactly one DOM reference token is evaluated
    /// first; anything else is stored as given.
    ///
    /// # Errors
    /// `The '{1}' attribute does not allow a value of '{2}'.` when
    /// `validate` rejects the (evaluated) value, and any Eval error.
    pub fn modify_string(this: &dyn Column, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        let label = MODIFICATION_LABEL;
        let name = this.name();

        // Only if it's a DOM ref, eval it first.
        let mut lexer = Lexer::new(value);
        let mut dom_ref = String::new();
        let mut type_ = Type::word;
        if lexer.token(&mut dom_ref, &mut type_) && type_ == Type::dom
            // Ensure 'value' contains only the DOM reference and no other tokens
            // The lexer.token returns false for end-of-string.
            // This works as long as all the DOM references we should support consist
            // only of a single token.
            && !lexer.token(&mut dom_ref, &mut type_)
        {
            let v = env.evaluate_dom_expression(value)?;
            let str_value = v.to_string();
            if this.validate(&str_value) {
                task.set(name, &str_value);
                env.debug(&format!("{label}{name} <-- '{str_value}' <-- '{value}'"));
            } else {
                return Err(TwError::Message(format(STRING_INVALID_MOD, &[&name, &value])));
            }
        } else if this.validate(value) {
            task.set(name, value);
            env.debug(&format!("{label}{name} <-- '{value}'"));
        } else {
            return Err(TwError::Message(format(STRING_INVALID_MOD, &[&name, &value])));
        }

        Ok(())
    }
}

impl Column for ColumnTypeString {
    column_base!(base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }
}
