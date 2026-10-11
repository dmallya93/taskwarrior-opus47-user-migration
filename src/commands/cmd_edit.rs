//! Rust translation of `src/commands/CmdEdit.cpp`.
//!
//! `task <filter> edit`: for every filtered task, write a text representation
//! (`formatTask`) to `task.<uuid8>.task` in `data.location`, launch the
//! editor on it through `system (3)` (i.e. `/bin/sh -c`), and parse the edited
//! text back into the task (`parseTask`), storing it with `TDB2::modify`
//! (hooks, validation and undo stay in the milestone 7 pipeline).
//!
//! Like the C++, the editor feedback (`Launching ...`, `Editing complete.`,
//! `Edits were detected.`, ...) goes straight to stdout, and a parse error is
//! printed straight to stderr before the "try again?" prompt.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::Write;

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::{strtol, Context};
use crate::dom::strtoll;
use crate::error::{Result, TwError};
use crate::filter::Filter;
use crate::lexer::Lexer;
use crate::shared::datetime::Datetime;
use crate::shared::duration::Duration;
use crate::shared::format::format;
use crate::shared::fs::{Directory, File, Path};
use crate::shared::json;
use crate::shared::pig::Pig;
use crate::shared::shared::{confirm, find_from, join, split, str_replace, with_console};
use crate::task::{Status, Task};
use crate::tdb2;

const STRING_EDIT_START_MOD: &str = "Start date modified.";
const STRING_EDIT_SCHED_MOD: &str = "Scheduled date modified.";
const STRING_EDIT_DUE_MOD: &str = "Due date modified.";
const STRING_EDIT_UNTIL_MOD: &str = "Until date modified.";
const STRING_EDIT_WAIT_MOD: &str = "Wait date modified.";

