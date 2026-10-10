//! Rust translation of `src/columns/Column.h` / `src/columns/Column.cpp`.
//!
//! Holds the [`Column`] trait (C++ `class Column`), the shared member state
//! [`ColumnBase`] (the C++ protected members and `Column::Column ()`), the
//! narrow environment [`ModifyEnv`] that `Column::modify` implementations
//! use in place of `Context::getContext ()`, and the factories
//! [`factory`] (`Column::factory (name, report)`), [`factory_all`]
//! (`Column::factory (std::map&)`), [`uda_all`] (`Column::uda (std::map&)`)
//! and [`uda`] (`Column::uda (name)`).
//!
//! # Not yet ported: rendering (milestone 6)
//!
//! The C++ rendering interface is deliberately absent from the trait until
//! milestone 6 ports reports: `measure (const std::string&, ...)`,
//! `measure (Task&, ...)`, `renderHeader`, both `render` overloads and the
//! protected helpers `renderInteger`, `renderDouble`, `renderStringLeft`
//! and `renderStringRight`. The member state those use (`_report`,
//! `_hyphenate`, `_indent`, `_dateformat`, ...) is already carried by the
//! structs so the milestone-6 port only adds methods.
//!
//! # Hierarchy
//!
//! C++ inheritance becomes composition: every column type struct
//! (`ColumnTypeString`, `ColumnTypeDate`, ...) owns a [`ColumnBase`] as
//! `base`; every derived column owns its C++ base class as `parent`
//! (`ColumnDue { parent: ColumnTypeDate }`, `ColumnUDAUUID { parent:
//! ColumnUUID }`). A virtual call made from a base-class method (C++
//! `ColumnTypeString::modify` calling the virtual `validate`) is ported as an
//! associated function taking `this: &dyn Column`, so the most-derived
//! override is used exactly as in C++.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::rc::Rc;

use crate::columns::col_depends::ColumnDepends;
use crate::columns::col_description::ColumnDescription;
use crate::columns::col_due::ColumnDue;
use crate::columns::col_end::ColumnEnd;
use crate::columns::col_entry::ColumnEntry;
use crate::columns::col_id::ColumnID;
use crate::columns::col_imask::ColumnIMask;
use crate::columns::col_last::ColumnLast;
use crate::columns::col_mask::ColumnMask;
use crate::columns::col_modified::ColumnModified;
use crate::columns::col_parent::ColumnParent;
use crate::columns::col_project::ColumnProject;
use crate::columns::col_recur::ColumnRecur;
use crate::columns::col_rtype::ColumnRType;
use crate::columns::col_scheduled::ColumnScheduled;
use crate::columns::col_start::ColumnStart;
use crate::columns::col_status::ColumnStatus;
use crate::columns::col_tags::ColumnTags;
use crate::columns::col_template::ColumnTemplate;
use crate::columns::col_uda::{ColumnUDADate, ColumnUDADuration, ColumnUDANumeric, ColumnUDAString, ColumnUDAUUID};
use crate::columns::col_until::ColumnUntil;
use crate::columns::col_urgency::ColumnUrgency;
use crate::columns::col_uuid::ColumnUUID;
use crate::columns::col_wait::ColumnWait;
use crate::context::Context;
use crate::error::{Result, TwError};
use crate::eval::{dom_source, Eval, FnResolver};
use crate::shared::configuration::Configuration;
use crate::shared::format::format;
use crate::shared::shared::split;
use crate::task::Task;
use crate::variant::Variant;

/// C++ `std::map<std::string, Column*>` as held by `Context::columns`.
pub type ColumnRegistry = BTreeMap<String, Rc<dyn Column>>;

/// The debug prefix every C++ `modify` builds as `std::string label = "  \033[1;37;43mMODIFICATION\033[0m ";`
/// (ColTypeString/Date/Duration/Numeric, ColProject, ColTags, ColRecur and
/// `Task::modify`).
pub const MODIFICATION_LABEL: &str = "  \x1b[1;37;43mMODIFICATION\x1b[0m ";

////////////////////////////////////////////////////////////////////////////////
/// The C++ `Column` protected members, initialised as `Column::Column ()`
/// does; each column constructor then overrides some of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnBase {
    /// C++ `_name`.
    pub name: String,
    /// C++ `_type`.
    pub type_: String,
    /// C++ `_style`.
    pub style: String,
    /// C++ `_label`.
    pub label: String,
    /// C++ `_report`.
    pub report: String,
    /// C++ `_modifiable`.
    pub modifiable: bool,
    /// C++ `_uda`.
    pub uda: bool,
    /// C++ `_fixed_width`.
    pub fixed_width: bool,
    /// C++ `_styles`.
    pub styles: Vec<String>,
    /// C++ `_examples`.
    pub examples: Vec<String>,
}

impl Default for ColumnBase {
    fn default() -> Self {
        ColumnBase::new()
    }
}

impl ColumnBase {
    /// C++ `Column::Column ()`.
    pub fn new() -> Self {
        ColumnBase {
            name: String::new(),
            type_: "string".to_string(),
            style: "default".to_string(),
            label: String::new(),
            report: String::new(),
            modifiable: true,
            uda: false,
            fixed_width: false,
            styles: Vec::new(),
            examples: Vec::new(),
        }
    }

    /// C++ `void Column::setStyle (const std::string& style)` (the
    /// non-virtual base implementation that overrides call first).
    ///
    /// # Errors
    /// `Unrecognized column format '{1}.{2}'` when `style` is neither
    /// `default` nor one of `_styles`.
    pub fn set_style(&mut self, style: &str) -> Result<()> {
        if style != "default" && !self.styles.iter().any(|s| s == style) {
            return Err(TwError::Message(format("Unrecognized column format '{1}.{2}'", &[&self.name, &style])));
        }

        self.style = style.to_string();
        Ok(())
    }
}

