//! Rust translation of `src/DOM.h` / `src/DOM.cpp`: DOM reference
//! resolution (`getDOM`, both overloads), the `DOM` provider tree class, and
//! the Eval resolver that C++ builds from `domSource` (`Eval.cpp`).
//!
//! # API
//!
//! | C++ | Rust |
//! |-----|------|
//! | `bool getDOM (name, Variant&)` | [`get_dom`]`(ctx, name) -> Result<Option<Variant>>` |
//! | `bool getDOM (name, const Task*, Variant&)` | [`get_dom_task`]`(ctx, name, task) -> Result<Option<Variant>>` (alias [`get`]) |
//! | `e.addSource (domSource)` + `Context::currentTask` | [`ContextResolver`] (a [`DomResolver`]) |
//! | `class DOM` / `DOM::Node` | [`Dom`] / [`Node`] |
//!
//! C++ `return true` after assigning `value` becomes `Ok (Some (value))`,
//! `return false` becomes `Ok (None)`, and anything C++ throws on the way
//! (TDB2 replica errors while loading a referenced task, `getStatus`,
//! `urgency_c`, `hasTag ("LATEST")`, an unparsable annotation date) is `Err`
//! with the C++ text.
//!
//! # Borrowing (milestone 5 Design Decision 2)
//!
//! The C++ reads everything through `Context::getContext ()`: the
//! configuration, `cli2` (binary, original args, `canonicalize`), `tdb2`
//! (`num_local_changes`, `get (id|uuid)`), the column registry, the terminal
//! size and `currentTask`. The Rust functions take `ctx: &mut Context`
//! explicitly (`&mut` because TDB2 loads lazily and `getWidth` detects the
//! terminal), and the contextual task separately as `Option<&Task>`.
//!
//! The convention, which the filter and later milestones follow, is:
//!
//! * **The contextual task is never borrowed out of the Context.** C++
//!   `Context::currentTask` is an `Option<Rc<Task>>`; a caller passing it
//!   clones the `Rc` first (`let task = ctx.current_task.clone();
//!   get_dom_task (ctx, name, task.as_deref ())`). Tasks from an owned
//!   `Vec<Task>` (filter loops, DD2 option 1) are passed by reference.
//! * **Eval gets a [`ContextResolver`]**, which holds the `&mut Context` in a
//!   `RefCell` plus the contextual task. Its `resolve` is C++ `domSource`;
//!   its `current_task` hands Eval the resolver itself as the
//!   [`VariantTask`], so `_hastag_` / `_notag_` evaluate virtual tags against
//!   the real Context (as C++ `Task::hasTag` does); its `debug` forwards to
//!   `Context::debug`. Eval never calls `resolve` while it holds the
//!   `current_task` borrow, so the `RefCell` is never contended.
//! * **CLI2 must be attached.** `tw.program`, `tw.args` and the attribute
//!   canonicalisation read `ctx.cli2`; while a CLI2 pass runs detached
//!   ([`crate::cli2::with_cli2`]), `ctx.cli2` is an empty placeholder. No
//!   C++ CLI2 pass calls `getDOM`, so DOM lookups only happen with the
//!   parser attached (commands, filters, `Task::modify`, column `modify`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::rc::Rc;

use crate::context::{strtol, Context};
use crate::error::Result;
use crate::eval::DomResolver;
use crate::lexer::{Lexer, Type as LexerType};
use crate::shared::datetime::Datetime;
use crate::shared::duration::Duration;
use crate::shared::shared::{join, os_name, split};
use crate::task::Task;
use crate::variant::{Type as VariantType, Variant, VariantTask};
use crate::version::VERSION;

/// C++ `strtoll (text, nullptr, 10)`: optional leading whitespace and sign,
/// then the leading decimal digits (saturating, as `strtoll` clamps to
/// `LLONG_MIN`/`LLONG_MAX`); 0 if none.
fn strtoll(text: &str) -> i64 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut value: i64 = 0;
    for b in digits.bytes().take_while(u8::is_ascii_digit) {
        let digit = i64::from(b - b'0');
        value = if negative {
            value.saturating_mul(10).saturating_sub(digit)
        } else {
            value.saturating_mul(10).saturating_add(digit)
        };
    }
    value
}

/// The C++ `tw.args` / `context.args` value: the `raw` attributes of
/// `cli2._original_args`, space separated.
fn command_line(ctx: &Context) -> String {
    let mut command_line = String::new();
    for arg in &ctx.cli2.original_args {
        if !command_line.is_empty() {
            command_line.push(' ');
        }

        command_line += &arg.attribute("raw");
    }

    command_line
}