/// C++ `CmdEdit::editResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditResult {
    Error,
    Changes,
    NoChanges,
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdEdit` (`edit`).
pub struct CmdEdit {
    meta: CommandMeta,
}

impl CmdEdit {
    /// C++ `CmdEdit::ANNOTATION_EDIT_MARKER`.
    pub const ANNOTATION_EDIT_MARKER: &'static str = "\n                     ";

    /// C++ `CmdEdit::CmdEdit`.
    pub fn new() -> Self {
        CmdEdit {
            meta: CommandMeta {
                keyword: "edit".into(),
                usage: "task <filter> edit".into(),
                description: "Launches an editor to modify a task directly".into(),
                read_only: false,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: true,
                uses_context: true,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Operation,
                ..CommandMeta::default()
            },
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::string CmdEdit::findValue (const std::string& text, const
    /// std::string& name)`: the trimmed rest of the line following the first
    /// `name`, or `""` (also when that line has no terminating newline).
    pub(crate) fn find_value(text: &str, name: &str) -> String {
        if let Some(found) = find_from(text, name, 0, true) {
            if let Some(eol) = find_from(text, "\n", found + 1, true) {
                let value = &text[found + name.len()..eol];

                return Lexer::trim_with(value, "\t ");
            }
        }

        String::new()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::string CmdEdit::findMultilineValue (const std::string& text,
    /// const std::string& startMarker, const std::string& endMarker)`.
    pub(crate) fn find_multiline_value(text: &str, start_marker: &str, end_marker: &str) -> String {
        if let Some(start) = find_from(text, start_marker, 0, true) {
            if let Some(end) = find_from(text, end_marker, start, true) {
                // C++ `substr (start + len, end - (start + len))`: with an end
                // marker overlapping the start marker the count wraps, i.e.
                // the rest of the text.
                let begin = start + start_marker.len();
                let value = if end >= begin {
                    &text[begin..end]
                } else {
                    &text[begin.min(text.len())..]
                };
                return Lexer::trim_with(value, "\\\t ");
            }
        }
        String::new()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::vector<std::string> CmdEdit::findValues (const std::string&
    /// text, const std::string& name)`: like [`CmdEdit::find_value`] for every
    /// occurrence of `name` (searching from offset 1).
    pub(crate) fn find_values(text: &str, name: &str) -> Vec<String> {
        let mut results = Vec::new();
        let mut found: Option<usize> = Some(0);

        while let Some(position) = found {
            found = find_from(text, name, position + 1, true);
            if let Some(start) = found {
                if let Some(eol) = find_from(text, "\n", start + 1, true) {
                    let value = &text[start + name.len()..eol];

                    found = Some(eol - 1);
                    results.push(Lexer::trim_with(value, "\t "));
                }
            }
        }

        results
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::string CmdEdit::formatDate (Task& task, const std::string&
    /// attribute, const std::string& dateformat)`.
    ///
    /// # Errors
    /// The `Datetime` parse error for a stored value that is not a date.
    pub(crate) fn format_date(task: &Task, attribute: &str, dateformat: &str) -> Result<String> {
        let mut value = task.get(attribute);
        if !value.is_empty() {
            value = Datetime::from_string(&value, "")?.to_string(dateformat);
        }

        Ok(value)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::string CmdEdit::formatDuration (Task& task, const
    /// std::string& attribute)`.
    pub(crate) fn format_duration(task: &Task, attribute: &str) -> String {
        let mut value = task.get(attribute);
        if !value.is_empty() {
            value = Duration::from_string(&value).format_iso();
        }

        value
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::string CmdEdit::formatTask (Task task, const std::string&
    /// dateformat)`: the editable text representation of `task`.
    ///
    /// # Errors
    /// Date parse errors of stored values, storage errors of the dependency
    /// lookups, an invalid stored status.
    pub(crate) fn format_task(ctx: &mut Context, task: &Task, dateformat: &str) -> Result<String> {
        let mut before = String::new();
        let verbose = ctx.verbose("edit");

        if verbose {
            before.push_str(
                "# The 'task <id> edit' command allows you to modify all aspects of a task\n\
                 # using a text editor.  Below is a representation of all the task details.\n\
                 # Modify what you wish, and when you save and quit your editor,\n\
                 # Taskwarrior will read this file, determine what changed, and apply\n\
                 # those changes.  If you exit your editor without saving or making\n\
                 # modifications, Taskwarrior will do nothing.\n\
                 #\n\
                 # Lines that begin with # represent data you cannot change, like ID.\n\
                 # If you get too creative with your editing, Taskwarrior will send you\n\
                 # back to the editor to try again.\n\
                 #\n\
                 # Should you find yourself in an endless loop, re-editing the same file,\n\
                 # just quit the editor without making any changes.  Taskwarrior will\n\
                 # notice this and stop the editing.\n\
                 #\n",
            );
        }

        before.push_str("# Name               Editable details\n");
        before.push_str("# -----------------  ----------------------------------------------------\n");
        before.push_str(&format!("# ID:                {}\n", task.id));
        before.push_str(&format!("# UUID:              {}\n", task.get("uuid")));
        before.push_str(&format!(
            "# Status:            {}\n",
            Lexer::uc_first(Task::status_to_text(task.get_status()?))
        ));
        before.push_str(&format!("# Mask:              {}\n", task.get("mask")));
        before.push_str(&format!("# iMask:             {}\n", task.get("imask")));
        before.push_str(&format!("  Project:           {}\n", task.get("project")));

        if verbose {
            before.push_str("# Separate the tags with spaces, like this: tag1 tag2\n");
        }

        before.push_str(&format!("  Tags:              {}\n", join(" ", &task.get_tags())));
        before.push_str(&format!("  Description:       {}\n", task.get("description")));
        before.push_str(&format!(
            "  Created:           {}\n",
            Self::format_date(task, "entry", dateformat)?
        ));
        before.push_str(&format!(
            "  Started:           {}\n",
            Self::format_date(task, "start", dateformat)?
        ));
        before.push_str(&format!(
            "  Ended:             {}\n",
            Self::format_date(task, "end", dateformat)?
        ));
        before.push_str(&format!(
            "  Scheduled:         {}\n",
            Self::format_date(task, "scheduled", dateformat)?
        ));
        before.push_str(&format!(
            "  Due:               {}\n",
            Self::format_date(task, "due", dateformat)?
        ));
        before.push_str(&format!(
            "  Until:             {}\n",
            Self::format_date(task, "until", dateformat)?
        ));
        before.push_str(&format!("  Recur:             {}\n", task.get("recur")));
        before.push_str(&format!(
            "  Wait until:        {}\n",
            Self::format_date(task, "wait", dateformat)?
        ));
        before.push_str(&format!(
            "# Modified:          {}\n",
            Self::format_date(task, "modified", dateformat)?
        ));
        before.push_str(&format!("  Parent:            {}\n", task.get("parent")));

        if verbose {
            before.push_str(
                "# Annotations look like this: <date> -- <text> and there can be any number of them.\n\
                 # The ' -- ' separator between the date and text field should not be removed.\n\
                 # Multiline annotations need to be indented up to <date> (",
            );
            before.push_str(&(Self::ANNOTATION_EDIT_MARKER.len() - 1).to_string());
            before.push_str(
                " spaces).\n\
                 # A \"blank slot\" for adding an annotation follows for your convenience.\n",
            );
        }

        for (name, text) in task.get_annotations() {
            let dt = Datetime::from_epoch(strtoll(name.get(11..).unwrap_or("")));
            before.push_str(&format!(
                "  Annotation:        {} -- {}\n",
                dt.to_string(dateformat),
                str_replace(&text, "\n", Self::ANNOTATION_EDIT_MARKER)
            ));
        }

        let now = Datetime::new();
        before.push_str(&format!(
            "  Annotation:        {} -- \n",
            now.to_string(dateformat)
        ));

        // Add dependencies here.
        let dependencies = task.get_dependency_uuids();
        let mut all_deps = String::new();
        for (i, dependency) in dependencies.iter().enumerate() {
            if i != 0 {
                all_deps.push(',');
            }

            let t = ctx.tdb2.get_by_uuid(dependency)?.unwrap_or_default();
            let status = t.get_status()?;
            if status == Status::Pending || status == Status::Waiting {
                all_deps.push_str(&t.id.to_string());
            } else {
                all_deps.push_str(dependency);
            }
        }

        if verbose {
            before.push_str(
                "# Dependencies should be a comma-separated list of task IDs/UUIDs or ID ranges, \
                 with no spaces.\n",
            );
        }

        before.push_str(&format!("  Dependencies:      {all_deps}\n"));

        // UDAs
        let mut udas: Vec<String> = ctx
            .columns
            .keys()
            .filter(|name| !ctx.config.get(&format!("uda.{name}.type")).is_empty())
            .cloned()
            .collect();

        if !udas.is_empty() {
            before.push_str("# User Defined Attributes\n");
            udas.sort();
            for uda in &udas {
                // C++ `int pad = 13 - uda.length ()`.
                let pad = 13 - uda.len() as i64;
                let padding = if pad > 0 {
                    " ".repeat(pad as usize)
                } else {
                    String::new()
                };

                let type_ = ctx.config.get(&format!("uda.{uda}.type"));
                if type_ == "string" || type_ == "numeric" {
                    let mut value = task.get(uda);
                    if type_ == "string" {
                        value = json::encode(&value);
                    }
                    before.push_str(&format!("  UDA {uda}: {padding}{value}\n"));
                } else if type_ == "date" {
                    before.push_str(&format!(
                        "  UDA {uda}: {padding}{}\n",
                        Self::format_date(task, uda, dateformat)?
                    ));
                } else if type_ == "duration" {
                    before.push_str(&format!(
                        "  UDA {uda}: {padding}{}\n",
                        Self::format_duration(task, uda)
                    ));
                }
            }
        }

        // UDA orphans
        let mut orphans = task.get_uda_orphans(ctx);
        if !orphans.is_empty() {
            before.push_str("# User Defined Attribute Orphans\n");
            orphans.sort();
            for orphan in &orphans {
                // C++ `int pad = 6 - orphan.length ()`.
                let pad = 6 - orphan.len() as i64;
                let padding = if pad > 0 {
                    " ".repeat(pad as usize)
                } else {
                    String::new()
                };

                before.push_str(&format!(
                    "  UDA Orphan {orphan}: {padding}{}\n",
                    task.get(orphan)
                ));
            }
        }

        before.push_str("# End\n");
        Ok(before)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void CmdEdit::parseTask (Task& task, const std::string& after,
    /// const std::string& dateformat)`: apply what can be parsed out of the
    /// edited text to `task`. As in C++, an error leaves the changes made so
    /// far in `task`.
    ///
    /// # Errors
    /// The C++ thrown strings (as [`TwError::Message`]), date parse errors and
    /// dependency errors.
    pub(crate) fn parse_task(
        ctx: &mut Context,
        task: &mut Task,
        after: &str,
        dateformat: &str,
    ) -> Result<()> {
        // project
        let mut value = Self::find_value(after, "\n  Project:");
        if task.get("project") != value {
            if !value.is_empty() {
                ctx.footnote("Project modified.");
                task.set("project", &value);
            } else {
                ctx.footnote("Project deleted.");
                task.remove("project");
            }
        }

        // tags
        value = Self::find_value(after, "\n  Tags:");
        task.set_tags(&split(&value, ' '));

        // description.
        value = Self::find_multiline_value(after, "\n  Description:", "\n  Created:");
        if task.get("description") != value {
            if !value.is_empty() {
                ctx.footnote("Description modified.");
                task.set("description", &value);
            } else {
                return Err(TwError::Message("Cannot remove description.".into()));
            }
        }

        // entry
        value = Self::find_value(after, "\n  Created:");
        if !value.is_empty() {
            if value != Self::format_date(task, "entry", dateformat)? {
                ctx.footnote("Creation date modified.");
                task.set(
                    "entry",
                    &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                );
            }
        } else {
            return Err(TwError::Message("Cannot remove creation date.".into()));
        }

        // start
        value = Self::find_value(after, "\n  Started:");
        if !value.is_empty() {
            if !task.get("start").is_empty() {
                if value != Self::format_date(task, "start", dateformat)? {
                    ctx.footnote(STRING_EDIT_START_MOD);
                    task.set(
                        "start",
                        &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                    );
                }
            } else {
                ctx.footnote(STRING_EDIT_START_MOD);
                task.set(
                    "start",
                    &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                );
            }
        } else if !task.get("start").is_empty() {
            ctx.footnote("Start date removed.");
            task.remove("start");
        }

        // end
        value = Self::find_value(after, "\n  Ended:");
        if !value.is_empty() {
            if !task.get("end").is_empty() {
                if value != Self::format_date(task, "end", dateformat)? {
                    ctx.footnote("End date modified.");
                    task.set(
                        "end",
                        &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                    );
                }
            } else if task.get_status()? != Status::Deleted {
                return Err(TwError::Message(
                    "Cannot set a done date on a pending task.".into(),
                ));
            }
        } else if !task.get("end").is_empty() {
            ctx.footnote("End date removed.");
            task.set_status(Status::Pending);
            task.remove("end");
        }

        // scheduled
        value = Self::find_value(after, "\n  Scheduled:");
        if !value.is_empty() {
            if !task.get("scheduled").is_empty() {
                if value != Self::format_date(task, "scheduled", dateformat)? {
                    ctx.footnote(STRING_EDIT_SCHED_MOD);
                    task.set(
                        "scheduled",
                        &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                    );
                }
            } else {
                ctx.footnote(STRING_EDIT_SCHED_MOD);
                task.set(
                    "scheduled",
                    &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                );
            }
        } else if !task.get("scheduled").is_empty() {
            ctx.footnote("Scheduled date removed.");
            task.remove("scheduled");
        }

        // due
        value = Self::find_value(after, "\n  Due:");
        if !value.is_empty() {
            if !task.get("due").is_empty() {
                if value != Self::format_date(task, "due", dateformat)? {
                    ctx.footnote(STRING_EDIT_DUE_MOD);
                    task.set(
                        "due",
                        &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                    );
                }
            } else {
                ctx.footnote(STRING_EDIT_DUE_MOD);
                task.set(
                    "due",
                    &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                );
            }
        } else if !task.get("due").is_empty() {
            if task.get_status()? == Status::Recurring || !task.get("parent").is_empty() {
                ctx.footnote("Cannot remove a due date from a recurring task.");
            } else {
                ctx.footnote("Due date removed.");
                task.remove("due");
            }
        }

        // until
        value = Self::find_value(after, "\n  Until:");
        if !value.is_empty() {
            if !task.get("until").is_empty() {
                if value != Self::format_date(task, "until", dateformat)? {
                    ctx.footnote(STRING_EDIT_UNTIL_MOD);
                    task.set(
                        "until",
                        &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                    );
                }
            } else {
                ctx.footnote(STRING_EDIT_UNTIL_MOD);
                task.set(
                    "until",
                    &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                );
            }
        } else if !task.get("until").is_empty() {
            ctx.footnote("Until date removed.");
            task.remove("until");
        }

        // recur
        value = Self::find_value(after, "\n  Recur:");
        if value != task.get("recur") {
            if !value.is_empty() {
                let mut p = Duration::new();
                let mut idx = 0usize;
                if p.parse(&value, &mut idx) {
                    ctx.footnote("Recurrence modified.");
                    if !task.get("due").is_empty() {
                        task.set("recur", &value);
                        task.set_status(Status::Recurring);
                    } else {
                        return Err(TwError::Message(
                            "A recurring task must have a due date.".into(),
                        ));
                    }
                } else {
                    return Err(TwError::Message(
                        "Not a valid recurrence duration.".into(),
                    ));
                }
            } else {
                ctx.footnote("Recurrence removed.");
                task.set_status(Status::Pending);
                task.remove("recur");
                task.remove("until");
                task.remove("mask");
                task.remove("imask");
            }
        }

        // wait
        value = Self::find_value(after, "\n  Wait until:");
        if !value.is_empty() {
            if !task.get("wait").is_empty() {
                if value != Self::format_date(task, "wait", dateformat)? {
                    ctx.footnote(STRING_EDIT_WAIT_MOD);
                    task.set(
                        "wait",
                        &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                    );
                    task.set_status(Status::Waiting);
                }
            } else {
                ctx.footnote(STRING_EDIT_WAIT_MOD);
                task.set(
                    "wait",
                    &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                );
                task.set_status(Status::Waiting);
            }
        } else if !task.get("wait").is_empty() {
            ctx.footnote("Wait date removed.");
            task.remove("wait");
            task.set_status(Status::Pending);
        }

        // parent
        value = Self::find_value(after, "\n  Parent:");
        if value != task.get("parent") {
            if !value.is_empty() {
                ctx.footnote("Parent UUID modified.");
                task.set("parent", &value);
            } else {
                ctx.footnote("Parent UUID removed.");
                task.remove("parent");
            }
        }

        // Annotations
        let marker = Self::ANNOTATION_EDIT_MARKER;
        let bytes = after.as_bytes();
        let mut annotations: BTreeMap<String, String> = BTreeMap::new();
        let mut search = 0usize;
        while let Some(position) = find_from(after, "\n  Annotation:", search, true) {
            let found = position + 14; // Length of "\n  Annotation:".
            search = found;

            let mut eol = Some(found);
            while let Some(e) = eol {
                eol = find_from(after, "\n", e + 1, true);
                match eol {
                    Some(e) => {
                        let end = (e + marker.len()).min(bytes.len());
                        if &bytes[e..end] != marker.as_bytes() {
                            break;
                        }
                    }
                    None => break,
                }
            }

            if let Some(eol) = eol {
                let value = Lexer::trim_with(
                    &str_replace(&after[found..eol], marker, "\n"),
                    "\t ",
                );
                if let Some(gap) = find_from(&value, " -- ", 0, true) {
                    // TODO keeping the initial dates even if dateformat approximates them
                    // is complex as finding the correspondence between each original line
                    // and edited line may be impossible (bug #705). It would be simpler if
                    // each annotation was put on a line with a distinguishable id (then
                    // for each line: if the annotation is the same, then it is copied; if
                    // the annotation is modified, then its original date may be kept; and
                    // if there is no corresponding id, then a new unique date is created).
                    let when = Datetime::from_string(&value[..gap], dateformat)?;

                    // If the map already contains an annotation for a given timestamp
                    // we need to increment until we find an unused key
                    let mut timestamp = when.to_epoch() as i32;

                    let mut name;
                    loop {
                        name = format!("annotation_{timestamp}");
                        timestamp = timestamp.wrapping_add(1);
                        if !annotations.contains_key(&name) {
                            break;
                        }
                    }

                    let text = Lexer::trim_with(&value[gap + 4..], "\t ");
                    annotations.entry(name).or_insert(text);
                }
            }
        }

        task.set_annotations(&annotations);

        // Dependencies
        value = Self::find_value(after, "\n  Dependencies:");
        let dependencies = split(&value, ',');

        for dep in task.get_dependency_uuids() {
            task.remove_dependency(&dep)?;
        }
        for dep in &dependencies {
            if dep.len() >= 7 {
                task.add_dependency(ctx, dep)?;
            } else {
                task.add_dependency_id(ctx, strtol(dep))?;
            }
        }

        // UDAs
        let columns: Vec<String> = ctx.columns.keys().cloned().collect();
        for col in &columns {
            let type_ = ctx.config.get(&format!("uda.{col}.type"));
            if !type_.is_empty() {
                let mut value = Self::find_value(after, &format!("\n  UDA {col}:"));
                if type_ == "string" {
                    value = json::decode(&value);
                }
                if task.get(col) != value
                    && (type_ != "date"
                        || task.get(col)
                            != Datetime::from_string(&value, dateformat)?.to_epoch_string())
                    && (type_ != "duration"
                        || task.get(col) != Duration::from_string(&value).to_string())
                {
                    if !value.is_empty() {
                        ctx.footnote(&format("UDA {1} modified.", &[col]));

                        if type_ == "string" {
                            task.set(col, &value);
                        } else if type_ == "numeric" {
                            let mut pig = Pig::new(&value);
                            let mut d = 0.0f64;
                            if pig.get_number_f64(&mut d) && pig.eos() {
                                task.set(col, &value);
                            } else {
                                return Err(TwError::Message(format(
                                    "The value '{1}' is not a valid numeric value.",
                                    &[&value],
                                )));
                            }
                        } else if type_ == "date" {
                            task.set(
                                col,
                                &Datetime::from_string(&value, dateformat)?.to_epoch_string(),
                            );
                        } else if type_ == "duration" {
                            task.set(col, &Duration::from_string(&value).to_time_t().to_string());
                        }
                    } else {
                        ctx.footnote(&format("UDA {1} deleted.", &[col]));
                        task.remove(col);
                    }
                }
            }
        }

        // UDA orphans
        for orphan in Self::find_values(after, "\n  UDA Orphan ") {
            if let Some(colon) = orphan.find(':') {
                let name = Lexer::trim_with(&orphan[..colon], "\t ");
                let value = Lexer::trim_with(&orphan[colon + 1..], "\t ");

                if !value.is_empty() {
                    task.set(&name, &value);
                } else {
                    task.remove(&name);
                }
            }
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `CmdEdit::editResult CmdEdit::editFile (Task& task)`.
    ///
    /// # Errors
    /// `Your data.location directory is not writable.`, `Task is already
    /// being edited.`, and errors of `formatTask` and of non-message errors
    /// raised while parsing (the C++ only catches thrown strings).
    fn edit_file(ctx: &mut Context, task: &mut Task) -> Result<EditResult> {
        // Check for file permissions.
        let location = Directory::new(&ctx.config.get("data.location"));
        if !location.writable()? {
            return Err(TwError::Message(
                "Your data.location directory is not writable.".into(),
            ));
        }

        // Create a temp file name in data.location.
        let uuid = task.get("uuid");
        let file = format!("task.{}.task", uuid.get(..8).unwrap_or(&uuid));

        // Determine the output date format, which uses a hierarchy of definitions.
        //   rc.dateformat.edit
        //   rc.dateformat
        let mut dateformat = ctx.config.get("dateformat.edit");
        if dateformat.is_empty() {
            dateformat = ctx.config.get("dateformat");
        }

        // Change directory for the editor, doing nothing on error.
        let current_dir = Directory::cwd();
        chdir(&location.data);

        // Check if the file already exists, if so, bail out
        let filepath = Path::new(&file);
        if filepath.exists() {
            return Err(TwError::Message("Task is already being edited.".into()));
        }

        // Format the contents, T -> text, write to a file.
        let mut before = Self::format_task(ctx, task, &dateformat)?;
        let before_orig = before.clone();
        File::write_file(&file, &before);

        // Determine correct editor: .taskrc:editor > $VISUAL > $EDITOR > vi
        let mut editor = ctx.config.get("editor");
        if editor.is_empty() {
            if let Some(peditor) = env_string("VISUAL") {
                editor = peditor;
            }
        }
        if editor.is_empty() {
            if let Some(peditor) = env_string("EDITOR") {
                editor = peditor;
            }
        }
        if editor.is_empty() {
            editor = "vi".into();
        }

        // Complete the command line.
        editor.push(' ');
        editor.push_str(&format!("\"{file}\""));

        // C++ `ARE_THESE_REALLY_HARMFUL:` / `goto`.
        let changes = loop {
            // Launch the editor.
            print_stdout(&format("Launching '{1}' now...\n", &[&editor]));
            let (exitcode, captured_errno) = system(&editor);
            if exitcode == 0 {
                print_stdout("Editing complete.\n");
            } else {
                print_stdout(&format(
                    "Editing failed with exit code {1}.\n",
                    &[&exitcode],
                ));
                if exitcode == -1 {
                    print_stdout(&format!("{}\n", strerror(captured_errno)));
                }
                File::remove_file(&file);
                return Ok(EditResult::Error);
            }

            // Slurp file.
            let after = File::read_file(&file).unwrap_or_default();

            // Update task based on what can be parsed back out of the file, but only
            // if changes were made.
            if before_orig != after {
                print_stdout("Edits were detected.\n");

                // C++ catches only thrown strings; anything else propagates.
                let problem = match Self::parse_task(ctx, task, &after, &dateformat) {
                    Ok(()) => None,
                    Err(TwError::Message(e)) => Some(e),
                    Err(other) => return Err(other),
                };

                if let Some(problem) = problem {
                    let mut stderr = std::io::stderr();
                    let _ = stderr.write_all(format!("Error: {problem}\n").as_bytes());
                    let _ = stderr.flush();

                    File::remove_file(&file);

                    if confirm(
                        "Taskwarrior couldn't handle your edits.  Would you like to try again?",
                    ) {
                        // Preserve the edits.
                        before = after;
                        File::write_file(&file, &before);

                        continue;
                    }
                    break false;
                }
                break true;
            }

            print_stdout("No edits were detected.\n");
            break false;
        };

        // Cleanup.
        File::remove_file(&file);
        chdir(&current_dir);
        Ok(if changes {
            EditResult::Changes
        } else {
            EditResult::NoChanges
        })
    }
}

/// `std::cout << text` (flushed, so it precedes the editor's own output).
fn print_stdout(text: &str) {
    with_console(|_, stdout| {
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    });
}

/// C++ `getenv (name)`: `None` when unset.
fn env_string(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

/// C++ `chdir (path.c_str ())`, ignoring errors (and, as `c_str ()` does,
/// stopping at an embedded NUL).
fn chdir(path: &str) {
    let path = c_string(path);
    // SAFETY: `path` is a valid NUL-terminated string.
    unsafe {
        libc::chdir(path.as_ptr());
    }
}

/// `std::string::c_str ()`: the text up to the first NUL.
fn c_string(text: &str) -> CString {
    let end = text.find('\0').unwrap_or(text.len());
    CString::new(&text[..end]).unwrap_or_default()
}

/// C++ `int exitcode = system (command); auto captured_errno = errno;`:
/// runs `command` through `/bin/sh -c`, returning the raw wait status (or
/// -1) and `errno`.
fn system(command: &str) -> (i32, i32) {
    let command = c_string(command);
    // SAFETY: `command` is a valid NUL-terminated string.
    let exitcode = unsafe { libc::system(command.as_ptr()) };
    let captured_errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    (exitcode, captured_errno)
}

/// C++ `std::strerror (errnum)`.
fn strerror(errnum: i32) -> String {
    // SAFETY: strerror returns a pointer to a NUL-terminated static string.
    unsafe {
        std::ffi::CStr::from_ptr(libc::strerror(errnum))
            .to_string_lossy()
            .into_owned()
    }
}

impl Default for CmdEdit {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdEdit {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `int CmdEdit::execute (std::string&)`: introducing the Silver
    /// Bullet. Edits each filtered task in turn, storing the changed ones.
    fn execute(&self, context: &mut Context, _output: &mut String) -> Result<i32> {
        // Filter the tasks.
        let mut filtered: Vec<Task> = Vec::new();
        Filter::new().subset(context, &mut filtered)?;

        if filtered.is_empty() {
            context.footnote("No matches.");
            return Ok(1);
        }

        // C++ `unsigned int bulk = config.getInteger ("bulk")`.
        let bulk = context.config.get_integer("bulk") as u32;

        // If we are editing more than "bulk" tasks, ask for confirmation.
        // Bulk = 0 denotes infinite bulk.
        if filtered.len() > bulk as usize
            && bulk != 0
            && !confirm(&format(
                "Do you wish to manually edit {1} tasks?",
                &[&filtered.len()],
            ))
        {
            return Ok(2);
        }

        // Find number of matching tasks.
        for task in &mut filtered {
            let result = Self::edit_file(context, task)?;
            if result == EditResult::Error {
                break;
            } else if result == EditResult::Changes {
                tdb2::modify(context, task)?;
            }
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::CommandFixture;

    const UUID1: &str = "a1b2c3d4-0000-4000-8000-000000000001";
    const UUID2: &str = "a1b2c3d4-0000-4000-8000-000000000002";

    fn seed() -> Vec<String> {
        vec![
            format!(
                r#"{{"uuid":"{UUID1}","description":"first","status":"pending","entry":"1700000000","project":"home","tags":["a","b"],"due":"1700100000","annotation_1700000500":"note one","ustr":"hi \"there\"","unum":"42","orph":"x"}}"#
            ),
            format!(
                r#"{{"uuid":"{UUID2}","description":"second","status":"pending","entry":"1700000000"}}"#
            ),
        ]
    }

    const RC: &str = "uda.ustr.type=string\nuda.unum.type=numeric\nuda.udate.type=date\n\
                      uda.udur.type=duration\ndateformat=Y-M-D H:N:S\n";

    fn fixture(rc: &str) -> CommandFixture {
        let seed = seed();
        let refs: Vec<&str> = seed.iter().map(String::as_str).collect();
        CommandFixture::new(&refs, rc, &["1", "edit"])
    }

    #[test]
    fn find_value_variants() {
        let text = "x\n  Project:  home \t\n  Tags: a b\n";
        assert_eq!(CmdEdit::find_value(text, "\n  Project:"), "home");
        assert_eq!(CmdEdit::find_value(text, "\n  Tags:"), "a b");
        assert_eq!(CmdEdit::find_value(text, "\n  Due:"), "");
        // No newline after the value: not found.
        assert_eq!(CmdEdit::find_value("\n  Due: x", "\n  Due:"), "");
        assert_eq!(
            CmdEdit::find_multiline_value(
                "\n  Description: a\nb \\\n  Created: x\n",
                "\n  Description:",
                "\n  Created:"
            ),
            "a\nb"
        );
        assert_eq!(
            CmdEdit::find_values(
                "#\n  UDA Orphan a: 1\n  UDA Orphan b: 2\n",
                "\n  UDA Orphan "
            ),
            vec!["a: 1".to_string(), "b: 2".to_string()]
        );
        // C++ quirk: the search starts at offset 1, so a match at 0 is missed.
        assert_eq!(
            CmdEdit::find_values("\n  UDA Orphan a: 1\n  UDA Orphan b: 2\n", "\n  UDA Orphan "),
            vec!["b: 2".to_string()]
        );
    }

    #[test]
    fn format_task_layout() {
        let mut f = fixture(RC);
        let task = f.task(UUID1);
        let text = CmdEdit::format_task(&mut f.ctx, &task, "Y-M-D H:N:S").unwrap();
        assert!(text.starts_with("# Name               Editable details\n"));
        assert!(text.contains(&format!("# UUID:              {UUID1}\n")));
        assert!(text.contains("# Status:            Pending\n"));
        assert!(text.contains("  Project:           home\n"));
        assert!(text.contains("  Tags:              a b\n"));
        assert!(text.contains("  Description:       first\n"));
        assert!(text.contains("  Annotation:        "));
        assert!(text.contains(" -- note one\n"));
        assert!(text.contains("# User Defined Attributes\n"));
        assert!(text.contains("  UDA unum:          42\n"));
        assert!(text.contains("  UDA ustr:          hi \\\"there\\\"\n"));
        assert!(text.contains("  UDA udate:         \n"));
        assert!(text.contains("# User Defined Attribute Orphans\n"));
        assert!(text.contains("  UDA Orphan orph:   x\n"));
        assert!(text.ends_with("  Dependencies:      \n# User Defined Attributes\n  UDA priority:      \n  UDA udate:         \n  UDA udur:          \n  UDA unum:          42\n  UDA ustr:          hi \\\"there\\\"\n# User Defined Attribute Orphans\n  UDA Orphan orph:   x\n# End\n"));
    }

    #[test]
    fn format_parse_round_trip_is_identity() {
        let mut f = fixture(RC);
        let original = f.task(UUID1);
        let mut task = original.clone();
        let text = CmdEdit::format_task(&mut f.ctx, &task, "Y-M-D H:N:S").unwrap();
        CmdEdit::parse_task(&mut f.ctx, &mut task, &text, "Y-M-D H:N:S").unwrap();
        assert_eq!(task.get("project"), "home");
        assert_eq!(task.get("description"), "first");
        assert_eq!(task.get("due"), "1700100000");
        assert_eq!(task.get_tags(), original.get_tags());
        assert_eq!(task.get_annotations(), original.get_annotations());
        assert_eq!(task.get("ustr"), "hi \"there\"");
        assert_eq!(task.get("unum"), "42");
        assert_eq!(task.get("orph"), "x");
        assert!(f.ctx.footnotes.is_empty(), "{:?}", f.ctx.footnotes);
    }

    #[test]
    fn parse_applies_edits() {
        let mut f = fixture(RC);
        let mut task = f.task(UUID1);
        let text = CmdEdit::format_task(&mut f.ctx, &task, "Y-M-D H:N:S").unwrap();
        let edited = text
            .replace("  Project:           home\n", "  Project:           work\n")
            .replace("  Tags:              a b\n", "  Tags:              c\n")
            .replace("  Description:       first\n", "  Description:       changed\n")
            .replace("  UDA unum:          42\n", "  UDA unum:          7\n")
            .replace("  UDA udur:          \n", "  UDA udur:          P1D\n")
            .replace("  UDA Orphan orph:   x\n", "  UDA Orphan orph:   \n")
            .replace(
                "  Dependencies:      \n",
                "  Dependencies:      2\n",
            );
        CmdEdit::parse_task(&mut f.ctx, &mut task, &edited, "Y-M-D H:N:S").unwrap();
        assert_eq!(task.get("project"), "work");
        assert_eq!(task.get_tags(), vec!["c".to_string()]);
        assert_eq!(task.get("description"), "changed");
        assert_eq!(task.get("unum"), "7");
        assert_eq!(task.get("udur"), "86400");
        assert!(!task.has("orph"));
        assert_eq!(task.get_dependency_uuids(), vec![UUID2.to_string()]);
        let notes = f.ctx.footnotes.clone();
        assert_eq!(
            notes,
            vec![
                "Project modified.",
                "Description modified.",
                "UDA udur modified.",
                "UDA unum modified.",
            ]
        );
    }

    #[test]
    fn parse_errors_are_cpp_strings() {
        let mut f = fixture(RC);
        let task = f.task(UUID1);
        let text = CmdEdit::format_task(&mut f.ctx, &task, "Y-M-D H:N:S").unwrap();

        let cases = [
            ("  Description:       first\n", "  Description:       \n", "Cannot remove description."),
            ("  UDA unum:          42\n", "  UDA unum:          4x\n", "The value '4x' is not a valid numeric value."),
            ("  Recur:             \n", "  Recur:             bogus\n", "Not a valid recurrence duration."),
        ];
        for (from, to, message) in cases {
            let mut t = task.clone();
            let err = CmdEdit::parse_task(&mut f.ctx, &mut t, &text.replace(from, to), "Y-M-D H:N:S")
                .unwrap_err();
            assert_eq!(err.to_string(), message);
        }

        // Ended on a pending task.
        let ended = text.replacen("  Ended:             \n", "  Ended:             2023-11-14 00:00:00\n", 1);
        let mut t = task.clone();
        let err = CmdEdit::parse_task(&mut f.ctx, &mut t, &ended, "Y-M-D H:N:S").unwrap_err();
        assert_eq!(err.to_string(), "Cannot set a done date on a pending task.");
    }

    #[test]
    fn multiline_annotations_round_trip() {
        let mut f = fixture(RC);
        let mut task = f.task(UUID2);
        task.set("annotation_1700000600", "line one\nline two");
        let text = CmdEdit::format_task(&mut f.ctx, &task, "Y-M-D H:N:S").unwrap();
        assert!(text.contains(&format!(" -- line one{}line two\n", CmdEdit::ANNOTATION_EDIT_MARKER)));
        CmdEdit::parse_task(&mut f.ctx, &mut task, &text, "Y-M-D H:N:S").unwrap();
        let annotations = task.get_annotations();
        assert_eq!(annotations.len(), 1);
        assert_eq!(
            annotations.get("annotation_1700000600").map(String::as_str),
            Some("line one\nline two")
        );
    }

    #[test]
    fn verbose_edit_adds_help_comments() {
        let mut f = fixture(&format!("{RC}verbose=edit\n"));
        let task = f.task(UUID2);
        let text = CmdEdit::format_task(&mut f.ctx, &task, "Y-M-D").unwrap();
        assert!(text.starts_with(
            "# The 'task <id> edit' command allows you to modify all aspects of a task\n"
        ));
        assert!(text.contains("# Multiline annotations need to be indented up to <date> (21 spaces).\n"));
        assert!(text.contains("# Separate the tags with spaces, like this: tag1 tag2\n"));
        assert!(text.contains(
            "# Dependencies should be a comma-separated list of task IDs/UUIDs or ID ranges, with no spaces.\n"
        ));
    }
}