/// `std::vector<std::string>` from string literals.
pub(crate) fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// C++ `_label.substr (0, n)` on the (ASCII) default labels.
pub(crate) fn label_prefix(label: &str, n: usize) -> String {
    label.chars().take(n).collect()
}

////////////////////////////////////////////////////////////////////////////////
/// The parts of `Context::getContext ()` that the C++ `Column::modify`
/// implementations reach: Eval with `domSource`, `Context::debug`,
/// `feedback_special_tags` (ColumnTags) and the TDB2/Context-backed
/// dependency operations (ColumnDepends). [`Context`] implements it; tests
/// use a fake.
pub trait ModifyEnv {
    /// C++ `Eval e; e.addSource (domSource); e.evaluateInfixExpression (value, v);`
    /// The result is `v`; an error is whatever `evaluateInfixExpression`
    /// throws.
    fn evaluate_dom_expression(&mut self, value: &str) -> Result<Variant>;

    /// C++ `Context::getContext ().debug (message)`.
    fn debug(&mut self, message: &str);

    /// C++ `feedback_special_tags (task, tag)` (feedback.cpp).
    fn feedback_special_tags(&mut self, task: &Task, tag: &str);

    /// C++ `Context::getContext ().tdb2.get (uuid, task)`: a task by full or
    /// partial UUID, `None` when not found.
    fn get_task(&mut self, uuid: &str) -> Result<Option<Task>>;

    /// C++ `task.addDependency (uuid)` (needs the Context for the circular
    /// dependency check and footnotes).
    fn add_dependency(&mut self, task: &mut Task, uuid: &str) -> Result<()>;

    /// C++ `task.addDependency (id)`.
    fn add_dependency_id(&mut self, task: &mut Task, id: i32) -> Result<()>;

    /// C++ `task.removeDependency (id)`.
    fn remove_dependency_id(&mut self, task: &mut Task, id: i32) -> Result<()>;
}

/// C++ `getDOM (name, Context::getContext ().currentTask, value)` as used by
/// `domSource` when a column's `modify` evaluates its value.
///
/// Milestone 5 hook (DOM task): `src/dom.rs` is not ported yet, so no DOM
/// reference resolves here (C++ `getDOM` returning `false`). Literals,
/// operators and the named constants still evaluate exactly as in C++; only
/// values that are DOM references (`due:1.due`, `project:2.project`) differ
/// until the DOM port replaces this function with `dom::get`.
fn context_get_dom(_ctx: &Context, _name: &str) -> Option<Variant> {
    None
}

impl ModifyEnv for Context {
    /// Builds the C++ `Eval` with `domSource` (resolved through
    /// [`context_get_dom`]) and the current task, then forwards Eval's debug
    /// messages to `Context::debug`, as the C++ Eval writes them there.
    fn evaluate_dom_expression(&mut self, value: &str) -> Result<Variant> {
        let current = self.current_task.clone();
        let (result, messages) = {
            let ctx: &Context = self;
            let source = |name: &str| dom_source(name, |n| context_get_dom(ctx, n));
            let mut resolver = match current.as_deref() {
                Some(task) => FnResolver::with_task(source, task),
                None => FnResolver::new(source),
            };
            let result = Eval::new().evaluate_infix_expression(value, &mut resolver);
            (result, resolver.debug_messages)
        };
        for message in messages {
            self.debug(&message);
        }
        result
    }

    fn debug(&mut self, message: &str) {
        Context::debug(self, message);
    }

    fn feedback_special_tags(&mut self, task: &Task, tag: &str) {
        crate::feedback::feedback_special_tags(self, task, tag);
    }

    fn get_task(&mut self, uuid: &str) -> Result<Option<Task>> {
        self.tdb2.get_by_uuid(uuid)
    }

    fn add_dependency(&mut self, task: &mut Task, uuid: &str) -> Result<()> {
        task.add_dependency(self, uuid)
    }

    fn add_dependency_id(&mut self, task: &mut Task, id: i32) -> Result<()> {
        task.add_dependency_id(self, id)
    }

