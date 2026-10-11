//! Rust translation of `src/commands/CmdUDAs.cpp`.
//!
//! Each struct below is one C++ class from `CmdUDAs.cpp`, carrying the exact
//! metadata of its C++ constructor.

use std::collections::BTreeMap;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::filter::Filter;
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::shared::join;
use crate::shared::table::Table;
use crate::task::Task;
use crate::util::{optional_blank_line, set_header_underline};

/// The C++ scan shared by both commands: for every config key (in map
/// order) starting with `uda.` and containing `.type` anywhere, the text
/// between `uda.` and the next `.`. As in C++, a UDA with two matching keys
/// (e.g. `uda.x.type` and `uda.x.typeface`) is listed twice.
fn defined_udas(config: &Configuration) -> Vec<String> {
    let mut udas: Vec<String> = Vec::new();
    for (name, _) in config {
        if name.starts_with("uda.") && name.contains(".type") {
            if let Some(period) = name[4..].find('.') {
                udas.push(name[4..4 + period].to_string());
            }
        }
    }
    udas
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdUDAs` (`udas`).
pub struct CmdUDAs {
    meta: CommandMeta,
}

impl CmdUDAs {
    /// C++ `CmdUDAs::CmdUDAs`.
    pub fn new() -> Self {
        CmdUDAs {
            meta: CommandMeta {
                keyword: "udas".into(),
                usage: "task          udas".into(),
                description: "Shows all the defined UDA details".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Config,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdUDAs {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdUDAs {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdUDAs::execute`. The orphan footer's singular/plural choice
    /// tests the number of *defined* UDAs, as the C++ does.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut rc = 0;
        let mut out = String::new();

        let mut udas = defined_udas(&context.config);

        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        if !udas.is_empty() {
            udas.sort();

            // Render a list of UDA name, type, label, allowed values,
            // possible default value, and finally the usage count.
            let mut table = Table::new();
            table.width(context.get_width());
            table.add("Name");
            table.add("Type");
            table.add("Label");
            table.add("Allowed Values");
            table.add("Default");
            table.add("Usage Count");
            set_header_underline(context, &mut table)?;

            for uda in &udas {
                let type_ = context.config.get(&format!("uda.{uda}.type"));
                let mut label = context.config.get(&format!("uda.{uda}.label"));
                let values = context.config.get(&format!("uda.{uda}.values"));
                let defval = context.config.get(&format!("uda.{uda}.default"));
                if label.is_empty() {
                    label = uda.clone();
                }

                // Count UDA usage by UDA.
                let mut count: i32 = 0;
                for i in &filtered {
                    if i.has(uda) {
                        count += 1;
                    }
                }

                let row = table.add_row();
                table.set(row, 0, uda);
                table.set(row, 1, &type_);
                table.set(row, 2, &label);
                table.set(row, 3, &values);
                table.set(row, 4, &defval);
                table.set_int(row, 5, count);
            }

            out += optional_blank_line(context);
            out += &table.render();
            out += optional_blank_line(context);
            out += &(if udas.len() == 1 {
                format("{1} UDA defined", &[&udas.len()])
            } else {
                format("{1} UDAs defined", &[&udas.len()])
            });
            out.push('\n');
        } else {
            out += "No UDAs defined.\n";
            rc = 1;
        }

        // Orphans are task attributes that are not represented in context.columns.
        let mut orphans: BTreeMap<String, i32> = BTreeMap::new();
        for i in &filtered {
            for att in i.get_uda_orphans(context) {
                *orphans.entry(att).or_insert(0) += 1;
            }
        }

        if !orphans.is_empty() {
            // Display the orphans and their counts.
            let mut orphan_table = Table::new();
            orphan_table.width(context.get_width());
            orphan_table.add("Orphan UDA");
            orphan_table.add("Usage Count");
            set_header_underline(context, &mut orphan_table)?;

            for (name, count) in &orphans {
                let row = orphan_table.add_row();
                orphan_table.set(row, 0, name);
                orphan_table.set_int(row, 1, *count);
            }

            out += optional_blank_line(context);
            out += &orphan_table.render();
            out += optional_blank_line(context);
            out += &(if udas.len() == 1 {
                format("{1} Orphan UDA", &[&orphans.len()])
            } else {
                format("{1} Orphan UDAs", &[&orphans.len()])
            });
            out.push('\n');
        }

        *output = out;
        Ok(rc)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionUDAs` (`_udas`).
pub struct CmdCompletionUDAs {
    meta: CommandMeta,
}

impl CmdCompletionUDAs {
    /// C++ `CmdCompletionUDAs::CmdCompletionUDAs`.
    pub fn new() -> Self {
        CmdCompletionUDAs {
            meta: CommandMeta {
                keyword: "_udas".into(),
                usage: "task          _udas".into(),
                description: "Shows the defined UDAs for completion purposes".into(),
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

impl Default for CmdCompletionUDAs {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionUDAs {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionUDAs::execute`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut udas = defined_udas(&context.config);

        if !udas.is_empty() {
            udas.sort();
            *output = join("\n", &udas) + "\n";
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    const RC: &str = "defaultwidth=80\nfontunderline=0\nuda.size.type=numeric\n\
                      uda.size.label=Size\nuda.size.values=1,2,3\n\
                      uda.extra.type=string\nuda.extra.default=x\n";

    fn run(command: &dyn Command, seed: &[&str], rc_lines: &str, args: &[&str]) -> (i32, String) {
        let mut f = CommandFixture::new(seed, rc_lines, args);
        let mut output = String::new();
        let rc = command.execute(&mut f.ctx, &mut output).unwrap();
        (rc, output)
    }

    #[test]
    fn defined_udas_scans_keys_like_cpp() {
        let mut config = Configuration::new();
        config.insert("uda.a.type".into(), "string".into());
        config.insert("uda.a.typeface".into(), "x".into());
        config.insert("uda.b.label".into(), "B".into());
        config.insert("uda.ctype".into(), "x".into());
        config.insert("x.uda.d.type".into(), "x".into());
        assert_eq!(defined_udas(&config), ["a", "a"]);
    }

    #[test]
    fn udas_table_counts_and_orphans() {
        let seed = [
            r#"{"description":"x","size":"2"}"#,
            r#"{"description":"y","gone":"1"}"#,
        ];
        let (rc, out) = run(&CmdUDAs::new(), &seed, RC, &["udas"]);
        assert_eq!(rc, 0);
        let expected = "\
Name     Type    Label    Allowed Values Default Usage Count
-------- ------- -------- -------------- ------- -----------
extra    string  extra                   x       2
priority string  Priority H,M,L,                 0
size     numeric Size     1,2,3                  1
3 UDAs defined
Orphan UDA Usage Count
---------- -----------
gone       1
1 Orphan UDAs
";
        assert_eq!(out, expected);
    }

    #[test]
    fn single_uda_singular_footers_and_no_udas() {
        let seed = [r#"{"description":"y","gone":"1","lost":"2"}"#];
        let mut f = CommandFixture::new(&seed, "fontunderline=0\n", &["udas"]);
        // Only the compiled-in `uda.priority` is defined.
        let mut output = String::new();
        let rc = CmdUDAs::new().execute(&mut f.ctx, &mut output).unwrap();
        assert_eq!(rc, 0);
        assert!(output.contains("\n1 UDA defined\n"), "{output}");
        assert!(output.ends_with("\n2 Orphan UDA\n"), "{output}");

        // The defaults always define `uda.priority.type`; remove it to reach
        // the C++ "No UDAs defined." branch.
        f.ctx.config.remove("uda.priority.type");
        let mut output = String::new();
        let rc = CmdUDAs::new().execute(&mut f.ctx, &mut output).unwrap();
        assert_eq!(rc, 1);
        assert!(output.starts_with("No UDAs defined.\n"), "{output}");
        assert!(output.ends_with("\n2 Orphan UDAs\n"), "{output}");

        let mut output = String::new();
        let rc = CmdCompletionUDAs::new().execute(&mut f.ctx, &mut output).unwrap();
        assert_eq!((rc, output.as_str()), (0, ""));
    }

    #[test]
    fn completion_udas_sorted() {
        let (rc, out) = run(&CmdCompletionUDAs::new(), &[], RC, &["_udas"]);
        assert_eq!((rc, out.as_str()), (0, "extra\npriority\nsize\n"));
    }
}
