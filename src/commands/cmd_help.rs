//! Rust translation of `src/commands/CmdHelp.cpp`.
//!
//! `task help`: the usage table (every command's usage and description,
//! regular commands first, then the `_` helper commands, then the aliases)
//! followed by the general help text; `task help usage` shows only the
//! usage table. `task --help` is rewritten to `task help` by `main`.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::shared::format::format;
use crate::shared::shared::close_enough;
use crate::shared::table::Table;

/// The general help text that follows the usage table, copied verbatim from
/// `CmdHelp::execute`, one C++ string literal per element (a unit test
/// re-extracts it from the C++ source when the reference is available).
const HELP_TEXT: &str = concat!(
    "Documentation for Taskwarrior can be found using 'man task', 'man taskrc', 'man ",
    "task-color', 'man task-sync' or at https://taskwarrior.org\n",
    "\n",
    "The general form of commands is:\n",
    "  task [<filter>] <command> [<mods>]\n",
    "\n",
    "The <filter> consists of zero or more restrictions on which tasks to select, ",
    "such as:\n",
    "  task                                      <command> <mods>\n",
    "  task 28                                   <command> <mods>\n",
    "  task +weekend                             <command> <mods>\n",
    "  task project:Home due.before:today        <command> <mods>\n",
    "  task ebeeab00-ccf8-464b-8b58-f7f2d606edfb <command> <mods>\n",
    "\n",
    "By default, filter elements are combined with an implicit 'and' operator, but ",
    "'or' and 'xor' may also be used, provided parentheses are included:\n",
    "  task '(/[Cc]at|[Dd]og/ or /[0-9]+/)'      <command> <mods>\n",
    "\n",
    "A filter may target specific tasks using ID or UUID numbers.  To specify ",
    "multiple tasks use one of these forms:\n",
    "  task 1,2,3                                    delete\n",
    "  task 1-3                                      info\n",
    "  task 1,2-5,19                                 modify pri:H\n",
    "  task 4-7 ebeeab00-ccf8-464b-8b58-f7f2d606edfb info\n",
    "\n",
    "The <mods> consist of zero or more changes to apply to the selected tasks, ",
    "such as:\n",
    "  task <filter> <command> project:Home\n",
    "  task <filter> <command> +weekend +garden due:tomorrow\n",
    "  task <filter> <command> Description/annotation text\n",
    "  task <filter> <command> /from/to/     <- replace first match\n",
    "  task <filter> <command> /from/to/g    <- replace all matches\n",
    "\n",
    "Tags are arbitrary words, any quantity:\n",
    "  +tag       The + means add the tag\n",
    "  -tag       The - means remove the tag\n",
    "\n",
    "Built-in attributes are:\n",
    "  description:    Task description text\n",
    "  status:         Status of task - pending, completed, deleted, waiting\n",
    "  project:        Project name\n",
    "  priority:       Priority\n",
    "  due:            Due date\n",
    "  recur:          Recurrence frequency\n",
    "  until:          Expiration date of a task\n",
    "  limit:          Desired number of rows in report, or 'page'\n",
    "  wait:           Date until task becomes pending\n",
    "  entry:          Date task was created\n",
    "  end:            Date task was completed/deleted\n",
    "  start:          Date task was started\n",
    "  scheduled:      Date task is scheduled to start\n",
    "  modified:       Date task was last modified\n",
    "  depends:        Other tasks that this task depends upon\n",
    "\n",
    "Attribute modifiers make filters more precise.  Supported modifiers are:\n",
    "\n",
    "  Modifiers         Example            Equivalent           Meaning\n",
    "  ----------------  -----------------  -------------------  -------------------------\n",
    "                    due:today          due = today          Fuzzy match\n",
    "  not               due.not:today      due != today         Fuzzy non-match\n",
    "  before, below     due.before:today   due < today          Exact date comparison\n",
    "  after, above      due.after:today    due >= tomorrow      Exact date comparison\n",
    "  none              project.none:      project == ''        Empty\n",
    "  any               project.any:       project !== ''       Not empty\n",
    "  is, equals        project.is:x       project == x         Exact match\n",
    "  isnt              project.isnt:x     project !== x        Exact non-match\n",
    "  has, contains     desc.has:Hello     desc ~ Hello         Pattern match\n",
    "  hasnt,            desc.hasnt:Hello   desc !~ Hello        Pattern non-match\n",
    "  startswith, left  desc.left:Hel      desc ~ '^Hel'        Beginning match\n",
    "  endswith, right   desc.right:llo     desc ~ 'llo$'        End match\n",
    "  word              desc.word:Hello    desc ~ '\\bHello\\b'   Boundaried word match\n",
    "  noword            desc.noword:Hello  desc !~ '\\bHello\\b'  Boundaried word non-match\n",
    "\n",
    "Alternately algebraic expressions support:\n",
    "  and  or  xor            Logical operators\n",
    "  <  <=  =  !=  >=  >     Relational operators\n",
    "  (  )                    Precedence\n",
    "\n",
    "  task due.before:eom priority.not:L   list\n",
    "  task '(due < eom and priority != L)'  list\n",
    "\n",
    "The default .taskrc file can be overridden with:\n",
    "  task ... rc:<alternate file> ...\n",
    "  task ... rc:~/.alt_taskrc ...\n",
    "\n",
    "The values in .taskrc (or alternate) can be overridden with:\n",
    "  task ... rc.<name>=<value> ...\n",
    "  task rc.color=off list\n",
    "\n",
    "Any command or attribute name may be abbreviated if still unique:\n",
    "  task list project:Home\n",
    "  task li       pro:Home\n",
    "\n",
    "Some task descriptions need to be escaped because of the shell:\n",
    "  task add \"quoted ' quote\"\n",
    "  task add escaped \\' quote\n",
    "\n",
    "The argument -- tells Taskwarrior to treat all other args as description, even ",
    "if they would otherwise be attributes or tags:\n",
    "  task add -- project:Home needs scheduling\n",
    "\n",
    "Many characters have special meaning to the shell, including:\n",
    "  $ ! ' \" ( ) ; \\ ` * ? { } [ ] < > | & % # ~\n",
    "\n",
);

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHelp` (`help`).
pub struct CmdHelp {
    meta: CommandMeta,
}

impl CmdHelp {
    /// C++ `CmdHelp::CmdHelp`.
    pub fn new() -> Self {
        CmdHelp {
            meta: CommandMeta {
                keyword: "help".into(),
                usage: "task          help ['usage']".into(),
                description: "Displays this usage help text".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: false,
                accepts_modifications: false,
                accepts_miscellaneous: true,
                category: Category::Misc,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdHelp {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdHelp {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdHelp::execute`: with the single word `usage` (or an
    /// abbreviation of it) only the usage table, otherwise the usage table
    /// and the general help text.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let words = context
            .cli2
            .get_words(&context.config, &mut context.debug_messages);
        if words.len() == 1 && close_enough("usage", &words[0], 0) {
            *output = "\n".to_string() + &compose_usage(context) + "\n";
        } else {
            *output = "\n".to_string() + &compose_usage(context) + "\n" + HELP_TEXT;
        }

        Ok(0)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdHelp::composeUsage`: a three-column table with a static `Usage:`
/// row, then the usage and description of every regular command (sorted by
/// name), then of every `_` helper command, then a blank row and one
/// `Aliased to '<value>'` row per `alias.<name>` setting.
fn compose_usage(context: &mut Context) -> String {
    let mut view = Table::new();
    view.width(context.get_width());
    view.add("");
    view.add("");
    view.add("");

    // Static first row.
    let row = view.add_row();
    view.set(row, 0, "Usage:");
    view.set(row, 1, "task");
    view.set(row, 2, "Runs rc.default.command, if specified.");

    // Obsolete method of getting a list of all commands.
    let mut all: Vec<&String> = context.commands.keys().collect();

    // Sort alphabetically by usage.
    all.sort();

    // Add the regular commands.
    for name in &all {
        if !name.starts_with('_') {
            let command = &context.commands[name.as_str()];
            let row = view.add_row();
            view.set(row, 1, command.usage());
            view.set(row, 2, command.description());
        }
    }

    // Add the helper commands.
    for name in &all {
        if name.starts_with('_') {
            let command = &context.commands[name.as_str()];
            let row = view.add_row();
            view.set(row, 1, command.usage());
            view.set(row, 2, command.description());
        }
    }

    // Add the aliases commands.
    let row = view.add_row();
    view.set(row, 1, " ");

    for (key, value) in &context.config {
        // C++ `alias.first.substr (0, 6) == "alias."`, then `substr (6)`.
        if let Some(name) = key.strip_prefix("alias.") {
            let row = view.add_row();
            view.set(row, 1, name);
            view.set(row, 2, &format("Aliased to '{1}'", &[value]));
        }
    }

    view.render()
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

    /// Decodes the C++ string literals of `CmdHelp::execute`'s help text
    /// (from `"Documentation for` to the terminating `;`).
    fn extract_cpp_help(source: &str) -> String {
        let start = source
            .find("\"Documentation for Taskwarrior")
            .expect("help text in CmdHelp.cpp");
        let mut result = String::new();
        let mut chars = source[start..].chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => loop {
                    match chars.next().expect("unterminated literal") {
                        '"' => break,
                        '\\' => match chars.next().expect("escape") {
                            'n' => result.push('\n'),
                            '"' => result.push('"'),
                            '\'' => result.push('\''),
                            '\\' => result.push('\\'),
                            other => panic!("unexpected escape \\{other}"),
                        },
                        other => result.push(other),
                    }
                },
                ';' => break,
                c if c.is_whitespace() => {}
                other => panic!("unexpected character {other:?}"),
            }
        }
        result
    }

    #[test]
    fn help_text_matches_cpp_source_verbatim() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../taskwarrior-opus47-dmallya93/src/commands/CmdHelp.cpp");
        let Ok(source) = std::fs::read_to_string(&path) else {
            eprintln!("skipping: C++ reference {} not found", path.display());
            return;
        };
        let expected = extract_cpp_help(&source);
        assert!(expected.len() > 4000);
        assert_eq!(HELP_TEXT, expected);
    }

    #[test]
    fn help_text_spot_checks() {
        assert!(HELP_TEXT.starts_with(
            "Documentation for Taskwarrior can be found using 'man task', 'man taskrc', \
             'man task-color', 'man task-sync' or at https://taskwarrior.org\n\n"
        ));
        assert!(HELP_TEXT.contains(
            "  word              desc.word:Hello    desc ~ '\\bHello\\b'   Boundaried word match\n"
        ));
        assert!(HELP_TEXT.contains("  task add escaped \\' quote\n"));
        assert!(HELP_TEXT.ends_with("  $ ! ' \" ( ) ; \\ ` * ? { } [ ] < > | & % # ~\n\n"));
    }

    #[test]
    fn help_without_words_is_usage_and_text() {
        let mut context = context(&[]);
        let mut output = String::new();
        let rc = CmdHelp::new().execute(&mut context, &mut output).unwrap();
        assert_eq!(rc, 0);
        let usage = compose_usage(&mut context);
        assert_eq!(output, "\n".to_string() + &usage + "\n" + HELP_TEXT);
    }

    #[test]
    fn usage_table_order_and_aliases() {
        let mut context = context(&[("alias.zork", "list")]);
        let usage = compose_usage(&mut context);
        let lines: Vec<&str> = usage.lines().collect();
        // Empty header labels wrap to no header lines: the static row is
        // first.
        assert!(lines[0].starts_with("Usage: task "), "{usage}");
        assert!(lines[0].ends_with("Runs rc.default.command, if specified."));
        let position = |needle: &str| {
            lines
                .iter()
                .position(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("{needle} missing:\n{usage}"))
        };
        let add = position("task          add <mods>");
        let version = position("task          version ");
        let helper = position("task          _aliases");
        let alias = position("Aliased to 'list'");
        assert!(add < version && version < helper && helper < alias);
        assert!(lines[alias].trim_start().starts_with("zork "), "{usage}");
        // The blank " " row separates the helpers from the aliases, which
        // follow in key order (the defaults `alias.burndown` … `alias.rm`,
        // then `alias.zork`).
        let burndown = position("Aliased to 'burndown.weekly'");
        assert!(lines[burndown - 1].trim().is_empty(), "{usage}");
        assert!(lines[burndown - 2].contains("task <filter> _zshuuids"));
        assert_eq!(alias, burndown + 4);
        assert!(lines[alias - 1].contains("Aliased to 'delete'"), "{usage}");
    }
}
