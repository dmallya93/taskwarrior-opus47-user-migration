//! Rust translation of `src/commands/CmdShow.cpp`.
//!
//! Milestone 1 ported `_show` (raw config output); milestone 8 ports `show`.
//!
//! Each struct below is one C++ class from `CmdShow.cpp`, carrying the exact
//! metadata of its C++ constructor. The C++ `configurationDefaults` parse
//! that `show` compares against is the [`Context::defaults`] snapshot
//! (foundation DD2), parsed the same way at startup.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::legacy::{legacy_check_for_deprecated_columns, legacy_check_for_deprecated_variables};
use crate::shared::color::Color;
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::fs::Directory;
use crate::shared::table::Table;
use crate::util::set_header_underline;

const STRING_CMD_SHOW_DIFFER_COLOR: &str = "These are highlighted in {1} above.";
const STRING_CMD_SHOW_CONFIG_ERROR: &str =
    "Configuration error: {1} contains an unrecognized value '{2}'.";

/// C++ `recognized` in `CmdShow::execute`: the regular configuration
/// variables, with a leading and trailing space so whole words can be
/// searched (the C++ appends `_forcecolor ` separately).
const RECOGNIZED: &str = concat!(
    " abbreviation.minimum",
    " active.indicator",
    " allow.empty.filter",
    " annotation.info",
    " avoidlastcolumn",
    " bulk",
    " calendar.details",
    " calendar.details.report",
    " calendar.holidays",
    " calendar.legend",
    " calendar.monthsperline",
    " calendar.offset",
    " calendar.offset.value",
    " color",
    " color.active",
    " color.alternate",
    " color.blocked",
    " color.blocking",
    " color.burndown.done",
    " color.burndown.pending",
    " color.burndown.started",
    " color.calendar.due",
    " color.calendar.due.today",
    " color.calendar.holiday",
    " color.calendar.scheduled",
    " color.calendar.overdue",
    " color.calendar.today",
    " color.calendar.weekend",
    " color.calendar.weeknumber",
    " color.completed",
    " color.debug",
    " color.deleted",
    " color.due",
    " color.due.today",
    " color.warning",
    " color.error",
    " color.footnote",
    " color.header",
    " color.history.add",
    " color.history.delete",
    " color.history.done",
    " color.label",
    " color.label.sort",
    " color.overdue",
    " color.recurring",
    " color.scheduled",
    " color.summary.background",
    " color.summary.bar",
    " color.sync.added",
    " color.sync.changed",
    " color.sync.rejected",
    " color.tagged",
    " color.undo.after",
    " color.undo.before",
    " color.until",
    " column.padding",
    " complete.all.tags",
    " confirmation",
    " context",
    " data.location",
    " dateformat",
    " dateformat.annotation",
    " dateformat.edit",
    " dateformat.holiday",
    " dateformat.info",
    " dateformat.report",
    " date.iso",
    " debug",
    " debug.hooks",
    " debug.parser",
    " default.command",
    " default.due",
    " default.project",
    " default.scheduled",
    " defaultheight",
    " defaultwidth",
    " default.timesheet.filter",
    " dependency.confirmation",
    " dependency.indicator",
    " dependency.reminder",
    " detection",
    " displayweeknumber",
    " due",
    " editor",
    " exit.on.missing.db",
    " purge.on-sync",
    " expressions",
    " fontunderline",
    " gc",
    " hooks",
    " hooks.location",
    " hyphenate",
    " indent.annotation",
    " indent.report",
    " journal.info",
    " journal.time",
    " journal.time.start.annotation",
    " journal.time.stop.annotation",
    " json.array",
    " limit",
    " list.all.projects",
    " list.all.tags",
    " nag",
    " news.version",
    " obfuscate",
    " print.empty.columns",
    " recurrence",
    " recurrence.confirmation",
    " recurrence.indicator",
    " recurrence.limit",
    " regex",
    " reserved.lines",
    " row.padding",
    " rule.color.merge",
    " rule.precedence.color",
    " search.case.sensitive",
    " sugar",
    " summary.all.projects",
    " sync.aws.access_key_id",
    " sync.aws.bucket",
    " sync.aws.default_credentials",
    " sync.aws.endpoint_url",
    " sync.aws.force_path_style",
    " sync.aws.profile",
    " sync.aws.region",
    " sync.aws.secret_access_key",
    " sync.gcp.credential_path",
    " sync.gcp.bucket",
    " sync.git.branch",
    " sync.git.git_path",
    " sync.git.local_only",
    " sync.git.local_path",
    " sync.git.remote",
    " sync.local.server_dir",
    " sync.server.client_id",
    " sync.encryption_secret",
    " sync.server.url",
    " sync.server.origin",
    " tag.indicator",
    " urgency.active.coefficient",
    " urgency.scheduled.coefficient",
    " urgency.annotations.coefficient",
    " urgency.blocked.coefficient",
    " urgency.blocking.coefficient",
    " urgency.due.coefficient",
    " urgency.project.coefficient",
    " urgency.tags.coefficient",
    " urgency.waiting.coefficient",
    " urgency.age.coefficient",
    " urgency.age.max",
    " urgency.inherit",
    " verbose",
    " weekstart",
    " xterm.title",
    " ",
);

