//! Rust translation of `src/commands/CmdCommands.cpp`.
//!
//! `task commands`: a table of every registered command with its behaviour
//! flags ("DNA"); `task _commands`: the sorted command names; `task
//! _zshcommands`: `command:category:description` lines sorted by category,
//! then name, then description.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::shared::table::Table;
use crate::util::{optional_blank_line, set_header_underline};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCommands` (`commands`).
pub struct CmdCommands {
    meta: CommandMeta,
}

impl CmdCommands {
    /// C++ `CmdCommands::CmdCommands`.
    pub fn new() -> Self {
        CmdCommands {
            meta: CommandMeta {
                keyword: "commands".into(),
                usage: "task          commands".into(),
                description: "Generates a list of all commands, with behavior details".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdCommands {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCommands {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCommands::execute`: one row per registered command (registry
    /// key order) with its category, `RO`/`RW` and the `ID`, `GC`, `Recur`,
    /// `Ctxt`, `Filt`, `Mods` and `Misc` flags, then its description.
    ///
    /// # Errors
    /// An unrecognized colour from [`set_header_underline`].
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut view = Table::new();
        view.width(context.get_width());
        view.add("Command");
        view.add("Category");
        view.add_with("R/W", false, true);
        view.add_with("ID", false, true);
        view.add_with("GC", false, true);
        view.add_with("Recur", false, true);
        view.add_with("Context", false, true);
        view.add_with("Filter", false, true);
        view.add_with("Mods", false, true);
        view.add_with("Misc", false, true);
        view.add("Description");
        view.left_margin(context.config.get_integer("indent.report"));
        view.extra_padding(context.config.get_integer("row.padding"));
        view.intra_padding(context.config.get_integer("column.padding"));
        set_header_underline(context, &mut view)?;

        for (keyword, command) in &context.commands {
            let row = view.add_row();
            view.set(row, 0, keyword);
            view.set(row, 1, command.category().name());

            if command.read_only() {
                view.set(row, 2, "RO");
            } else {
                view.set(row, 2, "RW");
            }

            if command.displays_id() {
                view.set(row, 3, "ID");
            }

            if command.needs_gc() {
                view.set(row, 4, "GC");
            }

            if command.needs_recur_update() {
                view.set(row, 5, "Recur");
            }

            if command.uses_context(&context.config) {
                view.set(row, 6, "Ctxt");
            }

            if command.accepts_filter() {
                view.set(row, 7, "Filt");
            }

            if command.accepts_modifications() {
                view.set(row, 8, "Mods");
            }

            if command.accepts_miscellaneous() {
                view.set(row, 9, "Misc");
            }

            view.set(row, 10, command.description());
        }

        let blank = optional_blank_line(context);
        *output = blank.to_string() + &view.render() + blank + "\n";

        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdCompletionCommands` (`_commands`).
pub struct CmdCompletionCommands {
    meta: CommandMeta,
}

impl CmdCompletionCommands {
    /// C++ `CmdCompletionCommands::CmdCompletionCommands`.
    pub fn new() -> Self {
        CmdCompletionCommands {
            meta: CommandMeta {
                keyword: "_commands".into(),
                usage: "task          _commands".into(),
                description: "Generates a list of all commands, for autocompletion purposes".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
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

impl Default for CmdCompletionCommands {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdCompletionCommands {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdCompletionCommands::execute`: every command name, sorted
    /// alphabetically, one per line.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Get a list of all commands.
        let mut commands: Vec<&String> = context.commands.keys().collect();

        // Sort alphabetically.
        commands.sort();

        let mut out = String::new();
        for c in commands {
            out.push_str(c);
            out.push('\n');
        }

        *output = out;
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdZshCommands` (`_zshcommands`).
pub struct CmdZshCommands {
    meta: CommandMeta,
}

impl CmdZshCommands {
    /// C++ `CmdZshCommands::CmdZshCommands`.
    pub fn new() -> Self {
        CmdZshCommands {
            meta: CommandMeta {
                keyword: "_zshcommands".into(),
                usage: "task          _zshcommands".into(),
                description: "Generates a list of all commands, for zsh autocompletion purposes"
                    .into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
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

impl Default for CmdZshCommands {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdZshCommands {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdZshCommands::execute`: `command:category:description` for
    /// every command, sorted by category (enum order), then command name,
    /// then description.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Get a list of all command descriptions, sorted by category and then
        // alphabetically by command name.
        let mut commands: Vec<ZshCommand> = context
            .commands
            .iter()
            .map(|(keyword, command)| ZshCommand {
                category: command.category(),
                command: keyword.clone(),
                description: command.description().to_string(),
            })
            .collect();

        commands.sort();

        // Emit the commands in order.
        let mut out = String::new();
        for zc in &commands {
            out.push_str(&zc.command);
            out.push(':');
            out.push_str(zc.category.name());
            out.push(':');
            out.push_str(&zc.description);
            out.push('\n');
        }

        *output = out;
        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `struct ZshCommand`. The derived ordering compares the fields in
/// declaration order, which is the C++ `ZshCommand::operator<`
/// lexicographical comparison (category, then command, then description).
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct ZshCommand {
    category: Category,
    command: String,
    description: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::command::factory;
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
        for (key, value) in [("defaultwidth", "200"), ("detection", "off")] {
            context.config.set(key, value);
        }
        for (key, value) in settings {
            context.config.set(key, *value);
        }
        let config = context.config.clone();
        factory(&mut context.commands, &config).unwrap();
        context
    }

    #[test]
    fn commands_table_shows_dna() {
        let mut context = context(&[("fontunderline", "off")]);
        let mut output = String::new();
        let rc = CmdCommands::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert_eq!(rc, 0);
        assert!(!output.contains('\x1b'), "{output:?}");
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "");
        assert!(lines[1].starts_with("Command"), "{output}");
        assert_eq!(
            lines[1].split_whitespace().collect::<Vec<_>>(),
            [
                "Command",
                "Category",
                "R/W",
                "ID",
                "GC",
                "Recur",
                "Context",
                "Filter",
                "Mods",
                "Misc",
                "Description"
            ]
        );
        assert!(lines[2].starts_with("---"), "{output}");
        let add = lines.iter().find(|l| l.starts_with("add ")).unwrap();
        let add: Vec<&str> = add.split_whitespace().collect();
        assert_eq!(add[..6], ["add", "operation", "RW", "Ctxt", "Mods", "Adds"]);
        let list = lines.iter().find(|l| l.starts_with("list ")).unwrap();
        let list: Vec<&str> = list.split_whitespace().collect();
        assert_eq!(
            list[..9],
            ["list", "report", "RO", "ID", "GC", "Recur", "Ctxt", "Filt", "Most"]
        );
        // One row per registered command, plus blank, header and rule lines,
        // then the optional blank line and the final `'\n'`.
        assert_eq!(lines.len(), context.commands.len() + 5);
        assert!(
            output.ends_with("Waiting (hidden) tasks\n\n\n"),
            "{output:?}"
        );
    }

    #[test]
    fn commands_without_blank_verbosity() {
        let mut context = context(&[("verbose", "nothing"), ("fontunderline", "off")]);
        let mut output = String::new();
        CmdCommands::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert!(output.starts_with("Command"), "{output:?}");
        assert!(output.ends_with("Waiting (hidden) tasks\n\n"), "{output:?}");
    }

    #[test]
    fn completion_commands_sorted_names() {
        let mut context = context(&[]);
        let mut output = String::new();
        let rc = CmdCompletionCommands::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert_eq!(rc, 0);
        let names: Vec<&str> = output.lines().collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(names.len(), context.commands.len());
        assert_eq!(names[0], "_aliases");
        assert!(names.contains(&"list"));
    }

    #[test]
    fn zsh_commands_sorted_by_category_then_name() {
        let mut context = context(&[]);
        let mut output = String::new();
        let rc = CmdZshCommands::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert_eq!(rc, 0);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), context.commands.len());
        // Categories in enum order: metadata first, internal last.
        let category = |l: &str| l.split(':').nth(1).unwrap().to_string();
        assert_eq!(category(lines[0]), "metadata");
        assert_eq!(category(lines[lines.len() - 1]), "internal");
        assert!(lines.contains(&"add:operation:Adds a new task"));
        let order: Vec<Category> = lines
            .iter()
            .map(|l| {
                let name = category(l);
                Category::ALL
                    .into_iter()
                    .find(|c| c.name() == name)
                    .unwrap()
            })
            .collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
    }
}
