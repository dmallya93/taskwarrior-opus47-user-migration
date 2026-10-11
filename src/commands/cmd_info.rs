//! Rust translation of `src/commands/CmdInfo.cpp`.
//!
//! `task <filter> information`: the Name/Value table of every attribute of
//! each selected task, its urgency breakdown and its journal (derived from
//! the taskchampion operations of the task).

use crate::commands::command::{Category, Command, CommandMeta};
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::filter::Filter;
use crate::lexer::Lexer;
use crate::operation::Operation;
use crate::rules;
use crate::shared::color::Color;
use crate::shared::datetime::Datetime;
use crate::shared::duration::Duration;
use crate::shared::format::{format, format_float, right_justify};
use crate::shared::shared::join;
use crate::shared::table::Table;
use crate::task::Task;
use crate::tdb2::uuid_from_string;
use crate::util::{optional_blank_line, set_header_underline};
use crate::variant::{Type as VariantType, Variant};

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdInfo` (`information`).
pub struct CmdInfo {
    meta: CommandMeta,
}

impl CmdInfo {
    /// C++ `CmdInfo::CmdInfo`.
    pub fn new() -> Self {
        CmdInfo {
            meta: CommandMeta {
                keyword: "information".into(),
                usage: "task <filter> information".into(),
                description: "Shows all data and metadata".into(),
                read_only: true,
                displays_id: false,
                needs_gc: false,
                needs_recur_update: false,
                uses_context: false,
                accepts_filter: true,
                accepts_modifications: false,
                accepts_miscellaneous: false,
                category: Category::Metadata,
                ..CommandMeta::default()
            },
        }
    }
}

impl Default for CmdInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for CmdInfo {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// C++ `CmdInfo::execute`: footnote "No matches." and return 1 when the
    /// filter selects nothing; otherwise render, per task, the Name/Value
    /// table, the urgency details (when the urgency is not 0) and the journal
    /// (with `rc.journal.info`).
    fn execute(&self, context: &mut Context, output: &mut String) -> Result<i32> {
        let mut rc = 0;

        // Apply filter.
        let mut filter = Filter::new();
        let mut filtered: Vec<Task> = Vec::new();
        filter.subset(context, &mut filtered)?;

        if filtered.is_empty() {
            context.footnote("No matches.");
            rc = 1;
        }

        // Determine the output date format, which uses a hierarchy of definitions.
        //   rc.dateformat.info
        //   rc.dateformat
        let mut dateformat = context.config.get("dateformat.info");
        if dateformat.is_empty() {
            dateformat = context.config.get("dateformat");
        }

        let mut dateformatanno = context.config.get("dateformat.annotation");
        if dateformatanno.is_empty() {
            dateformatanno = dateformat.clone();
        }

        // Render each task.
        let mut out = String::new();
        for task in &mut filtered {
            let mut view = Table::new();
            let width = context.get_width();
            view.width(width);
            if context.config.get_boolean("obfuscate") {
                view.obfuscate();
            }
            let color = context.color();
            view.with_color(color);
            view.add("Name");
            view.add("Value");
            set_header_underline(context, &mut view)?;

            let now = Datetime::new();

            // id
            let mut row = view.add_row();
            view.set(row, 0, "ID");
            view.set(
                row,
                1,
                &if task.id != 0 {
                    task.id.to_string()
                } else {
                    "-".to_string()
                },
            );

            let status = Lexer::uc_first(Task::status_to_text(task.get_status()?));

            // description
            let mut c = Color::default();
            rules::auto_colorize(context, &*task, &mut c)?;
            let mut description = task.get("description");
            if context.config.get_boolean("annotation.info") {
                let indent = context.config.get_integer("indent.annotation");

                for (key, value) in task.get_annotations() {
                    description.push('\n');
                    description.push_str(&" ".repeat(indent.max(0) as usize));
                    description.push_str(
                        &Datetime::from_string(&key[11..], "")?.to_string(&dateformatanno),
                    );
                    description.push(' ');
                    description.push_str(&value);
                }
            }

            if task.has("description") {
                row = view.add_row();
                view.set(row, 0, "Description");
                view.set_colored(row, 1, &description, c);
            }

            // status
            row = view.add_row();
            view.set(row, 0, "Status");
            view.set(row, 1, &status);

            // project
            if task.has("project") {
                row = view.add_row();
                view.set(row, 0, "Project");
                view.set(row, 1, &task.get("project"));
            }

            // dependencies: blocked
            {
                let blocked = task.get_dependency_tasks(&mut context.tdb2)?;
                if !blocked.is_empty() {
                    let mut message = String::new();
                    for block in &blocked {
                        message.push_str(&format!(
                            "{} {}\n",
                            block.id,
                            block.get_ref("description")
                        ));
                    }

                    row = view.add_row();
                    view.set(row, 0, "This task blocked by");
                    view.set(row, 1, &message);
                }
            }

            // dependencies: blocking
            {
                let blocking = task.get_blocked_tasks(&mut context.tdb2)?;
                if !blocking.is_empty() {
                    let mut message = String::new();
                    for block in &blocking {
                        message.push_str(&format!(
                            "{} {}\n",
                            block.id,
                            block.get_ref("description")
                        ));
                    }

                    row = view.add_row();
                    view.set(row, 0, "This task is blocking");
                    view.set(row, 1, &message);
                }
            }

            // recur, parent, mask, imask (the latter three deprecated in
            // 2.6.0), template, last, rtype.
            for (name, label) in [
                ("recur", "Recurrence"),
                ("parent", "Parent task"),
                ("mask", "Mask"),
                ("imask", "Mask Index"),
                ("template", "Template task"),
                ("last", "Last instance"),
                ("rtype", "Recurrence type"),
            ] {
                if task.has(name) {
                    row = view.add_row();
                    view.set(row, 0, label);
                    view.set(row, 1, &task.get(name));
                }
            }

            // entry
            if task.has("entry") && task.get_date("entry") != 0 {
                row = view.add_row();
                view.set(row, 0, "Entered");
                let dt = Datetime::from_epoch(task.get_date("entry"));
                let entry = dt.to_string(&dateformat);

                let mut age = String::new();
                let created = task.get("entry");
                if !created.is_empty() {
                    let dt = Datetime::from_epoch(c_strtoll(&created));
                    age =
                        Duration::from_seconds(now.to_epoch() - dt.to_epoch()).format_vague(false);
                }

                view.set(row, 1, &format!("{entry} ({age})"));
            }

            let valid_date = |task: &Task, prop: &str| task.has(prop) && task.get_date(prop) != 0;

            // wait, scheduled, start, due, end, until
            for (name, label) in [
                ("wait", "Waiting until"),
                ("scheduled", "Scheduled"),
                ("start", "Start"),
                ("due", "Due"),
                ("end", "End"),
                ("until", "Until"),
            ] {
                if valid_date(task, name) {
                    row = view.add_row();
                    view.set(row, 0, label);
                    view.set(
                        row,
                        1,
                        &Datetime::from_epoch(task.get_date(name)).to_string(&dateformat),
                    );
                }
            }

            // modified
            if valid_date(task, "modified") {
                row = view.add_row();
                view.set(row, 0, "Last modified");

                let modified = Datetime::from_epoch(task.get_date("modified"));
                let age = Duration::from_seconds(now.to_epoch() - modified.to_epoch())
                    .format_vague(false);
                view.set(
                    row,
                    1,
                    &format!("{} ({age})", modified.to_string(&dateformat)),
                );
            }

            // tags ...
            let tags = task.get_tags();
            if !tags.is_empty() {
                let all_tags = join(" ", &tags);

                row = view.add_row();
                view.set(row, 0, "Tags");
                view.set(row, 1, &all_tags);
            }

            // Virtual tags.
            {
                // Note: This list must match that in Task::hasTag.
                // Note: This list must match that in ::feedback_reserved_tags.
                let mut virtual_tags = String::new();
                for tag in VIRTUAL_TAGS {
                    if task.has_tag(tag, context)? {
                        virtual_tags.push_str(tag);
                        virtual_tags.push(' ');
                    }
                }

                row = view.add_row();
                view.set(row, 0, "Virtual tags");
                view.set(row, 1, &virtual_tags);
            }

            // uuid
            row = view.add_row();
            view.set(row, 0, "UUID");
            let uuid = task.get("uuid");
            view.set(row, 1, &uuid);

            // Task::urgency
            let urgency = task.urgency(context)?;
            row = view.add_row();
            view.set(row, 0, "Urgency");
            view.set(row, 1, &Lexer::trim_left(&format_float(urgency, 4, 4)));

            // Show any UDAs
            let all = task.all();
            for att in &all {
                if let Some(col) = context.columns.get(att).cloned() {
                    if col.is_uda() {
                        let mut value = task.get(att);
                        if !value.is_empty() {
                            row = view.add_row();
                            view.set(row, 0, col.label());

                            if col.type_() == "date" {
                                value = Datetime::from_string(&value, "")?.to_string(&dateformat);
                            } else if col.type_() == "duration" {
                                let mut iso = Duration::new();
                                let mut cursor = 0usize;
                                if iso.parse(&value, &mut cursor) {
                                    value =
                                        Variant::from_time(iso.to_time_t(), VariantType::Duration)?
                                            .to_string();
                                } else {
                                    value = "PT0S".to_string();
                                }
                            }

                            view.set(row, 1, &value);
                        }
                    }
                }
            }

            // Show any orphaned UDAs, which are identified by not being represented in
            // the context.columns map.
            for att in &all {
                if !att.starts_with("annotation_")
                    && !att.starts_with("tag_")
                    && !att.starts_with("dep_")
                    && !context.columns.contains_key(att)
                {
                    row = view.add_row();
                    view.set(row, 0, &format!("[{att}"));
                    view.set(row, 1, &format!("{}]", task.get_ref(att)));
                }
            }

            // Create a second table, containing urgency details, if necessary.
            let mut urgency_details = Table::new();
            if urgency != 0.0 {
                set_header_underline(context, &mut urgency_details)?;
                if context.color() {
                    let alternate = Color::parse(&context.config.get("color.alternate"))?;
                    urgency_details.color_odd(alternate);
                    urgency_details.intra_color_odd(alternate);
                }

                if context.config.get_boolean("obfuscate") {
                    urgency_details.obfuscate();
                }
                urgency_details.with_color(context.config.get_boolean("color"));

                urgency_details.width(context.get_width());
                urgency_details.add(""); // Attribute
                urgency_details.add(""); // Value
                urgency_details.add(""); // *
                urgency_details.add(""); // Coefficient
                urgency_details.add(""); // =
                urgency_details.add(""); // Result

                let ud = &mut urgency_details;
                urgency_term(
                    ud,
                    "project",
                    task.urgency_project(),
                    Task::urgency_project_coefficient(),
                );
                urgency_term(
                    ud,
                    "active",
                    task.urgency_active(),
                    Task::urgency_active_coefficient(),
                );
                urgency_term(
                    ud,
                    "scheduled",
                    task.urgency_scheduled(),
                    Task::urgency_scheduled_coefficient(),
                );
                urgency_term(
                    ud,
                    "waiting",
                    task.urgency_waiting(),
                    Task::urgency_waiting_coefficient(),
                );
                urgency_term(
                    ud,
                    "blocked",
                    task.urgency_blocked(),
                    Task::urgency_blocked_coefficient(),
                );
                urgency_term(
                    ud,
                    "blocking",
                    task.urgency_blocking(),
                    Task::urgency_blocking_coefficient(),
                );
                urgency_term(
                    ud,
                    "annotations",
                    task.urgency_annotations(),
                    Task::urgency_annotations_coefficient(),
                );
                urgency_term(
                    ud,
                    "tags",
                    task.urgency_tags(),
                    Task::urgency_tags_coefficient(),
                );
                urgency_term(
                    ud,
                    "due",
                    task.urgency_due(),
                    Task::urgency_due_coefficient(),
                );
                urgency_term(
                    ud,
                    "age",
                    task.urgency_age(),
                    Task::urgency_age_coefficient(),
                );

                // Tag, Project- and UDA-specific coefficients.
                for (key, coefficient) in Task::coefficients() {
                    if substr(&key, 0, 13) == "urgency.user." {
                        // urgency.user.project.<project>.coefficient
                        if substr(&key, 13, 8) == "project." {
                            if let Some(end) = key.find(".coefficient") {
                                let project = substr(&key, 21, end.wrapping_sub(21));
                                let task_project_name = task.get("project");
                                if task_project_name == project
                                    || task_project_name.starts_with(&format!("{project}."))
                                {
                                    urgency_term(
                                        ud,
                                        &format!("PROJECT {project}"),
                                        1.0,
                                        coefficient,
                                    );
                                }
                            }
                        }

                        // urgency.user.tag.<tag>.coefficient
                        if substr(&key, 13, 4) == "tag." {
                            if let Some(end) = key.find(".coefficient") {
                                let name = substr(&key, 17, end.wrapping_sub(17));
                                if task.has_tag(name, context)? {
                                    urgency_term(ud, &format!("TAG {name}"), 1.0, coefficient);
                                }
                            }
                        }

                        // urgency.user.keyword.<keyword>.coefficient
                        if substr(&key, 13, 8) == "keyword." {
                            if let Some(end) = key.find(".coefficient") {
                                let keyword = substr(&key, 21, end.wrapping_sub(21));
                                if task.get_ref("description").contains(keyword) {
                                    urgency_term(
                                        ud,
                                        &format!("KEYWORD {keyword}"),
                                        1.0,
                                        coefficient,
                                    );
                                }
                            }
                        }
                    }
                    // urgency.uda.<name>.coefficient
                    else if substr(&key, 0, 12) == "urgency.uda." {
                        // urgency.uda.<name>.coefficient
                        // urgency.uda.<name>.<value>.coefficient
                        if let Some(end) = key.find(".coefficient") {
                            let uda = substr(&key, 12, end.wrapping_sub(12));
                            match uda.find('.') {
                                None => {
                                    // urgency.uda.<name>.coefficient
                                    if task.has(uda) {
                                        urgency_term(ud, &format!("UDA {uda}"), 1.0, coefficient);
                                    }
                                }
                                Some(dot) => {
                                    // urgency.uda.<name>.<value>.coefficient
                                    if task.get_ref(&uda[..dot]) == &uda[dot + 1..] {
                                        urgency_term(ud, &format!("UDA {uda}"), 1.0, coefficient);
                                    }
                                }
                            }
                        }
                    }
                }

                row = urgency_details.add_row();
                urgency_details.set(row, 5, &right_justify("------", 6));
                row = urgency_details.add_row();
                urgency_details.set(row, 5, &right_justify(&format_float(urgency, 4, 4), 6));
            }

            // Create a third table, containing undo log change details.
            let mut journal = Table::new();
            set_header_underline(context, &mut journal)?;

            if context.config.get_boolean("obfuscate") {
                journal.obfuscate();
            }
            journal.with_color(context.config.get_boolean("color"));

            journal.width(context.get_width());
            journal.add("Date");
            journal.add("Modification");

            if context.config.get_boolean("journal.info") {
                let tcuuid = uuid_from_string(&uuid)?;
                let tcoperations = context.tdb2.get_task_operations(tcuuid)?;
                let mut operations = Operation::operations(&tcoperations);

                // Sort by type (Create < Update < Delete < UndoPoint) and then by timestamp.
                cpp_sort(&mut operations, |a, b| a.lt(b));

                let mut last_timestamp: i64 = 0;
                let mut i = 0usize;
                while i < operations.len() {
                    let op = &operations[i];

                    // Only display updates -- creation and deletion aren't interesting.
                    if !op.is_update() {
                        i += 1;
                        continue;
                    }

                    // Group operations that occur within 1s of this one. This is a heuristic
                    // for operations performed in the same `task` invocation, and allows e.g.,
                    // `task done end:-2h` to take the updated `end` value into account. It also
                    // groups these events into a single "row" of the table for better layout.
                    let group_start = i;
                    i += 1;
                    while i < operations.len() {
                        let op2 = &operations[i];
                        if !op2.is_update() || op2.get_timestamp() - op.get_timestamp() > 1 {
                            break;
                        }
                        i += 1;
                    }
                    let group_end = i;
                    // C++ `i--` followed by the loop's `i++` leaves `i` at `group_end`.

                    let msg = format_for_info(
                        &operations,
                        group_start,
                        group_end,
                        &dateformat,
                        &mut last_timestamp,
                        context,
                    )?;

                    let Some(msg) = msg else {
                        continue;
                    };

                    let row = journal.add_row();
                    let timestamp = Datetime::from_epoch(operations[group_start].get_timestamp());
                    journal.set(row, 0, &timestamp.to_string(&dateformat));
                    journal.set(row, 1, &msg);
                }
            }

            out.push_str(optional_blank_line(context));
            out.push_str(&view.render());
            out.push('\n');

            if urgency_details.rows() > 0 {
                out.push_str(&urgency_details.render());
                out.push('\n');
            }

            if journal.rows() > 0 {
                out.push_str(&journal.render());
                out.push('\n');
            }
        }

        *output = out;
        Ok(rc)
    }
}

/// The virtual tags shown by `info`, in C++ order.
/// If you update this list, update src/task.rs and src/commands/cmd_tags.rs as well.
const VIRTUAL_TAGS: [&str; 32] = [
    "ACTIVE",
    "ANNOTATED",
    "BLOCKED",
    "BLOCKING",
    "CHILD", // 2017-01-07: Deprecated in 2.6.0
    "COMPLETED",
    "DELETED",
    "DUE",
    "DUETODAY", // 2016-03-29: Deprecated in 2.6.0
    "INSTANCE",
    "LATEST",
    "MONTH",
    "ORPHAN",
    "OVERDUE",
    "PARENT", // 2017-01-07: Deprecated in 2.6.0
    "PENDING",
    "PRIORITY",
    "PROJECT",
    "QUARTER",
    "READY",
    "SCHEDULED",
    "TAGGED",
    "TEMPLATE",
    "TODAY",
    "TOMORROW",
    "UDA",
    "UNBLOCKED",
    "UNTIL",
    "WAITING",
    "WEEK",
    "YEAR",
    "YESTERDAY",
];

/// C `strtoll (value.c_str (), nullptr, 10)`.
fn c_strtoll(text: &str) -> i64 {
    let Ok(text) = std::ffi::CString::new(text) else {
        // An interior NUL ends the C string; parse the prefix.
        let prefix = text.split('\0').next().unwrap_or("");
        return c_strtoll(prefix);
    };
    // SAFETY: `text` is a valid NUL-terminated C string for the duration of
    // the call, and a null `endptr` is allowed.
    unsafe { libc::strtoll(text.as_ptr(), std::ptr::null_mut(), 10) }
}

/// C++ `std::string::substr (pos, len)` for `pos <= size ()` (all callers
/// guarantee it via a preceding prefix check): `len` is clamped to the rest
/// of the string, so a wrapped-around `npos`-like length takes everything.
/// Byte-based; returns `""` if the cut would split a UTF-8 sequence (never
/// the case for the ASCII prefixes compared here).
fn substr(s: &str, pos: usize, len: usize) -> &str {
    let pos = pos.min(s.len());
    let end = pos.saturating_add(len).min(s.len());
    s.get(pos..end).unwrap_or("")
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdInfo::urgencyTerm`: one row `label measure * coefficient = value`
/// of the urgency details table, omitted when the product is 0.
fn urgency_term(view: &mut Table, label: &str, measure: f32, coefficient: f32) {
    let value = measure * coefficient;
    if value != 0.0 {
        let row = view.add_row();
        view.set(row, 0, &format!("    {label}"));
        view.set(row, 1, &right_justify(&format_float(measure, 5, 3), 6));
        view.set(row, 2, "*");
        view.set(row, 3, &right_justify(&format_float(coefficient, 4, 2), 4));
        view.set(row, 4, "=");
        view.set(row, 5, &right_justify(&format_float(value, 5, 3), 6));
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `CmdInfo::formatForInfo`: the journal text for the update operations
/// `operations[group_start..group_end]`, or `None` when nothing in the group
/// is worth displaying. `last_start` carries the most recent start time
/// across groups, for the duration shown when `start` is deleted.
///
/// # Errors
/// C++ throws (uncaught here) when a changed `start` value is not a date.
/// When `start` is deleted in a group that also deletes `end`, C++
/// dereferences an empty `std::optional` (`std::bad_optional_access`, which
/// reaches `catch (...)`); that is [`TwError::Runtime`] here.
fn format_for_info(
    operations: &[Operation],
    group_start: usize,
    group_end: usize,
    dateformat: &str,
    last_start: &mut i64,
    context: &Context,
) -> Result<Option<String>> {
    let render = |prop: &str, value: &str| {
        crate::feedback::render_attribute(
            &context.columns,
            &context.config,
            prop,
            value,
            dateformat,
        )
    };

    let mut out = String::new();
    for operation in &operations[group_start..group_end] {
        debug_assert!(operation.is_update());

        // Extract the parts of the Update operation.
        let prop = operation.get_property();
        let value = operation.get_value();
        let old_value = operation.get_old_value();
        let timestamp = Datetime::from_epoch(operation.get_timestamp());

        // Never care about modifying the modification time, or the legacy properties `depends` and
        // `tags`.
        if prop == "modified" || prop == "depends" || prop == "tags" {
            continue;
        }

        // Handle property deletions
        if let (None, Some(old_value)) = (&value, &old_value) {
            if Task::is_annotation_attr(&prop) {
                out.push_str(&format("Annotation '{1}' deleted.\n", &[old_value]));
            } else if Task::is_tag_attr(&prop) {
                out.push_str(&format("Tag '{1}' deleted.\n", &[&Task::attr2tag(&prop)]));
            } else if Task::is_dep_attr(&prop) {
                out.push_str(&format(
                    "Dependency on '{1}' deleted.\n",
                    &[&Task::attr2dep(&prop)],
                ));
            } else if prop == "start" {
                let started = Datetime::from_epoch(*last_start);
                let mut stopped = timestamp;

                // If any update in this group sets the `end` property, use that instead of the
                // timestamp deleting the `start` property as the stop time.
                // See https://github.com/GothenburgBitFactory/taskwarrior/issues/2514
                for op in &operations[group_start..group_end] {
                    debug_assert!(op.is_update());
                    if op.get_property() == "end" {
                        let Some(end) = op.get_value() else {
                            return Err(TwError::Runtime("bad optional access".into()));
                        };
                        stopped = match Datetime::from_string(&end, "") {
                            Ok(d) => d,
                            // Fall back to the 'start' timestamp if its value is un-parseable.
                            Err(_) => Datetime::from_epoch(op.get_timestamp()),
                        };
                    }
                }

                out.push_str(&format(
                    "{1} deleted (duration: {2}).",
                    &[
                        &Lexer::uc_first(&prop),
                        &Duration::from_seconds(stopped.to_epoch() - started.to_epoch()).format(),
                    ],
                ));
                out.push('\n');
            } else {
                out.push_str(&format("{1} deleted.\n", &[&Lexer::uc_first(&prop)]));
            }
        }

        // Handle property additions.
        if let (Some(value), None) = (&value, &old_value) {
            if Task::is_annotation_attr(&prop) {
                out.push_str(&format("Annotation of '{1}' added.\n", &[value]));
            } else if Task::is_tag_attr(&prop) {
                out.push_str(&format("Tag '{1}' added.\n", &[&Task::attr2tag(&prop)]));
            } else if Task::is_dep_attr(&prop) {
                out.push_str(&format(
                    "Dependency on '{1}' added.\n",
                    &[&Task::attr2dep(&prop)],
                ));
            } else {
                // Record the last start time for later duration calculation.
                if prop == "start" {
                    // Invalid dates are ignored.
                    if let Ok(d) = Datetime::from_string(value, "") {
                        *last_start = d.to_epoch();
                    }
                }

                out.push_str(&format(
                    "{1} set to '{2}'.",
                    &[&Lexer::uc_first(&prop), &render(&prop, value)],
                ));
                out.push('\n');
            }
        }

        // Handle property changes.
        if let (Some(value), Some(old_value)) = (&value, &old_value) {
            if Task::is_tag_attr(&prop) || Task::is_dep_attr(&prop) {
                // Dependencies and tags do not have meaningful values.
            } else if Task::is_annotation_attr(&prop) {
                out.push_str(&format("Annotation changed to '{1}'.\n", &[value]));
            } else {
                // Record the last start time for later duration calculation.
                if prop == "start" {
                    *last_start = Datetime::from_string(value, "")?.to_epoch();
                }

                out.push_str(&format(
                    "{1} changed from '{2}' to '{3}'.",
                    &[
                        &Lexer::uc_first(&prop),
                        &render(&prop, old_value),
                        &render(&prop, value),
                    ],
                ));
                out.push('\n');
            }
        }
    }

    if out.is_empty() {
        return Ok(None);
    }

    Ok(Some(out))
}

////////////////////////////////////////////////////////////////////////////////
// libstdc++ `std::sort` (introsort + final insertion sort, `_S_threshold`
// 16). C++ sorts the operations with `std::sort`, which is not stable:
// updates in the same second compare equal and may be reordered. This port
// reproduces libstdc++'s exact element movements so the journal lists
// same-second updates in the same order as C++.

const S_THRESHOLD: usize = 16;

/// C++ `std::sort (first, last, comp)` (libstdc++).
pub(crate) fn cpp_sort<T, F: Fn(&T, &T) -> bool>(v: &mut [T], less: F) {
    let n = v.len();
    if n > 1 {
        let lg = (usize::BITS - 1 - n.leading_zeros()) as usize;
        introsort_loop(v, 0, n, lg * 2, &less);
        final_insertion_sort(v, 0, n, &less);
    }
}

fn introsort_loop<T, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut last: usize,
    mut depth: usize,
    less: &F,
) {
    while last - first > S_THRESHOLD {
        if depth == 0 {
            heap_sort(&mut v[first..last], less);
            return;
        }
        depth -= 1;
        let cut = unguarded_partition_pivot(v, first, last, less);
        introsort_loop(v, cut, last, depth, less);
        last = cut;
    }
}

fn unguarded_partition_pivot<T, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &F,
) -> usize {
    let mid = first + (last - first) / 2;
    move_median_to_first(v, first, first + 1, mid, last - 1, less);
    unguarded_partition(v, first + 1, last, first, less)
}

