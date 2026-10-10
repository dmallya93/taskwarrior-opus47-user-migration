//! Rust translation of `src/columns/ColDescription.cpp`.
//!
//! Constructor metadata and the render settings it captures (`_hyphenate`,
//! `_dateformat`, `_indent`); `validate` and `modify` are inherited from
//! `ColumnTypeString`. The `measure` / `render` overrides arrive with
//! milestone 6.

use crate::columns::col_type_string::ColumnTypeString;
use crate::columns::column::{column_base, strings, Column, ModifyEnv};
use crate::error::Result;
use crate::shared::configuration::Configuration;
use crate::shared::datetime::Datetime;
use crate::task::Task;

/// C++ `class ColumnDescription : public ColumnTypeString`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDescription {
    /// The inherited `ColumnTypeString`.
    pub parent: ColumnTypeString,
    /// C++ `_hyphenate` (`rc.hyphenate`).
    pub hyphenate: bool,
    /// C++ `_dateformat` (`rc.dateformat.annotation`, else `rc.dateformat`).
    pub dateformat: String,
    /// C++ `_indent` (`rc.indent.annotation`, at least 0).
    pub indent: i32,
}

impl ColumnDescription {
    /// C++ `ColumnDescription::ColumnDescription ()`.
    pub fn new(config: &Configuration) -> Self {
        let mut parent = ColumnTypeString::new();
        parent.base.name = "description".to_string();
        parent.base.style = "combined".to_string();
        parent.base.label = "Description".to_string();
        parent.base.modifiable = true;
        parent.base.styles = strings(&["combined", "desc", "oneline", "truncated", "count", "truncated_count"]);

        let mut dateformat = config.get("dateformat.annotation");
        if dateformat.is_empty() {
            dateformat = config.get("dateformat");
        }

        let t = Datetime::new().to_string(&dateformat);
        let d = "Move your clothes down on to the lower peg";
        let a1 = "Immediately before your lunch";
        let a2 = "If you are playing in the match this afternoon";
        let a3 = "Before you write your letter home";
        let a4 = "If you're not getting your hair cut";
        // C++ `d.substr (0, 20)` (ASCII).
        let d20 = &d[..20];
        parent.base.examples = vec![
            format!("{d}\n  {t} {a1}\n  {t} {a2}\n  {t} {a3}\n  {t} {a4}"),
            d.to_string(),
            format!("{d} {t} {a1} {t} {a2} {t} {a3} {t} {a4}"),
            format!("{d20}..."),
            format!("{d} [4]"),
            format!("{d20}... [4]"),
        ];

        ColumnDescription {
            parent,
            hyphenate: config.get_boolean("hyphenate"),
            dateformat,
            indent: config.get_integer("indent.annotation").max(0),
        }
    }
}

impl Column for ColumnDescription {
    column_base!(parent.base);

    fn validate(&self, input: &str) -> bool {
        ColumnTypeString::validate_string(input)
    }

    fn modify(&self, task: &mut Task, value: &str, env: &mut dyn ModifyEnv) -> Result<()> {
        ColumnTypeString::modify_string(self, task, value, env)
    }
}