/// C++ `std::string::substr (0, n)` compared against `prefix`: byte-wise
/// prefix test (`substr` never fails on a short string).
fn has_prefix(name: &str, prefix: &str) -> bool {
    name.as_bytes().starts_with(prefix.as_bytes())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ static `isUdaUrgency`: `urgency.uda.<name>.*` is only meaningful when
/// `<name>` is a defined UDA (`uda.<name>.type` is set, honouring the
/// context like `Configuration::has`).
fn is_uda_urgency(config: &Configuration, name: &str) -> bool {
    if !has_prefix(name, "urgency.uda.") {
        return false;
    }

    // name.substr (12, name.find ('.', 12) - 12): up to the next dot, or the
    // rest of the name when there is none (npos - 12 is still "to the end").
    let rest = &name[12..];
    let uda = match rest.find('.') {
        Some(dot) => &rest[..dot],
        None => rest,
    };
    config.has(&format!("uda.{uda}.type"))
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdShow` (`show`).
pub struct CmdShow {
    meta: CommandMeta,
}

impl CmdShow {
    /// C++ `CmdShow::CmdShow`.
    pub fn new() -> Self {
        CmdShow {
            meta: CommandMeta {
                keyword: "show".into(),
                usage: "task          show [all | substring]".into(),
                description: "Shows all configuration variables or subset".into(),
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

impl Default for CmdShow {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdShow {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdShow::execute`: the configuration table (optionally only
    /// the variables whose name contains the one search word; `all` shows
    /// everything), colouring unrecognized variables with `color.error` and
    /// non-default ones with `color.warning`, followed by the explanatory
    /// notes and the configuration checks. Returns 1 only for an empty
    /// configuration.
    ///
    /// Deviation: C++ `Color (std::string)` throws on a bad `color.error` /
    /// `color.warning` (or header colour); the same text is returned as `Err`.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut rc = 0;
        let mut out = String::new();

        // Obtain the arguments from the description.  That way, things like '--'
        // have already been handled.
        let words = context
            .cli2
            .get_words(&context.config, &mut context.debug_messages);
        if words.len() > 1 {
            return Err(TwError::Message(
                "You can only specify 'all' or a search string.".into(),
            ));
        }

        let width = context.get_width();

        // Complain about configuration variables that are not recognized.
        // These are the regular configuration variables.
        let mut recognized = RECOGNIZED.to_string();

        // This configuration variable is supported, but not documented.  It exists
        // so that unit tests can force color to be on even when the output from task
        // is redirected to a file, or stdout is not a tty.
        recognized += "_forcecolor ";

        // C++ iterates the std::map directly: every key in order, raw value.
        let all = context.config.all();

        let mut unrecognized: Vec<String> = Vec::new();
        for first in &all {
            // Disallow partial matches by tacking a leading and trailing space on each
            // variable name.
            let pattern = format!(" {first} ");
            if !recognized.contains(&pattern) {
                // These are special configuration variables, because their name is
                // dynamic.
                if !has_prefix(first, "color.keyword.")
                    && !has_prefix(first, "color.project.")
                    && !has_prefix(first, "color.tag.")
                    && !has_prefix(first, "color.uda.")
                    && !has_prefix(first, "context.")
                    && !has_prefix(first, "holiday.")
                    && !has_prefix(first, "report.")
                    && !has_prefix(first, "alias.")
                    && !has_prefix(first, "hook.")
                    && !has_prefix(first, "uda.")
                    && !has_prefix(first, "default.")
                    && !has_prefix(first, "urgency.user.project.")
                    && !has_prefix(first, "urgency.user.tag.")
                    && !has_prefix(first, "urgency.user.keyword.")
                    && !is_uda_urgency(&context.config, first)
                {
                    unrecognized.push(first.clone());
                }
            }
        }

        // Find all the values that match the defaults, for highlighting.
        let mut default_values: Vec<String> = Vec::new();
        let default_config = &context.defaults;

        for first in &all {
            if context.config.get_from_context(first, false) != default_config.get(first) {
                default_values.push(first.clone());
            }
        }

        // Create output view.
        let mut view = Table::new();
        view.width(width);
        view.add("Config Variable");
        view.add("Value");
        set_header_underline(context, &mut view)?;

        let mut error = Color::new();
        let mut warning = Color::new();
        if context.color() {
            error = Color::parse(&context.config.get("color.error"))?;
            warning = Color::parse(&context.config.get("color.warning"))?;
        }

        let mut issue_error = false;
        let mut issue_warning = false;

        let mut section = String::new();

        // Look for the first plausible argument which could be a pattern
        if let Some(first) = words.first() {
            section = first.clone();
        }

        if section == "all" {
            section = String::new();
        }

        // Re-read the keys, as C++ iterates the map again after
        // Context::color (which may set `color`/`detection` on its first call).
        for first in &context.config.all() {
            if first.contains(section.as_str()) {
                // Look for unrecognized.
                let mut color = Color::new();
                if unrecognized.contains(first) {
                    issue_error = true;
                    color = error;
                } else if default_values.contains(first) {
                    issue_warning = true;
                    color = warning;
                }

                let value = context.config.get_from_context(first, false);
                let row = view.add_row();
                view.set_colored(row, 0, first, color);
                view.set_colored(row, 1, &value, color);

                // default_config[i.first] (operator[]: raw lookup, "" if unset).
                let default_value = context.defaults.get_from_context(first, false);
                if default_value != value && !default_value.is_empty() {
                    let row = view.add_row();
                    view.set_colored(row, 0, &(String::from("  ") + "Default value"), color);
                    view.set_colored(row, 1, &default_value, color);
                }
            }
        }

        out.push('\n');
        out += &view.render();
        out += if view.rows() == 0 {
            "No matching configuration variables."
        } else {
            ""
        };
        out += if view.rows() == 0 { "\n\n" } else { "\n" };

        if issue_warning {
            out += "Some of your .taskrc variables differ from the default values.\n";

            if context.color() && warning.nontrivial() {
                out += "  ";
                out += &format(STRING_CMD_SHOW_DIFFER_COLOR, &[&warning.colorize("color")]);
                out += "\n\n";
            }
        }

        // Display the unrecognized variables.
        if issue_error {
            out += "Your .taskrc file contains these unrecognized variables:\n";

            for i in &unrecognized {
                out += "  ";
                out += i;
                out.push('\n');
            }

            if unrecognized.iter().any(|i| has_prefix(i, "urgency.uda.")) {
                out += "urgency.uda.<name> coefficients only apply to UDAs. For tags, use \
                        urgency.user.tag.<tag>.coefficient.\n";
            }

            if context.color() && error.nontrivial() {
                out.push('\n');
                out += &format(STRING_CMD_SHOW_DIFFER_COLOR, &[&error.colorize("color")]);
            }

            out += "\n\n";
        }

        out += &legacy_check_for_deprecated_variables(&context.config);
        out += &legacy_check_for_deprecated_columns(&context.config);

        // Check for bad values in rc.calendar.details.
        let calendardetails = context.config.get("calendar.details");
        if calendardetails != "full" && calendardetails != "sparse" && calendardetails != "none" {
            out += &format(
                STRING_CMD_SHOW_CONFIG_ERROR,
                &[&"calendar.details", &calendardetails],
            );
            out.push('\n');
        }

        // Check for bad values in rc.calendar.holidays.
        let calendarholidays = context.config.get("calendar.holidays");
        if calendarholidays != "full" && calendarholidays != "sparse" && calendarholidays != "none"
        {
            out += &format(
                STRING_CMD_SHOW_CONFIG_ERROR,
                &[&"calendar.holidays", &calendarholidays],
            );
            out.push('\n');
        }

        // Verify installation.  This is mentioned in the documentation as the way
        // to ensure everything is properly installed.

        if context.config.all().is_empty() {
            out += "Configuration error: .taskrc contains no entries.\n";
            rc = 1;
        } else {
            let location = Directory::new(&context.config.get("data.location"));

            if location.data.is_empty() {
                out += "Configuration error: data.location not specified in .taskrc file.\n";
            }

            if !location.exists() {
                out += "Configuration error: data.location contains a directory name that \
                        doesn't exist, or is unreadable.\n";
            }
        }

        *output = out;
        Ok(rc)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdShowRaw` (`_show`).
pub struct CmdShowRaw {
    meta: CommandMeta,
}

impl CmdShowRaw {
    /// C++ `CmdShowRaw::CmdShowRaw`.
    pub fn new() -> Self {
        CmdShowRaw {
            meta: CommandMeta {
                keyword: "_show".into(),
                usage: "task          _show".into(),
                description: "Shows all configuration settings in a machine-readable format".into(),
                read_only: true,
                displays_id: false,
                category: Category::Internal,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdShowRaw {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdShowRaw {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdShowRaw::execute`: every setting as `name=value`, sorted by
    /// name.
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        // Get all the settings and sort alphabetically by name.
        let mut all = context.config.all();
        all.sort();

        // Display them all.
        let mut out = String::new();
        for i in &all {
            out.push_str(i);
            out.push('=');
            out.push_str(&context.config.get(i));
            out.push('\n');
        }

        *output = out;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    /// `task show <args>` in a fixture whose taskrc adds `rc_lines`.
    fn show(rc_lines: &str, args: &[&str]) -> (Result<i32>, String) {
        let mut full = vec!["show"];
        full.extend_from_slice(args);
        let mut f = CommandFixture::new(&[], rc_lines, &full);
        let mut output = String::new();
        let rc = CmdShow::new().execute(&mut f.ctx, &mut output);
        (rc, output)
    }

    #[test]
    fn show_recognized_list_is_space_delimited() {
        assert!(RECOGNIZED.starts_with(" abbreviation.minimum "));
        assert!(RECOGNIZED.ends_with(" xterm.title "));
        assert!(RECOGNIZED.contains(" purge.on-sync "));
        assert!(!RECOGNIZED.contains("  "));
    }

    #[test]
    fn show_is_uda_urgency() {
        let mut config = Configuration::new();
        config.set("uda.size.type", "numeric");
        assert!(is_uda_urgency(&config, "urgency.uda.size.coefficient"));
        assert!(is_uda_urgency(&config, "urgency.uda.size"));
        assert!(!is_uda_urgency(&config, "urgency.uda.tag.coefficient"));
        assert!(!is_uda_urgency(&config, "urgency.user.size.coefficient"));
        assert!(!is_uda_urgency(&config, "urgency.uda"));
    }

    #[test]
    fn show_no_matches() {
        // The header is rendered even without rows; the trailing "\n" is
        // legacyCheckForDeprecatedColumns.
        let (rc, output) = show("fontunderline=0\n", &["zzzzzz"]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(
            output,
            "\nConfig Variable Value\n--------------- -----\n\
             No matching configuration variables.\n\n\n"
        );
    }

    #[test]
    fn show_too_many_words() {
        let (rc, output) = show("", &["one", "two"]);
        assert_eq!(
            rc.unwrap_err().to_string(),
            "You can only specify 'all' or a search string."
        );
        assert_eq!(output, "");
    }

    #[test]
    fn show_differs_from_default() {
        // recurrence=off differs from the default (on).
        let (rc, output) = show("fontunderline=0\n", &["recurrence.limit"]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(
            output,
            "\nConfig Variable  Value\n---------------- -----\nrecurrence.limit 1\n\n\n"
        );
        let (rc, output) = show("fontunderline=0\nrecurrence.limit=3\n", &["recurrence.limit"]);
        assert_eq!(rc.unwrap(), 0);
        assert_eq!(
            output,
            "\nConfig Variable  Value\n---------------- -----\n\
             recurrence.limit 3\n  Default value  1\n\n\
             Some of your .taskrc variables differ from the default values.\n\n"
        );
    }

    #[test]
    fn show_unrecognized() {
        let (rc, output) = show("foo.bar=1\nurgency.uda.x.coefficient=2\n", &["all"]);
        assert_eq!(rc.unwrap(), 0);
        assert!(output.contains("\nfoo.bar "), "{output}");
        assert!(output.contains(
            "Your .taskrc file contains these unrecognized variables:\n  foo.bar\n  \
             urgency.uda.x.coefficient\nurgency.uda.<name> coefficients only apply to UDAs. \
             For tags, use urgency.user.tag.<tag>.coefficient.\n\n\n"
        ));

        // A defined UDA makes its urgency coefficient recognized; dynamic
        // prefixes are never reported.
        let (rc, output) = show(
            "uda.x.type=string\nurgency.uda.x.coefficient=2\ncolor.tag.foo=red\n",
            &[],
        );
        assert_eq!(rc.unwrap(), 0);
        assert!(!output.contains("unrecognized variables"), "{output}");
    }

    #[test]
    fn show_calendar_checks() {
        let (rc, output) = show(
            "calendar.details=bogus\ncalendar.holidays=nope\n",
            &["calendar.monthsperline"],
        );
        assert_eq!(rc.unwrap(), 0);
        assert!(output.ends_with(
            "Configuration error: calendar.details contains an unrecognized value 'bogus'.\n\
             Configuration error: calendar.holidays contains an unrecognized value 'nope'.\n"
        ));
    }

    #[test]
    fn show_missing_data_location() {
        let (rc, output) = show("", &["zzzzzz"]);
        assert_eq!(rc.unwrap(), 0);
        assert!(!output.contains("data.location"));

        let mut f = CommandFixture::new(&[], "", &["show", "zzzzzz"]);
        f.ctx.config.set("data.location", "/nonexistent/tw/show/dir");
        let mut output = String::new();
        assert_eq!(CmdShow::new().execute(&mut f.ctx, &mut output).unwrap(), 0);
        assert!(output.ends_with(
            "Configuration error: data.location contains a directory name that doesn't \
             exist, or is unreadable.\n"
        ));

        f.ctx.config.set("data.location", "");
        let mut output = String::new();
        CmdShow::new().execute(&mut f.ctx, &mut output).unwrap();
        assert!(output.contains(
            "Configuration error: data.location not specified in .taskrc file.\n"
        ));
    }

    #[test]
    fn show_empty_configuration() {
        let mut f = CommandFixture::new(&[], "", &["show"]);
        f.ctx.config = Configuration::new();
        let mut output = String::new();
        assert_eq!(CmdShow::new().execute(&mut f.ctx, &mut output).unwrap(), 1);
        assert!(output.contains("\nNo matching configuration variables.\n\n"));
        assert!(output.ends_with("Configuration error: .taskrc contains no entries.\n"));
    }

    #[test]
    fn show_raw_sorted() {
        let mut context = Context::new();
        for (key, value) in [
            ("zeta", "last"),
            ("alpha", "1"),
            ("Beta", ""),
            ("a.b", "x=y"),
        ] {
            context.config.set(key, value);
        }
        let mut output = String::new();
        assert_eq!(
            CmdShowRaw::new()
                .execute(&mut context, &mut output)
                .unwrap(),
            0
        );
        assert_eq!(output, "Beta=\na.b=x=y\nalpha=1\nzeta=last\n");
    }

    #[test]
    fn show_raw_empty() {
        let mut context = Context::new();
        context.config = crate::shared::configuration::Configuration::new();
        let mut output = String::from("stale");
        CmdShowRaw::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert_eq!(output, "");
    }
}