fn move_median_to_first<T, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    result: usize,
    a: usize,
    b: usize,
    c: usize,
    less: &F,
) {
    let pick = if less(&v[a], &v[b]) {
        if less(&v[b], &v[c]) {
            b
        } else if less(&v[a], &v[c]) {
            c
        } else {
            a
        }
    } else if less(&v[a], &v[c]) {
        a
    } else if less(&v[b], &v[c]) {
        c
    } else {
        b
    };
    v.swap(result, pick);
}

fn unguarded_partition<T, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    mut first: usize,
    mut last: usize,
    pivot: usize,
    less: &F,
) -> usize {
    loop {
        while less(&v[first], &v[pivot]) {
            first += 1;
        }
        last -= 1;
        while less(&v[pivot], &v[last]) {
            last -= 1;
        }
        if first >= last {
            return first;
        }
        v.swap(first, last);
        first += 1;
    }
}

fn final_insertion_sort<T, F: Fn(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &F,
) {
    if last - first > S_THRESHOLD {
        insertion_sort(v, first, first + S_THRESHOLD, less);
        for i in first + S_THRESHOLD..last {
            unguarded_linear_insert(v, i, less);
        }
    } else {
        insertion_sort(v, first, last, less);
    }
}

fn insertion_sort<T, F: Fn(&T, &T) -> bool>(v: &mut [T], first: usize, last: usize, less: &F) {
    if first == last {
        return;
    }
    for i in first + 1..last {
        if less(&v[i], &v[first]) {
            // move_backward (first, i, i + 1); *first = val;
            v[first..=i].rotate_right(1);
        } else {
            unguarded_linear_insert(v, i, less);
        }
    }
}