    fn remove_dependency_id(&mut self, task: &mut Task, id: i32) -> Result<()> {
        task.remove_dependency_id(self, id)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class Column`: column metadata and the non-rendering behaviour
/// (`validate`, `modify`, `setStyle`/`setLabel`/`setReport`). See the module
/// documentation for the rendering methods milestone 6 adds.
pub trait Column {
    /// The C++ protected members.
    fn base(&self) -> &ColumnBase;

    /// The C++ protected members, mutably.
    fn base_mut(&mut self) -> &mut ColumnBase;

    /// C++ `const std::string& name () const`.
    fn name(&self) -> &str {
        &self.base().name
    }

    /// C++ `const std::string& style () const`.
    fn style(&self) -> &str {
        &self.base().style
    }

    /// C++ `const std::string& label () const`.
    fn label(&self) -> &str {
        &self.base().label
    }

    /// C++ `const std::string& type () const`.
    fn type_(&self) -> &str {
        &self.base().type_
    }

    /// C++ `bool modifiable () const`.
    fn modifiable(&self) -> bool {
        self.base().modifiable
    }

    /// C++ `bool is_uda () const`.
    fn is_uda(&self) -> bool {
        self.base().uda
    }

    /// C++ `bool is_fixed_width () const`.
    fn is_fixed_width(&self) -> bool {
        self.base().fixed_width
    }

    /// C++ `std::vector<std::string> styles () const`.
    fn styles(&self) -> Vec<String> {
        self.base().styles.clone()
    }

    /// C++ `std::vector<std::string> examples () const`.
    fn examples(&self) -> Vec<String> {
        self.base().examples.clone()
    }

    /// C++ `virtual void setStyle (const std::string&)`. The overrides that
    /// link the label to the style read `rc.<x>.indicator`, hence `config`
    /// (C++ `Context::getContext ().config`).
    ///
    /// # Errors
    /// `Unrecognized column format '{1}.{2}'` (see [`ColumnBase::set_style`]).
    fn set_style(&mut self, style: &str, _config: &Configuration) -> Result<()> {
        self.base_mut().set_style(style)
    }

    /// C++ `virtual void setLabel (const std::string& value)`.
    fn set_label(&mut self, value: &str) {
        self.base_mut().label = value.to_string();
    }

    /// C++ `virtual void setReport (const std::string& value)`.
    fn set_report(&mut self, value: &str) {
        self.base_mut().report = value.to_string();
    }

    /// C++ `virtual bool validate (const std::string&) const { return false; }`.
    fn validate(&self, _input: &str) -> bool {
        false
    }

    /// C++ `virtual void modify (Task&, const std::string&) {}`.
    fn modify(&self, _task: &mut Task, _value: &str, _env: &mut dyn ModifyEnv) -> Result<()> {
        Ok(())
    }
}

/// Implements [`Column::base`] / [`Column::base_mut`] through a field path
/// (`base`, `parent.base`, `parent.parent.base`).
macro_rules! column_base {
    ($($field:ident).+) => {
        fn base(&self) -> &$crate::columns::column::ColumnBase {
            &self.$($field).+
        }

        fn base_mut(&mut self) -> &mut $crate::columns::column::ColumnBase {
            &mut self.$($field).+
        }
    };
}
pub(crate) use column_base;

////////////////////////////////////////////////////////////////////////////////
/// C++ `Column* Column::factory (const std::string& name, const std::string&
/// report)`: one column from a full `<type>[.<format>]` definition, with
/// its report and style set.
///
/// # Errors
/// `Unrecognized column name '{1}'.`, `Unrecognized column format
/// '{1}.{2}'`, and the [`uda`] type error.
///
/// Deviation: when `uda.<name>.type` exists but is empty, C++ `Column::uda`
/// returns `nullptr` and `factory` dereferences it (a crash); here that case
/// reports `Unrecognized column name '{1}'.`.
pub fn factory(name: &str, report: &str, config: &Configuration) -> Result<Box<dyn Column>> {
    // Decompose name into type and style.
    let (column_name, column_style) = match name.find('.') {
        Some(dot) => (&name[..dot], &name[dot + 1..]),
        None => (name, "default"),
    };

    let mut c: Box<dyn Column> = match column_name {
        "depends" => Box::new(ColumnDepends::new(config)),
        "description" => Box::new(ColumnDescription::new(config)),
        "due" => Box::new(ColumnDue::new(config)),
        "end" => Box::new(ColumnEnd::new(config)),
        "entry" => Box::new(ColumnEntry::new(config)),
        "id" => Box::new(ColumnID::new()),
        "imask" => Box::new(ColumnIMask::new()),
        "last" => Box::new(ColumnLast::new()),
        "mask" => Box::new(ColumnMask::new()),
        "modified" => Box::new(ColumnModified::new(config)),
        "parent" => Box::new(ColumnParent::new()),
        "project" => Box::new(ColumnProject::new(config)),
        "recur" => Box::new(ColumnRecur::new(config)),
        "rtype" => Box::new(ColumnRType::new()),
        "scheduled" => Box::new(ColumnScheduled::new(config)),
        "start" => Box::new(ColumnStart::new(config)),
        "status" => Box::new(ColumnStatus::new()),
        "tags" => Box::new(ColumnTags::new(config)),
        "template" => Box::new(ColumnTemplate::new()),
        "until" => Box::new(ColumnUntil::new(config)),
        "urgency" => Box::new(ColumnUrgency::new()),
        "uuid" => Box::new(ColumnUUID::new()),
        "wait" => Box::new(ColumnWait::new(config)),

        // UDA.
        _ if config.has(&format!("uda.{column_name}.type")) => match uda(column_name, config)? {
            Some(c) => c,
            None => {
                return Err(TwError::Message(format("Unrecognized column name '{1}'.", &[&column_name])));
            }
        },

        _ => return Err(TwError::Message(format("Unrecognized column name '{1}'.", &[&column_name]))),
    };

    c.set_report(report);
    c.set_style(column_style, config)?;
    Ok(c)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void Column::factory (std::map<std::string, Column*>& all)`: bulk
/// column instantiation (every built-in column, then [`uda_all`]).
///
/// # Errors
/// Those of [`uda_all`].
pub fn factory_all(all: &mut ColumnRegistry, config: &Configuration) -> Result<()> {
    let builtins: Vec<Rc<dyn Column>> = vec![
        Rc::new(ColumnDepends::new(config)),
        Rc::new(ColumnDescription::new(config)),
        Rc::new(ColumnDue::new(config)),
        Rc::new(ColumnEnd::new(config)),
        Rc::new(ColumnEntry::new(config)),
        Rc::new(ColumnID::new()),
        Rc::new(ColumnIMask::new()),
        Rc::new(ColumnLast::new()),
        Rc::new(ColumnMask::new()),
        Rc::new(ColumnModified::new(config)),
        Rc::new(ColumnParent::new()),
        Rc::new(ColumnProject::new(config)),
        Rc::new(ColumnRecur::new(config)),
        Rc::new(ColumnRType::new()),
        Rc::new(ColumnScheduled::new(config)),
        Rc::new(ColumnStart::new(config)),
        Rc::new(ColumnStatus::new()),
        Rc::new(ColumnTags::new(config)),
        Rc::new(ColumnTemplate::new()),
        Rc::new(ColumnUntil::new(config)),
        Rc::new(ColumnUrgency::new()),
        Rc::new(ColumnUUID::new()),
        Rc::new(ColumnWait::new(config)),
    ];
    for c in builtins {
        all.insert(c.name().to_string(), c);
    }

    uda_all(all, config)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void Column::uda (std::map<std::string, Column*>& all)`: one
/// `ColumnUDA*` for each distinct `<name>` among the `uda.<name>.*` keys.
///
/// # Errors
/// `The UDA named '{1}' is the same as a core attribute, and is not
/// permitted.`, and the [`uda`] type error.
pub fn uda_all(all: &mut ColumnRegistry, config: &Configuration) -> Result<()> {
    // For each UDA, instantiate and initialize ColumnUDA.
    let mut udas: BTreeSet<String> = BTreeSet::new();

    for key in config.keys() {
        if let Some(rest) = key.strip_prefix("uda.") {
            // One byte after the first '.'.
            if let Some(period) = rest.find('.') {
                udas.insert(rest[..period].to_string());
            }
        }
    }

    for name in &udas {
        if all.contains_key(name) {
            return Err(TwError::Message(format(
                "The UDA named '{1}' is the same as a core attribute, and is not permitted.",
                &[name],
            )));
        }

        if let Some(c) = uda(name, config)? {
            all.insert(c.name().to_string(), Rc::from(c));
        }
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `Column* Column::uda (const std::string& name)`: the `ColumnUDA*` for
/// `uda.<name>.type`, labelled `uda.<name>.label`, with the allowed values
/// `uda.<name>.values` (comma-separated; not for `uuid`). `None` (C++
/// `nullptr`) when the type is empty.
///
/// # Errors
/// `User defined attributes may only be of type 'string', 'uuid', 'date',
/// 'duration' or 'numeric'.` for any other type.
pub fn uda(name: &str, config: &Configuration) -> Result<Option<Box<dyn Column>>> {
    let type_ = config.get(&format!("uda.{name}.type"));
    let label = config.get(&format!("uda.{name}.label"));
    let values = config.get(&format!("uda.{name}.values"));

    let c: Box<dyn Column> = if type_ == "string" {
        let mut c = ColumnUDAString::new();
        c.parent.base.name = name.to_string();
        c.parent.base.label = label;
        if !values.is_empty() {
            c.values = split(&values, ',');
        }
        Box::new(c)
    } else if type_ == "numeric" {
        let mut c = ColumnUDANumeric::new();
        c.parent.base.name = name.to_string();
        c.parent.base.label = label;
        if !values.is_empty() {
            c.values = split(&values, ',');
        }
        Box::new(c)
    } else if type_ == "date" {
        let mut c = ColumnUDADate::new(config);
        c.parent.base.name = name.to_string();
        c.parent.base.label = label;
        if !values.is_empty() {
            c.values = split(&values, ',');
        }
        Box::new(c)
    } else if type_ == "duration" {
        let mut c = ColumnUDADuration::new();
        c.parent.base.name = name.to_string();
        c.parent.base.label = label;
        if !values.is_empty() {
            c.values = split(&values, ',');
        }
        Box::new(c)
    } else if type_ == "uuid" {
        let mut c = ColumnUDAUUID::new();
        c.parent.parent.base.name = name.to_string();
        c.parent.parent.base.label = label;
        Box::new(c)
    } else if !type_.is_empty() {
        return Err(TwError::Message(
            "User defined attributes may only be of type 'string', 'uuid', 'date', 'duration' or 'numeric'."
                .to_string(),
        ));
    } else {
        return Ok(None);
    };

    Ok(Some(c))
}

#[cfg(test)]
mod tests {
    //! Factory contents, style/label linking, `validate` and `modify` of
    //! every column class, against a fake [`ModifyEnv`].
    use super::*;
    use crate::shared::datetime::Datetime;
    use crate::shared::localtime;

    /// A [`ModifyEnv`] with a real Eval whose DOM resolves from `dom`, and
    /// recorders for debug output, special-tag feedback and dependency
    /// operations.
    #[derive(Default)]
    struct FakeEnv {
        dom: BTreeMap<String, Variant>,
        debug: Vec<String>,
        feedback: Vec<String>,
        tasks: Vec<Task>,
        added: Vec<String>,
        added_ids: Vec<i32>,
        removed_ids: Vec<i32>,
    }

    impl ModifyEnv for FakeEnv {
        fn evaluate_dom_expression(&mut self, value: &str) -> Result<Variant> {
            let dom = &self.dom;
            let mut resolver = FnResolver::new(|name: &str| dom_source(name, |n| dom.get(n).cloned()));
            Eval::new().evaluate_infix_expression(value, &mut resolver)
        }

        fn debug(&mut self, message: &str) {
            self.debug.push(message.to_string());
        }

        fn feedback_special_tags(&mut self, _task: &Task, tag: &str) {
            self.feedback.push(tag.to_string());
        }

        fn get_task(&mut self, uuid: &str) -> Result<Option<Task>> {
            Ok(self.tasks.iter().find(|t| t.get_ref("uuid").starts_with(uuid)).cloned())
        }

        fn add_dependency(&mut self, _task: &mut Task, uuid: &str) -> Result<()> {
            self.added.push(uuid.to_string());
            Ok(())
        }

        fn add_dependency_id(&mut self, _task: &mut Task, id: i32) -> Result<()> {
            self.added_ids.push(id);
            Ok(())
        }

        fn remove_dependency_id(&mut self, _task: &mut Task, id: i32) -> Result<()> {
            self.removed_ids.push(id);
            Ok(())
        }
    }

    fn config(pairs: &[(&str, &str)]) -> Configuration {
        let mut config = Configuration::new();
        for (key, value) in pairs {
            config.set(key, *value);
        }
        config
    }

    fn registry(config: &Configuration) -> ColumnRegistry {
        let mut all = ColumnRegistry::new();
        factory_all(&mut all, config).unwrap();
        all
    }

    fn err<T>(r: Result<T>) -> String {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    fn modify(column: &dyn Column, value: &str, env: &mut FakeEnv) -> Result<Task> {
        let mut task = Task::new();
        column.modify(&mut task, value, env)?;
        Ok(task)
    }

    fn label(text: &str) -> String {
        format!("{MODIFICATION_LABEL}{text}")
    }

    #[test]
    fn base_defaults_match_column_constructor() {
        let base = ColumnBase::new();
        assert_eq!(base.name, "");
        assert_eq!(base.type_, "string");
        assert_eq!(base.style, "default");
        assert_eq!(base.label, "");
        assert_eq!(base.report, "");
        assert!(base.modifiable && !base.uda && !base.fixed_width);
        assert!(base.styles.is_empty() && base.examples.is_empty());
        assert_eq!(MODIFICATION_LABEL.as_bytes()[2], 0x1b);
    }

    #[test]
    fn factory_all_builtin_metadata() {
        let all = registry(&Configuration::new());
        // name, type, style, label, modifiable, styles
        let expected: &[BuiltinRow] = &[
            ("depends", "string", "list", "Depends", true, &["list", "count", "indicator"]),
            (
                "description",
                "string",
                "combined",
                "Description",
                true,
                &["combined", "desc", "oneline", "truncated", "count", "truncated_count"],
            ),
            ("due", "date", "formatted", "Due", true, DATE_STYLES),
            ("end", "date", "formatted", "Completed", true, DATE_STYLES),
            ("entry", "date", "formatted", "Added", true, DATE_STYLES),
            ("id", "numeric", "number", "ID", false, &["number"]),
            ("imask", "numeric", "number", "Mask Index", false, &["number"]),
            ("last", "numeric", "number", "Last instance", false, &["number"]),
            ("mask", "string", "default", "Mask", false, &["default"]),
            ("modified", "date", "formatted", "Modified", true, DATE_STYLES),
            ("parent", "string", "long", "Parent task", false, &["long", "short"]),
            ("project", "string", "full", "Project", true, &["full", "parent", "indented"]),
            ("recur", "string", "duration", "Recur", true, &["duration", "indicator"]),
            ("rtype", "string", "default", "Recurrence type", false, &["default", "indicator"]),
            ("scheduled", "date", "formatted", "Scheduled", true, DATE_STYLES),
            (
                "start",
                "date",
                "formatted",
                "Started",
                true,
                &["formatted", "julian", "epoch", "iso", "age", "relative", "remaining", "countdown", "active"],
            ),
            ("status", "string", "long", "Status", true, &["long", "short"]),
            ("tags", "string", "list", "Tags", true, &["list", "indicator", "count"]),
            ("template", "string", "long", "Template task", false, &["long", "short"]),
            ("until", "date", "formatted", "Until", true, DATE_STYLES),
            ("urgency", "numeric", "real", "Urgency", false, &["real", "integer"]),
            ("uuid", "string", "long", "UUID", false, &["long", "short"]),
            ("wait", "date", "formatted", "Wait", true, DATE_STYLES),
        ];
        assert_eq!(all.len(), expected.len());
        for (name, type_, style, label, modifiable, styles) in expected {
            let c = &all[*name];
            assert_eq!(c.name(), *name);
            assert_eq!(c.type_(), *type_, "{name}");
            assert_eq!(c.style(), *style, "{name}");
            assert_eq!(c.label(), *label, "{name}");
            assert_eq!(c.modifiable(), *modifiable, "{name}");
            assert!(!c.is_uda(), "{name}");
            assert!(!c.is_fixed_width(), "{name}");
            assert_eq!(c.styles(), strings(styles), "{name}");
            assert_eq!(c.examples().len(), styles.len(), "{name}");
        }
    }

    /// name, type, style, label, modifiable, styles.
    type BuiltinRow = (&'static str, &'static str, &'static str, &'static str, bool, &'static [&'static str]);

    const DATE_STYLES: &[&str] = &["formatted", "julian", "epoch", "iso", "age", "relative", "remaining", "countdown"];

    #[test]
    fn constructor_examples() {
        let config = config(&[
            ("tag.indicator", "+"),
            ("dependency.indicator", "D"),
            ("recurrence.indicator", "R"),
            ("active.indicator", "*"),
            ("dateformat", "Y-M-D"),
        ]);
        let all = registry(&config);
        assert_eq!(all["tags"].examples(), strings(&["home @chore next", "+", "[2]"]));
        assert_eq!(all["depends"].examples(), strings(&["1 2 10", "[3]", "D"]));
        assert_eq!(all["recur"].examples(), strings(&["weekly", "R"]));
        assert_eq!(all["start"].examples()[8], "*");
        assert_eq!(all["project"].examples(), strings(&["home.garden", "home", "  home.garden"]));
        assert_eq!(all["urgency"].examples(), strings(&["4.6", "4"]));
        assert_eq!(all["uuid"].examples(), strings(&["f30cb9c3-3fc0-483f-bfb2-3bf134f00694", "f30cb9c3"]));

        let due = all["due"].examples();
        let now = Datetime::from_epoch(localtime::time_now() - 125);
        assert_eq!(due[0], now.to_string("Y-M-D"));
        assert_eq!(due[0].len(), 10);
        assert_eq!(due[6], "");
        assert!(due[5].starts_with('-'));
        assert_eq!(due[5][1..], due[4]);

        let description = all["description"].examples();
        assert_eq!(description[1], "Move your clothes down on to the lower peg");
        assert_eq!(description[3], "Move your clothes do...");
        assert_eq!(description[4], "Move your clothes down on to the lower peg [4]");
        assert_eq!(description[5], "Move your clothes do... [4]");
        assert!(description[0].starts_with("Move your clothes down on to the lower peg\n  "));
    }

    #[test]
    fn factory_single_column_styles_and_labels() {
        let config = config(&[("tag.indicator", "+"), ("dependency.indicator", "D"), ("recurrence.indicator", "RR")]);
        let cases = [
            // Without a format the style is "default", as in C++.
            ("due", "default", "Due"),
            ("due.countdown", "countdown", "Count"),
            ("due.age", "age", "Due"),
            ("scheduled.countdown", "countdown", "Count"),
            ("entry.age", "age", "Age"),
            ("start.active", "active", "A"),
            ("status.short", "short", "St"),
            ("tags.indicator", "indicator", "T"),
            ("tags.count", "count", "Tag"),
            ("depends.indicator", "indicator", "D"),
            ("depends.count", "count", "Dep"),
            ("recur.indicator", "indicator", "Re"),
            ("rtype.indicator", "indicator", ""),
            ("id.default", "default", "ID"),
            ("description.oneline", "oneline", "Description"),
        ];
        for (name, style, label) in cases {
            let c = factory(name, "next", &config).unwrap();
            assert_eq!(c.style(), style, "{name}");
            assert_eq!(c.label(), label, "{name}");
            assert_eq!(c.base().report, "next", "{name}");
        }

        // The C++ `name.style` decomposition splits at the first dot only.
        assert_eq!(err(factory("bogus", "", &config)), "Unrecognized column name 'bogus'.");
        assert_eq!(err(factory("bogus.list", "", &config)), "Unrecognized column name 'bogus'.");
        assert_eq!(err(factory("due.bogus", "", &config)), "Unrecognized column format 'due.bogus'");
        assert_eq!(err(factory("due.age.x", "", &config)), "Unrecognized column format 'due.age.x'");
        assert_eq!(err(factory("id.real", "", &config)), "Unrecognized column format 'id.real'");
    }

    #[test]
    fn factory_udas() {
        let config = config(&[
            ("uda.size.type", "string"),
            ("uda.size.label", "Size"),
            ("uda.size.values", "S,M,L"),
            ("uda.blank.type", ""),
            ("uda.odd.type", "list"),
        ]);
        let c = factory("size.indicator", "list", &config).unwrap();
        assert_eq!(c.name(), "size");
        assert_eq!(c.style(), "indicator");
        assert_eq!(c.label(), "Size");
        assert!(c.is_uda());
        assert_eq!(err(factory("size.long", "", &config)), "Unrecognized column format 'size.long'");
        // Deviation: C++ dereferences the null column.
        assert_eq!(err(factory("blank", "", &config)), "Unrecognized column name 'blank'.");
        assert_eq!(
            err(factory("odd", "", &config)),
            "User defined attributes may only be of type 'string', 'uuid', 'date', 'duration' or 'numeric'."
        );
    }

    #[test]
    fn uda_types_and_values() {
        let config = config(&[
            ("uda.s.type", "string"),
            ("uda.s.values", "H,M,L,"),
            ("uda.n.type", "numeric"),
            ("uda.n.label", "Num"),
            ("uda.n.values", "1,2"),
            ("uda.d.type", "date"),
            ("uda.du.type", "duration"),
            ("uda.u.type", "uuid"),
            ("uda.u.values", "ignored"),
            ("uda.none.label", "x"),
        ]);
        let mut all = ColumnRegistry::new();
        uda_all(&mut all, &config).unwrap();
        assert_eq!(all.keys().cloned().collect::<Vec<_>>(), strings(&["d", "du", "n", "s", "u"]));
        // name, type, style, styles
        let expected: &[(&str, &str, &str, &[&str])] = &[
            ("s", "string", "default", &["default", "indicator"]),
            ("n", "numeric", "default", &["default", "indicator"]),
            ("d", "date", "default", &["default", "indicator"]),
            ("du", "duration", "default", &["default", "indicator"]),
            ("u", "uuid", "long", &["long", "short"]),
        ];
        for (name, type_, style, styles) in expected {
            let c = &all[*name];
            assert_eq!(c.name(), *name);
            assert_eq!(c.type_(), *type_);
            assert_eq!(c.style(), *style);
            assert_eq!(c.styles(), strings(styles));
            assert!(c.is_uda() && c.modifiable());
        }
        assert_eq!(all["n"].label(), "Num");
        assert_eq!(all["u"].examples(), strings(&["f30cb9c3-3fc0-483f-bfb2-3bf134f00694", "f30cb9c3"]));

        // validate: exact match against the values (an empty value is
        // allowed by the trailing comma), anything without values.
        assert!(all["s"].validate("H") && all["s"].validate("") && !all["s"].validate("X"));
        assert!(all["n"].validate("2") && !all["n"].validate("3"));
        assert!(all["d"].validate("anything") && all["du"].validate(""));
        assert!(all["u"].validate("f30cb9c3-3fc0-483f-bfb2-3bf134f00694"));
        // Lexer::isUUID accepts a prefix of at least 8 characters.
        assert!(all["u"].validate("f30cb9c3") && !all["u"].validate("f30cb9c"));
        assert!(!all["u"].validate("ignored") && !all["u"].validate("f30cb9c3x"));

        assert!(uda("none", &config).unwrap().is_none());
        assert!(uda("missing", &config).unwrap().is_none());
    }

    #[test]
    fn uda_name_collisions_and_odd_keys() {
        let mut all = registry(&Configuration::new());
        assert_eq!(
            err(uda_all(&mut all, &config(&[("uda.project.type", "string")]))),
            "The UDA named 'project' is the same as a core attribute, and is not permitted."
        );
        let mut all = ColumnRegistry::new();
        assert_eq!(
            err(factory_all(&mut all, &config(&[("uda.tags.label", "x")]))),
            "The UDA named 'tags' is the same as a core attribute, and is not permitted."
        );

        // Keys without a second dot name no UDA; `uda..type` names "".
        let mut all = ColumnRegistry::new();
        uda_all(&mut all, &config(&[("uda.x", "1"), ("uda..type", "numeric")])).unwrap();
        assert_eq!(all.keys().cloned().collect::<Vec<_>>(), vec![String::new()]);
    }

    #[test]
    fn validate_builtins() {
        let all = registry(&Configuration::new());
        for name in ["project", "description", "due", "scheduled", "id", "urgency", "uuid", "tags", "depends"] {
            assert!(all[name].validate("x"), "{name}");
            assert!(!all[name].validate(""), "{name}");
        }
        assert!(all["rtype"].validate("periodic") && all["rtype"].validate("chained"));
        assert!(!all["rtype"].validate("x"));
    }

    #[test]
    fn modify_string() {
        let all = registry(&Configuration::new());
        let mut env = FakeEnv::default();
        let t = modify(&*all["description"], "hello world", &mut env).unwrap();
        assert_eq!(t.get("description"), "hello world");
        assert_eq!(env.debug, vec![label("description <-- 'hello world'")]);

        assert_eq!(
            err(modify(&*all["description"], "", &mut env)),
            "The 'description' attribute does not allow a value of ''."
        );

        // A lone DOM reference is evaluated, then validated.
        let mut env = FakeEnv::default();
        env.dom.insert("rc.foo".to_string(), Variant::from_string("bar"));
        env.dom.insert("rc.empty".to_string(), Variant::from_string(""));
        let t = modify(&*all["description"], "rc.foo", &mut env).unwrap();
        assert_eq!(t.get("description"), "bar");
        assert_eq!(env.debug, vec![label("description <-- 'bar' <-- 'rc.foo'")]);
        assert_eq!(
            err(modify(&*all["description"], "rc.empty", &mut env)),
            "The 'description' attribute does not allow a value of 'rc.empty'."
        );
        // More than one token: stored as given.
        let t = modify(&*all["description"], "rc.foo x", &mut env).unwrap();
        assert_eq!(t.get("description"), "rc.foo x");

        // UDA values are enforced through the virtual validate.
        let config = config(&[("uda.prio.type", "string"), ("uda.prio.values", "H,M,L")]);
        let all = registry(&config);
        assert_eq!(modify(&*all["prio"], "H", &mut env).unwrap().get("prio"), "H");
        assert_eq!(err(modify(&*all["prio"], "X", &mut env)), "The 'prio' attribute does not allow a value of 'X'.");
    }

    #[test]
    fn modify_uuid_uda() {
        let all = registry(&config(&[("uda.ref.type", "uuid")]));
        let mut env = FakeEnv::default();
        let uuid = "f30cb9c3-3fc0-483f-bfb2-3bf134f00694";
        assert_eq!(modify(&*all["ref"], uuid, &mut env).unwrap().get("ref"), uuid);
        assert_eq!(
            err(modify(&*all["ref"], "f30cb9c", &mut env)),
            "The 'ref' attribute does not allow a value of 'f30cb9c'."
        );
    }

    #[test]
    fn modify_date() {
        let all = registry(&Configuration::new());
        let mut env = FakeEnv::default();
        let t = modify(&*all["due"], "2030-01-01", &mut env).unwrap();
        let epoch = Datetime::from_string("2030-01-01", "").unwrap().to_epoch();
        assert_eq!(t.get("due"), epoch.to_string());
        let iso = Datetime::from_epoch(epoch).to_iso_local_extended();
        assert_eq!(env.debug, vec![label(&format!("due <-- '{epoch}' <-- '{iso}' <-- '2030-01-01'"))]);

        // A duration is relative to now; the debug line shows its seconds.
        let mut env = FakeEnv::default();
        let before = localtime::time_now();
        let t = modify(&*all["scheduled"], "2d", &mut env).unwrap();
        let after = localtime::time_now();
        let scheduled = t.get_date("scheduled");
        assert!(scheduled >= before + 2 * 86400 && scheduled <= after + 2 * 86400);
        assert_eq!(env.debug, vec![label("scheduled <-- '172800' <-- 'P2D' <-- '2d'")]);

        // Outside [EPOCH_MIN_VALUE, EPOCH_MAX_VALUE).
        assert_eq!(err(modify(&*all["due"], "1975-01-01", &mut env)), "'1975-01-01' is not a valid date.");

        // UDA dates share the implementation.
        let all = registry(&config(&[("uda.when.type", "date")]));
        assert_eq!(modify(&*all["when"], "2030-01-01", &mut env).unwrap().get("when"), epoch.to_string());
    }

    #[test]
    fn modify_numeric() {
        let all = registry(&config(&[("uda.size.type", "numeric")]));
        let col = &*all["size"];
        let mut env = FakeEnv::default();
        assert_eq!(modify(col, "5", &mut env).unwrap().get("size"), "5");
        // Eval keeps the literal text in the cast Variant's string.
        assert_eq!(env.debug, vec![label("size <-- '5' <-- '5'")]);
        assert_eq!(modify(col, "2+3", &mut env).unwrap().get("size"), "5");
        assert_eq!(modify(col, "2.5", &mut env).unwrap().get("size"), "2.5");
        assert_eq!(modify(col, "2d", &mut env).unwrap().get("size"), "172800");

        let mut env = FakeEnv::default();
        assert_eq!(err(modify(col, "abc", &mut env)), "The value 'abc' is not a valid numeric value.");
        assert_eq!(env.debug, vec![label("size <-- 'abc' <-- 'abc'")]);
        assert_eq!(err(modify(col, "true", &mut env)), "Unexpected variant type: '0'");
    }

    #[test]
    fn modify_duration() {
        let all = registry(&config(&[("uda.est.type", "duration")]));
        let col = &*all["est"];
        let mut env = FakeEnv::default();
        assert_eq!(modify(col, "2d", &mut env).unwrap().get("est"), "P2D");
        assert_eq!(env.debug, vec![label("est <-- P2D <-- '2d'")]);
        assert_eq!(err(modify(col, "abc", &mut env)), "The duration value 'abc' is not supported.");
        assert_eq!(err(modify(col, "5", &mut env)), "The duration value '5' is not supported.");
    }

    #[test]
    fn modify_recur() {
        let all = registry(&Configuration::new());
        let mut env = FakeEnv::default();
        assert_eq!(modify(&*all["recur"], "weekly", &mut env).unwrap().get("recur"), "weekly");
        assert_eq!(env.debug, vec![label("recur <-- 'weekly'")]);
        assert_eq!(err(modify(&*all["recur"], "often", &mut env)), "The duration value 'often' is not supported.");
    }

    #[test]
    fn modify_project() {
        let all = registry(&Configuration::new());
        let mut env = FakeEnv::default();
        env.dom.insert("rc.p".to_string(), Variant::from_string("work"));
        let t = modify(&*all["project"], "home.garden", &mut env).unwrap();
        assert_eq!(t.get("project"), "home.garden");
        let t = modify(&*all["project"], "rc.p", &mut env).unwrap();
        assert_eq!(t.get("project"), "work");
        // Not validated: an empty project is stored.
        let t = modify(&*all["project"], "", &mut env).unwrap();
        assert!(t.has("project") && t.get("project").is_empty());
        assert_eq!(
            env.debug,
            vec![label("project <-- 'home.garden'"), label("project <-- 'work' <-- 'rc.p'"), label("project <-- ''")]
        );
    }

    #[test]
    fn modify_tags() {
        let all = registry(&Configuration::new());
        let mut env = FakeEnv::default();
        let mut task = Task::new();
        task.add_tag("old");
        all["tags"].modify(&mut task, "a,next", &mut env).unwrap();
        assert_eq!(task.get_tags(), strings(&["a", "next"]));
        assert_eq!(env.feedback, strings(&["a", "next"]));
        assert_eq!(env.debug, vec![label("tags <-- 'a'"), label("tags <-- 'next'")]);

        env.dom.insert("rc.t".to_string(), Variant::from_string("x,y"));
        all["tags"].modify(&mut task, "rc.t", &mut env).unwrap();
        assert_eq!(task.get_tags(), strings(&["x", "y"]));
    }

    #[test]
    fn modify_depends() {
        let all = registry(&Configuration::new());
        let col = &*all["depends"];
        let full = "f30cb9c3-3fc0-483f-bfb2-3bf134f00694";
        let mut env = FakeEnv::default();
        let mut other = Task::new();
        other.set("uuid", "abcdef01-0000-0000-0000-000000000000");
        env.tasks.push(other);

        let mut task = Task::new();
        col.modify(&mut task, &format!("{full},abcdef01,1-3,5,-7"), &mut env).unwrap();
        assert_eq!(env.added, strings(&[full, "abcdef01-0000-0000-0000-000000000000"]));
        assert_eq!(env.added_ids, vec![1, 2, 3, 5]);
        assert_eq!(env.removed_ids, vec![7]);
        assert!(env.debug.is_empty());

        // Removing a full UUID goes straight to Task::removeDependency.
        task.set(&Task::dep2attr(full), "x");
        col.modify(&mut task, &format!("-{full}"), &mut env).unwrap();
        assert!(!task.has(&Task::dep2attr(full)));
        assert_eq!(
            err(col.modify(&mut task, &format!("-{full}"), &mut env)),
            format!("Could not delete a dependency on task {full} - not found.")
        );

        assert_eq!(
            err(col.modify(&mut task, "12345678", &mut env)),
            "Dependency could not be set - task with UUID '12345678' does not exist."
        );
        assert_eq!(err(col.modify(&mut task, "abc", &mut env)), "Invalid dependency value: 'abc'");
        assert_eq!(err(col.modify(&mut task, "abcdef012", &mut env)), "Invalid dependency value: 'abcdef012'");
        assert_eq!(err(col.modify(&mut task, "1,,2", &mut env)), "Invalid dependency value: ''");
    }

    #[test]
    fn non_modifiable_columns() {
        let all = registry(&Configuration::new());
        let fixed: Vec<&str> =
            all.iter().filter(|(_, c)| !c.modifiable()).map(|(name, _)| name.as_str()).collect();
        assert_eq!(fixed, vec!["id", "imask", "last", "mask", "parent", "rtype", "template", "urgency", "uuid"]);
    }
}