/// The C++ `tw.width` / `context.width` value.
fn terminal_width(ctx: &mut Context) -> i32 {
    if ctx.terminal_width != 0 {
        ctx.terminal_width
    } else {
        ctx.get_width()
    }
}

/// The C++ `tw.height` / `context.height` value.
fn terminal_height(ctx: &mut Context) -> i32 {
    if ctx.terminal_height != 0 {
        ctx.terminal_height
    } else {
        ctx.get_height()
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `bool getDOM (const std::string& name, Variant& value)`: the
/// context-free references.
///
/// DOM Supported References:
///
/// ```text
/// Configuration:
///   rc.<name>
///
/// Taskwarrior:
///   tw.syncneeded
///   tw.program
///   tw.args
///   tw.width
///   tw.height
///   tw.version
///
/// System:
///   context.program         // 2017-02-25 Deprecated in 2.6.0
///   context.args            // 2017-02-25 Deprecated in 2.6.0
///   context.width           // 2017-02-25 Deprecated in 2.6.0
///   context.height          // 2017-02-25 Deprecated in 2.6.0
///   system.version
///   system.os
/// ```
///
/// # Errors
/// `tw.syncneeded` propagates TDB2 replica errors (C++ throws from
/// `num_local_changes`).
pub fn get_dom(ctx: &mut Context, name: &str) -> Result<Option<Variant>> {
    // Special case, blank refs cause problems.
    if name.is_empty() {
        return Ok(None);
    }

    let len = name.len();

    // rc. --> context.config
    if len > 3 && name.starts_with("rc.") {
        let key = &name[3..];
        // C++ `config.find (key)`: the raw map, no context-specific lookup.
        if let Some(c) = ctx.config.deref().get(key) {
            return Ok(Some(Variant::from_string(c)));
        }

        return Ok(None);
    }

    // tw.*
    if len > 3 && name.starts_with("tw.") {
        if name == "tw.syncneeded" {
            let mut value = Variant::from_int(0);
            if ctx.tdb2.num_local_changes()? > 0 {
                value = Variant::from_int(1);
            }

            return Ok(Some(value));
        } else if name == "tw.program" {
            return Ok(Some(Variant::from_string(&ctx.cli2.get_binary())));
        } else if name == "tw.args" {
            return Ok(Some(Variant::from_string(&command_line(ctx))));
        } else if name == "tw.width" {
            return Ok(Some(Variant::from_int(terminal_width(ctx))));
        } else if name == "tw.height" {
            return Ok(Some(Variant::from_int(terminal_height(ctx))));
        } else if name == "tw.version" {
            return Ok(Some(Variant::from_string(VERSION)));
        }

        return Ok(None);
    }

    // context.*
    if len > 8 && name.starts_with("context.") {
        if name == "context.program" {
            return Ok(Some(Variant::from_string(&ctx.cli2.get_binary())));
        } else if name == "context.args" {
            return Ok(Some(Variant::from_string(&command_line(ctx))));
        } else if name == "context.width" {
            return Ok(Some(Variant::from_int(terminal_width(ctx))));
        } else if name == "context.height" {
            return Ok(Some(Variant::from_int(terminal_height(ctx))));
        }

        return Ok(None);
    }

    // system. --> Implement locally.
    if len > 7 && name.starts_with("system.") {
        // Taskwarrior version number.
        if name == "system.version" {
            return Ok(Some(Variant::from_string(VERSION)));
        }
        // OS type.
        else if name == "system.os" {
            return Ok(Some(Variant::from_string(&os_name())));
        }

        return Ok(None);
    }

    // Empty string if nothing is found.
    Ok(None)
}

/// The C++ `<date>.<element>` switch shared by `<attribute>.<element>` and
/// `annotations.<N>.entry.<element>`: `None` for an unknown element.
fn date_element(date: &Datetime, element: &str) -> Option<Variant> {
    let value = match element {
        "year" => date.year(),
        "month" => date.month(),
        "day" => date.day(),
        "week" => date.week(),
        "weekday" => date.day_of_week(),
        "julian" => date.day_of_year(),
        "hour" => date.hour(),
        "minute" => date.minute(),
        "second" => date.second(),
        _ => return None,
    };

    Some(Variant::from_int(value))
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `bool getDOM (const std::string& name, const Task* task, Variant&
/// value)`: task-relative references, then the context-free ones.
///
/// DOM Supported References:
///
/// ```text
/// Relative or absolute attribute:
///   <attribute>
///   <id>.<attribute>
///   <uuid>.<attribute>
///
/// Single tag:
///   tags.<word>
///
/// Date type:
///   <date>.year
///   <date>.month
///   <date>.day
///   <date>.week
///   <date>.weekday
///   <date>.julian
///   <date>.hour
///   <date>.minute
///   <date>.second
///
/// Annotations (entry is a date):
///   annotations.count
///   annotations.<N>.entry
///   annotations.<N>.description
/// ```
///
/// This code emphasizes speed, hence 'id' and 'urgency' being evaluated
/// first as special cases.
///
/// If `task` is `None`, then the contextual task will be determined from
/// the DOM string, if any exists.
///
/// # Errors
/// As C++ throws: TDB2 errors loading `<id>`/`<uuid>` tasks, `urgency_c`,
/// `getStatus`, `hasTag` (`LATEST`), an annotation key whose timestamp does
/// not parse as a date, and those of [`get_dom`].
pub fn get_dom_task(ctx: &mut Context, name: &str, task: Option<&Task>) -> Result<Option<Variant>> {
    // Special case, blank refs cause problems.
    if name.is_empty() {
        return Ok(None);
    }

    // Quickly deal with the most common cases.
    if let Some(task) = task {
        if name == "id" {
            return Ok(Some(Variant::from_int(task.id)));
        }

        if name == "urgency" {
            return Ok(Some(Variant::from_real(f64::from(task.urgency_c(ctx)?))));
        }
    }

    // split name on '.'
    let mut elements = split(name, '.');
    let loaded_task: Task;

    // decide whether the reference is going to be the passed
    // "task" or whether it's going to be a newly loaded task (if id/uuid was
    // given).
    let mut reference: Option<&Task> = task;
    let mut lexer = Lexer::new(&elements[0]);
    let mut token = String::new();
    let mut type_ = LexerType::word;

    // If this can be ID/UUID reference (the name contains '.'),
    // lex it to figure out. Otherwise don't lex, as lexing can be slow.
    if elements.len() > 1 && lexer.token(&mut token, &mut type_) {
        let mut reloaded: Option<Task> = None;

        if type_ == LexerType::uuid && token.len() == elements[0].len() {
            if task.is_none_or(|t| token != t.get("uuid")) {
                reloaded = ctx.tdb2.get_by_uuid(&token)?;
            }

            // Eat elements[0]/UUID.
            elements.remove(0);
        } else if type_ == LexerType::number && !token.contains('.') {
            // Deviation: C++ keeps `strtol`'s `long` for the comparison
            // and narrows it to `int` for `TDB2::get (int, ...)`; IDs beyond
            // the `int` range cannot exist, so the `int` value is used.
            let id = strtol(&token);
            if id != 0 && task.is_none_or(|t| id != t.id) {
                reloaded = ctx.tdb2.get_by_id(id)?;
            }

            // Eat elements[0]/ID.
            elements.remove(0);
        }

        if let Some(found) = reloaded {
            loaded_task = found;
            reference = Some(&loaded_task);
        }
    }

    // The remainder of this method requires a contextual task, so if we do not
    // have one, delegate to the two-argument getDOM
    let Some(reference) = reference else {
        return get_dom(ctx, name);
    };

    let size = elements.len();

    let canonical = if size == 1 || size == 2 { ctx.cli2.canonicalize("attribute", &elements[0]) } else { None };
    if let Some(canonical) = canonical {
        // Now that 'ref' is the contextual task, and any ID/UUID is chopped off the
        // elements vector, DOM resolution is now simple.
        if size == 1 && canonical == "id" {
            return Ok(Some(Variant::from_int(reference.id)));
        }

        if size == 1 && canonical == "urgency" {
            return Ok(Some(Variant::from_real(f64::from(reference.urgency_c(ctx)?))));
        }

        // Special handling of status required for virtual waiting status
        // implementation. Remove in 3.0.0.
        if size == 1 && canonical == "status" {
            return Ok(Some(Variant::from_string(Task::status_to_text(reference.get_status()?))));
        }

        // The "tags" property is deprecated, but it is documented as part of the DOM, so simulate it.
        if size == 1 && canonical == "tags" {
            let tags = reference.get_tags();
            return Ok(Some(Variant::from_string(&join(",", &tags))));
        }

        // C++ `Context::getContext ().columns[canonical]`: every "attribute"
        // entity is a column name (Context registers them from the column
        // registry), so the C++ `operator[]` never inserts a null entry.
        let column = ctx.columns.get(&canonical).cloned();

        if let (1, Some(column)) = (size, &column) {
            if column.is_uda() && !reference.has(&canonical) {
                return Ok(Some(Variant::from_string("")));
            }

            let value = if column.type_() == "date" {
                let numeric = reference.get_date(&canonical);
                if numeric == 0 {
                    Variant::from_string("")
                } else {
                    Variant::from_time(numeric, VariantType::Date)?
                }
            } else if column.type_() == "duration" || canonical == "recur" {
                let period = reference.get(&canonical);

                let mut iso = Duration::new();
                let mut cursor = 0;
                if iso.parse(&period, &mut cursor) {
                    Variant::from_time(iso.to_time_t(), VariantType::Duration)?
                } else {
                    Variant::from_time(
                        Duration::from_string(&reference.get(&canonical)).to_time_t(),
                        VariantType::Duration,
                    )?
                }
            } else if column.type_() == "numeric" {
                Variant::from_real(f64::from(reference.get_float(&canonical)))
            } else {
                Variant::from_string(&reference.get(&canonical))
            };

            return Ok(Some(value));
        }

        if size == 2 && canonical == "tags" {
            let has = reference.has_tag(&elements[1], ctx)?;
            return Ok(Some(Variant::from_string(if has { &elements[1] } else { "" })));
        }

        if size == 2 && column.as_ref().is_some_and(|c| c.type_() == "date") {
            let date = Datetime::from_epoch(reference.get_date(&canonical));
            if let Some(value) = date_element(&date, &elements[1]) {
                return Ok(Some(value));
            }
        }
    }

    if size == 2 && elements[0] == "annotations" && elements[1] == "count" {
        return Ok(Some(Variant::from_int(reference.get_annotation_count())));
    }

    if size == 3 && elements[0] == "annotations" {
        let annos: BTreeMap<String, String> = reference.get_annotations();

        let a = strtol(&elements[1]);
        let mut count = 0;

        // Count off the 'a'th annotation.
        for (key, description) in &annos {
            count += 1;
            if count == a {
                if elements[2] == "entry" {
                    // annotation_1234567890
                    // 0          ^11
                    return Ok(Some(Variant::from_time(strtoll(&key[11..]), VariantType::Date)?));
                } else if elements[2] == "description" {
                    return Ok(Some(Variant::from_string(description)));
                }
            }
        }
    }

    if size == 4 && elements[0] == "annotations" && elements[2] == "entry" {
        let annos = reference.get_annotations();

        let a = strtol(&elements[1]);
        let mut count = 0;

        // Count off the 'a'th annotation.
        for key in annos.keys() {
            count += 1;
            if count == a {
                // <annotations>.<N>.entry.year
                // <annotations>.<N>.entry.month
                // <annotations>.<N>.entry.day
                // <annotations>.<N>.entry.week
                // <annotations>.<N>.entry.weekday
                // <annotations>.<N>.entry.julian
                // <annotations>.<N>.entry.hour
                // <annotations>.<N>.entry.minute
                // <annotations>.<N>.entry.second
                //
                // C++ `Datetime date (i.first.substr (11))`: the string
                // constructor (default format ""), which throws when the
                // timestamp does not parse.
                let date = Datetime::from_string(&key[11..], "")?;
                if let Some(value) = date_element(&date, &elements[3]) {
                    return Ok(Some(value));
                }
            }
        }
    }

    // Delegate to the context-free version of DOM::get.
    get_dom(ctx, name)
}

/// The milestone 5 API name for [`get_dom_task`] (C++ `getDOM (name, task,
/// value)`; the C++ header deprecates `getDOM` in favour of `DOM::get`, which
/// was never given a task-aware form).
pub fn get(ctx: &mut Context, name: &str, task: Option<&Task>) -> Result<Option<Variant>> {
    get_dom_task(ctx, name, task)
}

////////////////////////////////////////////////////////////////////////////////
/// The contextual task of a [`ContextResolver`]: either C++
/// `Context::currentTask` (shared) or a task the caller owns.
enum ResolverTask<'a> {
    Shared(Rc<Task>),
    Borrowed(&'a Task),
}

impl ResolverTask<'_> {
    fn get(&self) -> &Task {
        match self {
            ResolverTask::Shared(task) => task,
            ResolverTask::Borrowed(task) => task,
        }
    }
}

/// The resolver C++ `Eval` gets from `e.addSource (domSource)`: identifier
/// lookups go through `getDOM (identifier, currentTask, value)` (with the
/// identifier set as the value's source), the current task is the
/// contextual task, and debug output goes to `Context::debug`. See the
/// module documentation, "Borrowing".
pub struct ContextResolver<'a> {
    ctx: RefCell<&'a mut Context>,
    task: Option<ResolverTask<'a>>,
}

impl<'a> ContextResolver<'a> {
    /// C++ `domSource` exactly: the contextual task is
    /// `Context::getContext ().currentTask` (`ctx.current_task`, an `Rc`
    /// clone).
    pub fn new(ctx: &'a mut Context) -> Self {
        let task = ctx.current_task.clone().map(ResolverTask::Shared);
        ContextResolver { ctx: RefCell::new(ctx), task }
    }

    /// `domSource` with an explicit contextual task (C++ code that sets
    /// `currentTask` around an evaluation, e.g. `Filter::subset`), or
    /// `None` for a null `currentTask` (e.g. `Eval::compileExpression`).
    pub fn with_task(ctx: &'a mut Context, task: Option<&'a Task>) -> Self {
        ContextResolver { ctx: RefCell::new(ctx), task: task.map(ResolverTask::Borrowed) }
    }

    /// The Context, e.g. to read it between evaluations.
    pub fn context(&mut self) -> &mut Context {
        self.ctx.get_mut()
    }
}

impl DomResolver for ContextResolver<'_> {
    /// C++ `domSource`: `getDOM (identifier, currentTask, value)`, and on
    /// success `value.source (identifier)`.
    fn resolve(&mut self, name: &str) -> Result<Option<Variant>> {
        let ctx = self.ctx.get_mut();
        let task = self.task.as_ref().map(ResolverTask::get);
        match get_dom_task(ctx, name, task)? {
            Some(mut value) => {
                value.set_source(name);
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn current_task(&self) -> Option<&dyn VariantTask> {
        match self.task {
            Some(_) => Some(self),
            None => None,
        }
    }

    fn debug(&mut self, message: &str) {
        self.ctx.get_mut().debug(message);
    }
}

/// The contextual task as Variant sees it: C++ `Task::hasTag` reads the
/// Context (`rc.due`, the UDA columns, `tdb2.latest_id`), so the tag check
/// runs against the resolver's Context.
impl VariantTask for ContextResolver<'_> {
    fn get_annotations(&self) -> BTreeMap<String, String> {
        self.task.as_ref().map(|t| t.get().get_annotations()).unwrap_or_default()
    }

    fn has_tag(&self, tag: &str) -> Result<bool> {
        match &self.task {
            Some(task) => {
                let mut ctx = self.ctx.borrow_mut();
                task.get().has_tag(tag, &mut **ctx)
            }
            None => Ok(false),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ provider function pointer `bool (*)(const std::string&, Variant&)`.
pub type Provider = fn(&str, &mut Variant) -> bool;

////////////////////////////////////////////////////////////////////////////////
/// C++ `class DOM`.
///
/// References are paths into a tree structure. For example:
///
/// ```text
///   1.due.month.number
///   1.due.day.number
/// ```
///
/// Are represented internally as:
///
/// ```text
///   1
///   +- due
///     +- day
///     | +- number
///     +- month
///       +- number
/// ```
///
/// The tree is augmented by other elements:
///
/// ```text
///   1
///   +- due
///   | +- day
///   | | +- number
///   | +- month
///   |   +- number
///   +- system
///      +- os
/// ```
///
/// Each node in the tree has a name ("due", "system", "day"), and each node
/// may have a data source attached to it.
///
/// The DOM class is independent of the project, in that it knows nothing
/// about the internal data or program structure. It knows only that certain
/// DOM path elements have handlers which will provide the data.
///
/// The DOM class is therefore responsible for maintaining a tree of named
/// nodes with associated proividers. When a reference value is requested,
/// the DOM class will decompose the reference path, and navigate the tree to
/// the lowest level provider, and call it.
///
/// This makes the DOM class a reusible object.
#[derive(Debug, Default)]
pub struct Dom {
    /// C++ `DOM::Node* _node{nullptr}`.
    node: Option<Box<Node>>,
}

impl Dom {
    /// C++ `DOM ()` (implicit).
    pub fn new() -> Self {
        Dom::default()
    }

    /// C++ `DOM::addSource`.
    pub fn add_source(&mut self, reference: &str, provider: Provider) {
        self.node.get_or_insert_with(|| Box::new(Node::new())).add_source(reference, provider);
    }

    /// C++ `DOM::valid`.
    pub fn valid(&self, reference: &str) -> bool {
        self.node.as_ref().is_some_and(|node| node.find(reference).is_some())
    }

    /// C++ `DOM::get`: the provider's value, or `""` when the reference has
    /// no provider or the provider returns `false`. As in C++, a provider
    /// returning `false` after assigning still leaves its assignment in the
    /// result.
    pub fn get(&self, reference: &str) -> Variant {
        let mut v = Variant::from_string("");

        if let Some(node) = &self.node {
            if let Some(found) = node.find(reference) {
                if let Some(provider) = found.provider {
                    if provider(reference, &mut v) {
                        return v;
                    }
                }
            }
        }

        v
    }

    /// C++ `DOM::count`.
    pub fn count(&self) -> i32 {
        match &self.node {
            Some(node) => node.count(),
            None => 0,
        }
    }

    /// C++ static `DOM::decomposeReference`.
    pub fn decompose_reference(reference: &str) -> Vec<String> {
        split(reference, '.')
    }

    /// C++ `DOM::dump`.
    pub fn dump(&self) -> String {
        match &self.node {
            Some(node) => node.dump(),
            None => String::new(),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class DOM::Node`.
#[derive(Debug)]
pub struct Node {
    /// C++ `std::string _name{"Unknown"}`.
    pub name: String,
    /// C++ `_provider{nullptr}`.
    pub provider: Option<Provider>,
    /// C++ `std::vector<DOM::Node*> _branches{}`.
    pub branches: Vec<Box<Node>>,
}

impl Default for Node {
    fn default() -> Self {
        Node::new()
    }
}

impl Node {
    /// C++ `DOM::Node ()` (member initialisers).
    pub fn new() -> Self {
        Node { name: "Unknown".to_string(), provider: None, branches: Vec::new() }
    }

    /// C++ `DOM::Node::addSource`.
    pub fn add_source(&mut self, reference: &str, provider: Provider) {
        let mut cursor = self;
        for element in Dom::decompose_reference(reference) {
            let position = cursor.branches.iter().position(|branch| branch.name == element);
            cursor = match position {
                Some(index) => &mut cursor.branches[index],
                None => {
                    let mut branch = Box::new(Node::new());
                    branch.name = element;
                    cursor.branches.push(branch);
                    cursor.branches.last_mut().expect("just pushed")
                }
            };
        }

        cursor.provider = Some(provider);
    }

    /// C++ `DOM::Node::valid`: a valid reference is one that has a provider
    /// function.
    pub fn valid(&self, reference: &str) -> bool {
        self.find(reference).is_some()
    }

    /// C++ `DOM::Node::find`: the deepest node matching a prefix of the
    /// reference's elements, or `None` when not even the first element
    /// matches (or the reference is empty).
    pub fn find(&self, reference: &str) -> Option<&Node> {
        let mut cursor = self;
        for element in Dom::decompose_reference(reference) {
            match cursor.branches.iter().find(|branch| branch.name == element) {
                Some(branch) => cursor = branch,
                None => break,
            }
        }

        if !reference.is_empty() && !std::ptr::eq(cursor, self) {
            return Some(cursor);
        }

        None
    }

    /// C++ `DOM::Node::count`: recurse and count the branches with a
    /// provider.
    pub fn count(&self) -> i32 {
        let mut total = 0;
        for branch in &self.branches {
            if branch.provider.is_some() {
                total += 1;
            }
            total += branch.count();
        }

        total
    }

    /// C++ `DOM::Node::dumpNode`. The provider is printed as its address,
    /// as C++ streams `std::hex << (long long)(void*)provider`.
    pub fn dump_node(&self, node: &Node, depth: usize) -> String {
        let mut out = String::new();

        // Indent.
        out += &" ".repeat(depth * 2);

        out += &format!("\x1b[31m{}\x1b[0m", node.name);

        if let Some(provider) = node.provider {
            out += &format!(" 0x{:x}", provider as usize);
        }

        out.push('\n');

        // Recurse for branches.
        for b in &node.branches {
            out += &self.dump_node(b, depth + 1);
        }

        out
    }

    /// C++ `DOM::Node::dump`.
    pub fn dump(&self) -> String {
        format!("DOM::Node ({} nodes)\n{}", self.count(), self.dump_node(self, 1))
    }
}

#[cfg(test)]
#[path = "dom_tests.rs"]
mod tests;
