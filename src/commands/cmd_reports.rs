//! Rust translation of `src/commands/CmdReports.cpp`.
//!
//! `task reports`: a table of every custom and built-in report with its
//! description, and an `N reports` footnote line.

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::Result;
use crate::shared::table::Table;
use crate::util::{optional_blank_line, set_header_underline};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdReports` (`reports`).
pub struct CmdReports {
    meta: CommandMeta,
}

impl CmdReports {
    /// C++ `CmdReports::CmdReports`.
    pub fn new() -> Self {
        CmdReports {
            meta: CommandMeta {
                keyword: "reports".into(),
                usage: "task          reports".into(),
                description: "Lists all supported reports".into(),
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

impl Default for CmdReports {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdReports {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdReports::execute`.
    ///
    /// Collects every `report.<name>.columns` setting plus the built-in
    /// reports, sorts them and renders a Report/Description table.
    ///
    /// Deviation: C++ `commands[report]` on a name with no registered
    /// command would dereference a null pointer; here the description is
    /// left empty. (`Command::factory` registers a custom report for every
    /// `report.*.columns` setting, so this does not happen in practice.)
    ///
    /// # Errors
    /// An unrecognized colour from [`set_header_underline`].
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut reports: Vec<String> = Vec::new();

        // Add custom reports.
        for key in context.config.keys() {
            if let Some(report) = key.strip_prefix("report.") {
                if let Some(columns) = report.find(".columns") {
                    reports.push(report[..columns].to_string());
                }
            }
        }

        // Add known reports.
        for known in [
            "burndown.annual",
            "burndown.daily",
            "burndown.monthly",
            "burndown.weekly",
            "ghistory.annual",
            "ghistory.monthly",
            "history.annual",
            "history.monthly",
            "information",
            "projects",
            "summary",
            "tags",
        ] {
            reports.push(known.to_string());
        }

        reports.sort();

        // Compose the output.
        let mut view = Table::new();
        view.width(context.get_width());
        view.add("Report");
        view.add("Description");
        set_header_underline(context, &mut view)?;

        for report in &reports {
            let row = view.add_row();
            view.set(row, 0, report);
            let description = context
                .commands
                .get(report)
                .map(|command| command.description().to_string())
                .unwrap_or_default();
            view.set(row, 1, &description);
        }

        let mut out = optional_blank_line(context).to_string();
        out += &view.render();
        out += optional_blank_line(context);
        out += &format!("{} reports\n", reports.len());

        *output = out;
        Ok(0)
    }
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
        for (key, value) in [("defaultwidth", "80"), ("detection", "off")] {
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
    fn reports_table_and_count() {
        let mut context = context(&[
            ("fontunderline", "off"),
            ("report.aaa.columns", "id"),
            ("report.aaa.description", "My report"),
        ]);
        let mut output = String::new();
        assert_eq!(
            CmdReports::new()
                .execute(&mut context, &mut output)
                .unwrap(),
            0
        );
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "");
        assert_eq!(lines[1], "Report           Description");
        assert_eq!(
            lines[2],
            "---------------- --------------------------------------------------"
        );
        assert_eq!(lines[3], "aaa              My report");
        assert_eq!(lines[4], "active           Active tasks");
        assert!(
            lines.contains(&"burndown.annual  Shows a graphical burndown chart, by annual"),
            "{output}"
        );
        assert!(
            lines.contains(&"next             Most urgent tasks"),
            "{output}"
        );
        assert!(
            output.ends_with("waiting          Waiting (hidden) tasks\n\n30 reports\n"),
            "{output}"
        );
    }

    #[test]
    fn reports_without_blank_lines() {
        let mut context = context(&[("verbose", "nothing"), ("fontunderline", "off")]);
        let mut output = String::new();
        CmdReports::new()
            .execute(&mut context, &mut output)
            .unwrap();
        assert!(output.starts_with("Report "), "{output:?}");
        assert!(
            output.ends_with("Waiting (hidden) tasks\n29 reports\n"),
            "{output:?}"
        );
    }
}