fn unguarded_linear_insert<T, F: Fn(&T, &T) -> bool>(v: &mut [T], mut last: usize, less: &F) {
    // Equivalent element moves: shift `val` left while it is less than its
    // predecessor.
    while less(&v[last], &v[last - 1]) {
        v.swap(last, last - 1);
        last -= 1;
    }
}

/// libstdc++ `std::__partial_sort (first, last, last)`: `__make_heap` then
/// `__sort_heap`.
fn heap_sort<T, F: Fn(&T, &T) -> bool>(v: &mut [T], less: &F) {
    let len = v.len();
    if len < 2 {
        return;
    }
    // __make_heap
    let mut parent = (len - 2) / 2;
    loop {
        adjust_heap(v, parent, len, less);
        if parent == 0 {
            break;
        }
        parent -= 1;
    }
    // __sort_heap
    let mut last = len;
    while last > 1 {
        last -= 1;
        // __pop_heap (first, last, last): value = *last; *last = *first;
        // __adjust_heap (first, 0, last - first, value).
        v.swap(0, last);
        adjust_heap(v, 0, last, less);
    }
}

/// libstdc++ `std::__adjust_heap` with the value initially at `hole`,
/// followed by `__push_heap`.
fn adjust_heap<T, F: Fn(&T, &T) -> bool>(v: &mut [T], mut hole: usize, len: usize, less: &F) {
    let top = hole;
    // Move the value out by tracking its position: the moves below shift
    // children up; the value ends at the final hole, then sifts up.
    let mut second = hole;
    while second < (len.saturating_sub(1)) / 2 {
        second = 2 * (second + 1);
        if less(&v[second], &v[second - 1]) {
            second -= 1;
        }
        v.swap(hole, second);
        hole = second;
    }
    if len & 1 == 0 && len >= 2 && second == (len - 2) / 2 {
        second = 2 * (second + 1);
        v.swap(hole, second - 1);
        hole = second - 1;
    }
    // __push_heap (first, hole, top, value)
    let mut parent = hole.wrapping_sub(1) / 2;
    while hole > top && less(&v[parent], &v[hole]) {
        v.swap(hole, parent);
        hole = parent;
        parent = hole.wrapping_sub(1) / 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpp_sort_sorts() {
        for n in [0usize, 1, 2, 5, 16, 17, 40, 200] {
            let mut v: Vec<u32> = (0..n as u32).map(|i| (i * 7919 + 13) % 101).collect();
            let mut expected = v.clone();
            expected.sort();
            cpp_sort(&mut v, |a, b| a < b);
            assert_eq!(v, expected, "{n}");
        }
    }

    #[test]
    fn heap_sort_sorts() {
        let mut v: Vec<u32> = (0..57u32).map(|i| (i * 31 + 7) % 23).collect();
        let mut expected = v.clone();
        expected.sort();
        heap_sort(&mut v, &|a: &u32, b: &u32| a < b);
        assert_eq!(v, expected);
    }

    #[test]
    fn cpp_sort_small_is_stable_like_insertion_sort() {
        // <= 16 elements: plain insertion sort keeps equal elements in order.
        let mut v: Vec<(u32, u32)> = (0..16u32).map(|i| (i % 3, i)).collect();
        cpp_sort(&mut v, |a, b| a.0 < b.0);
        let mut expected = v.clone();
        expected.sort_by_key(|e| e.0);
        assert_eq!(v, expected);
    }

    #[test]
    fn substr_clamps_like_cpp() {
        assert_eq!(
            substr("urgency.user.tag.coefficient", 17, 16usize.wrapping_sub(17)),
            "coefficient"
        );
        assert_eq!(
            substr("urgency.user.tag.foo.coefficient", 17, 20 - 17),
            "foo"
        );
        assert_eq!(substr("abc", 0, 13), "abc");
    }

    fn update(
        property: &str,
        value: Option<&str>,
        old_value: Option<&str>,
        timestamp: i64,
    ) -> Operation {
        use taskchampion::chrono::{TimeZone, Utc};
        Operation::from(taskchampion::Operation::Update {
            uuid: taskchampion::Uuid::from_u128(1),
            property: property.into(),
            old_value: old_value.map(Into::into),
            value: value.map(Into::into),
            timestamp: Utc.timestamp_opt(timestamp, 0).unwrap(),
        })
    }

    fn info_context() -> Context {
        let mut context = Context::new();
        context.config.set("dateformat", "Y-M-D");
        crate::columns::column::factory_all(&mut context.columns, &context.config).unwrap();
        context
    }

    #[test]
    fn format_for_info_messages() {
        let context = info_context();
        let ops = vec![
            update("modified", Some("5"), None, 100),
            update("tags", Some("a"), None, 100),
            update("description", Some("new"), Some("old"), 100),
            update("annotation_1", Some("note"), None, 100),
            update("annotation_2", Some("b"), Some("a"), 100),
            update("annotation_3", None, Some("gone"), 100),
            update("tag_x", Some("x"), None, 100),
            update("tag_y", None, Some("x"), 100),
            update("tag_z", Some("x"), Some("x"), 100),
            update("dep_abc", Some("x"), None, 100),
            update("dep_def", None, Some("x"), 100),
            update("due", Some("129600"), None, 100),
            update("due", Some("216000"), Some("129600"), 100),
            update("project", None, Some("P"), 100),
        ];
        let mut last = 0;
        let msg = format_for_info(&ops, 0, ops.len(), "", &mut last, &context)
            .unwrap()
            .unwrap();
        assert_eq!(
            msg,
            "Description changed from 'old' to 'new'.\n\
             Annotation of 'note' added.\n\
             Annotation changed to 'b'.\n\
             Annotation 'gone' deleted.\n\
             Tag 'x' added.\n\
             Tag 'y' deleted.\n\
             Dependency on 'abc' added.\n\
             Dependency on 'def' deleted.\n\
             Due set to '1970-01-02'.\n\
             Due changed from '1970-01-02' to '1970-01-03'.\n\
             Project deleted.\n"
        );

        // Only ignored properties: nothing to show.
        let ops = vec![
            update("modified", Some("5"), None, 100),
            update("depends", Some("x"), None, 100),
        ];
        assert_eq!(
            format_for_info(&ops, 0, 2, "", &mut last, &context).unwrap(),
            None
        );
    }

    #[test]
    fn format_for_info_start_duration() {
        let context = info_context();
        let mut last = 0;
        let start = vec![update("start", Some("1700049600"), None, 1700049600)];
        let msg = format_for_info(&start, 0, 1, "Y-M-D", &mut last, &context)
            .unwrap()
            .unwrap();
        assert_eq!(msg, "Start set to '2023-11-15'.\n");
        assert_eq!(last, 1700049600);

        // Stop: duration from the last start to the deleting timestamp.
        let stop = vec![update("start", None, Some("1700049600"), 1700053200)];
        let msg = format_for_info(&stop, 0, 1, "Y-M-D", &mut last, &context)
            .unwrap()
            .unwrap();
        assert_eq!(msg, "Start deleted (duration: 1:00:00).\n");

        // An `end` set in the same group is the stop time (#2514).
        let done = vec![
            update("start", None, Some("1700049600"), 1700060000),
            update("end", Some("1700049660"), None, 1700060000),
        ];
        let msg = format_for_info(&done, 0, 2, "Y-M-D", &mut last, &context)
            .unwrap()
            .unwrap();
        assert_eq!(
            msg,
            "Start deleted (duration: 0:01:00).\nEnd set to '2023-11-15'.\n"
        );

        // An `end` deleted in the same group: C++ std::bad_optional_access.
        let bad = vec![
            update("start", None, Some("1700049600"), 1700060000),
            update("end", None, Some("1700049660"), 1700060000),
        ];
        assert!(matches!(
            format_for_info(&bad, 0, 2, "", &mut last, &context),
            Err(TwError::Runtime(_))
        ));
    }

    #[test]
    fn urgency_term_rows() {
        let mut t = Table::new();
        for _ in 0..6 {
            t.add("");
        }
        urgency_term(&mut t, "project", 1.0, 0.0);
        assert_eq!(t.rows(), 0);
        urgency_term(&mut t, "project", 1.0, 1.0);
        assert_eq!(t.rows(), 1);
    }
}
