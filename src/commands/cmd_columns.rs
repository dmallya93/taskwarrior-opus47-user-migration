//! Rust translation of `src/commands/CmdColumns.cpp`.
//!
//! `task columns [substring]`: a table of every column, its type, whether it
//! is modifiable, its formats and examples; `task _columns`: only the names.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::shared::find;
use crate::shared::table::Table;
use crate::util::{optional_blank_line, set_header_underline};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdColumns` (`columns`).
pub struct CmdColumns {
    meta: CommandMeta,
}

impl CmdColumns {
    /// C++ `CmdColumns::CmdColumns`.
    pub fn new() -> Self {
        CmdColumns {
            meta: CommandMeta {
                keyword: "columns".into(),
                usage: "task          columns [substring]".into(),
                description: "All supported columns and formatting styles".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Config,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdColumns {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdColumns {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdColumns::execute`.
    ///
    /// Lists every registered column (sorted by name) whose name contains the
    /// optional search word (case-insensitive), one row per style, followed
    /// by the generic `<uda>` rows and the default-format footnote text.
    ///
    /// # Errors
    /// `You can only specify one search string.` for more than one word, or
    /// an unrecognized colour from [`set_header_underline`].
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Obtain the arguments from the description.  That way, things like '--'
        // have already been handled.
        let words = context
            .cli2
            .get_words(&context.config, &mut context.debug_messages);
        if words.len() > 1 {
            return Err(TwError::Message(
                "You can only specify one search string.".into(),
            ));
        }

        // Include all columns in the table (the registry is a BTreeMap, so
        // the names are already sorted as the C++ `std::sort` leaves them).
        let columns: Vec<_> = context
            .columns
            .iter()
            .map(|(n, c)| (n.clone(), c.clone()))
            .collect();

        // Render a list of column names, formats and examples.
        let mut formats = Table::new();
        formats.width(context.get_width());
        formats.add("Columns");
        formats.add("Type");
        formats.add("Modifiable");
        formats.add("Supported Formats");
        formats.add("Example");
        set_header_underline(context, &mut formats)?;

        for (name, column) in &columns {
            if words.is_empty() || find(name, &words[0], false).is_some() {
                let styles = column.styles();
                let examples = column.examples();

                for (i, style) in styles.iter().enumerate() {
                    let row = formats.add_row();
                    formats.set(row, 0, if i == 0 { name } else { "" });
                    formats.set(row, 1, if i == 0 { column.type_() } else { "" });
                    formats.set(
                        row,
                        2,
                        if i == 0 {
                            if column.modifiable() {
                                "Modifiable"
                            } else {
                                "Read Only"
                            }
                        } else {
                            ""
                        },
                    );
                    formats.set(row, 3, &(style.clone() + if i == 0 { "*" } else { "" }));
                    formats.set(row, 4, examples.get(i).map_or("", String::as_str));
                }
            }
        }

        let row = formats.add_row();
        formats.set(row, 0, "<uda>");
        formats.set(row, 1, "<type>");
        formats.set(row, 2, "Modifiable");
        formats.set(row, 3, "default*");

        let row = formats.add_row();
        formats.set(row, 0, "");
        formats.set(row, 3, "indicator");

        *output = optional_blank_line(context).to_string()
            + &formats.render()
            + "\n"
            + "* Means default format, and therefore optional.  For example, 'due' and "
            + "'due.formatted' are equivalent.\n";

        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionColumns` (`_columns`).
pub struct CmdCompletionColumns {
    meta: CommandMeta,
}

impl CmdCompletionColumns {
    /// C++ `CmdCompletionColumns::CmdCompletionColumns`.
    pub fn new() -> Self {
        CmdCompletionColumns {
            meta: CommandMeta {
                keyword: "_columns".into(),
                usage: "task          _columns".into(),
                description: "Displays only a list of supported columns".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCompletionColumns {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionColumns {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionColumns::execute`: every column name, sorted, one
    /// per line.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Include all columns (the BTreeMap registry iterates in sorted order).
        // Render only the column names.
        for name in context.columns.keys() {
            output.push_str(name);
            output.push('\n');
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::column::factory_all;
    use crate::context::set_stdout_tty_override;

    fn context(settings: &[(&str, &str)]) -> Context {
        set_stdout_tty_override(Some(false));
        let mut context = Context::new();
        context
            .config
            .parse(
                crate::context::CONFIGURATION_DEFAULTS,
                1,
                &[crate::context::task_rcdir()],
                "",
            )
            .unwrap();
        for (key, value) in [("defaultwidth", "80"), ("detection", "off")] {
            context.config.set(key, value);
        }
        for (key, value) in settings {
            context.config.set(key, *value);
        }
        let config = context.config.clone();
        factory_all(&mut context.columns, &config).unwrap();
        context
    }

    #[test]
    fn columns_table_uda_rows_and_footnote() {
        let mut context = context(&[("fontunderline", "off"), ("uda.size.type", "numeric")]);
        let mut output = String::new();
        assert_eq!(
            CmdColumns::new()
                .execute(&mut context, &mut output)
                .unwrap(),
            0
        );
        assert!(!output.contains('\x1b'), "{output:?}");
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "");
        assert!(
            lines[1].starts_with("Columns     Type    Modifiable Supported Formats Example"),
            "{output}"
        );
        assert!(
            lines[2].starts_with("----------- ------- ---------- ----------------- -------"),
            "{output}"
        );
        assert_eq!(
            lines[3],
            "depends     string  Modifiable list*             1 2 10"
        );
        assert_eq!(
            lines[4],
            "                               count             [3]"
        );
        assert_eq!(
            lines[5],
            "                               indicator         D"
        );
        assert!(
            lines.contains(&"project     string  Modifiable full*             home.garden"),
            "{output}"
        );
        assert!(
            lines.contains(&"                               parent            home"),
            "{output}"
        );
        assert!(lines.contains(&"uuid        string  Read Only  long*             f30cb9c3-3fc0-483f-bfb2-3bf134f00694"));
        assert!(
            lines.contains(&"size        numeric Modifiable default*"),
            "{output}"
        );
        assert!(
            lines.contains(&"<uda>       <type>  Modifiable default*"),
            "{output}"
        );
        assert!(output.ends_with(
            "<uda>       <type>  Modifiable default*\n                               indicator\n\n\
             * Means default format, and therefore optional.  For example, 'due' and 'due.formatted' are equivalent.\n"
        ));
    }

    #[test]
    fn columns_header_styles() {
        // fontunderline (default on) without colour: underlined labels, no dashes.
        let mut context = context(&[("verbose", "nothing")]);
        let mut output = String::new();
        CmdColumns::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert!(
            output.starts_with("\x1b[4mColumns    \x1b[0m \x1b[4mType"),
            "{output:?}"
        );
        assert!(!output.contains("-------"), "{output:?}");
        // Forced colour: alternate rows use color.alternate.
        let mut colored = self::context(&[("_forcecolor", "on"), ("color.alternate", "on gray2")]);
        let mut output = String::new();
        CmdColumns::new()
            .execute(&mut colored, &mut output)
            .unwrap();
        assert!(
            output.contains("\x1b[48;5;234mdepends    \x1b[0m"),
            "{output:?}"
        );
    }

    #[test]
    fn completion_columns_lists_sorted_names() {
        let mut context = context(&[("uda.aaa.type", "string")]);
        let mut output = String::new();
        assert_eq!(
            CmdCompletionColumns::new()
                .execute(&mut context, &mut output)
                .unwrap(),
            0
        );
        let names: Vec<&str> = output.lines().collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(names[0], "aaa");
        assert!(
            names.contains(&"description") && names.contains(&"uuid"),
            "{output}"
        );
        assert_eq!(names.len(), context.columns.len());
        assert!(output.ends_with("\n"));
    }
}
