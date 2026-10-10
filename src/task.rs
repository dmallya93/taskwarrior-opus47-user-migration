//! Rust translation of `src/Task.h` / `src/Task.cpp`.
//!
//! This part holds the core data model: the process-wide `Task::*` statics,
//! the [`Status`] / [`DateState`] / [`ModType`] enums, the ordered attribute
//! map ([`Task::data`], C++ `std::map<std::string, std::string> data`),
//! typed getters and setters, the virtual-tag predicates, tags, annotations,
//! dependency attributes, `encode`/`decode`, and `parseTC` (the
//! `Task (rust::Box<tc::TaskData>)` constructor) over the native
//! [`taskchampion::TaskData`].
//!
//! API mapping:
//! * C++ statics are thread-local getter/setter pairs on [`Task`]
//!   (`Task::defaultProject` -> [`Task::default_project`] /
//!   [`Task::set_default_project`], ...), defaults as in C++.
//! * `getStatus` -> [`Task::get_status`] (`Result`, since `textToStatus`
//!   throws). The predicates (`is_due`, `hasTag`, ...) are `bool` as in C++;
//!   see [`Task::get_status`] for how they treat an invalid status.
//! * C++ methods that read `Context::getContext ()` take a
//!   `&mut dyn` [`TaskEnv`] (`rc.due`, the column registry,
//!   `tdb2.latest_id ()`). [`Context`] implements it; [`NullEnv`] is the
//!   context-free stand-in, and [`TaskWithEnv`] binds a task to an
//!   environment so DOM/filter code can hand it to Variant as a
//!   [`VariantTask`].
//! * Overloads become distinct names: `set (name, long long)` ->
//!   [`Task::set_int`]; `Task (rust::Box<tc::TaskData>)` ->
//!   [`Task::from_task_data`].
//! * `isupper`, `strtol`, `strtoul`, `strtof` follow the C locale, through
//!   libc for the numeric conversions.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::CString;

use crate::context::Context;
use crate::shared::datetime::Datetime;
use crate::shared::format::format;
use crate::shared::localtime;
use crate::shared::shared::{join, str_replace};
use crate::variant::VariantTask;
use crate::{Result, TwError};

thread_local! {
    /// C++ `std::string Task::defaultProject = "";`
    static DEFAULT_PROJECT: RefCell<String> = const { RefCell::new(String::new()) };
    /// C++ `std::string Task::defaultDue = "";`
    static DEFAULT_DUE: RefCell<String> = const { RefCell::new(String::new()) };
    /// C++ `std::string Task::defaultScheduled = "";`
    static DEFAULT_SCHEDULED: RefCell<String> = const { RefCell::new(String::new()) };
    /// C++ `bool Task::searchCaseSensitive = true;`
    static SEARCH_CASE_SENSITIVE: Cell<bool> = const { Cell::new(true) };
    /// C++ `bool Task::regex = false;`
    static REGEX: Cell<bool> = const { Cell::new(false) };
    /// C++ `std::map<std::string, std::string> Task::attributes;` (name -> type).
    static ATTRIBUTES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    /// C++ `std::map<std::string, float> Task::coefficients;`
    static COEFFICIENTS: RefCell<BTreeMap<String, f32>> = const { RefCell::new(BTreeMap::new()) };
    /// C++ `std::map<std::string, std::vector<std::string>> Task::customOrder;`
    static CUSTOM_ORDER: RefCell<BTreeMap<String, Vec<String>>> = const { RefCell::new(BTreeMap::new()) };
    /// C++ `float Task::urgencyProjectCoefficient = 0.0;`
    static URGENCY_PROJECT_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyActiveCoefficient = 0.0;`
    static URGENCY_ACTIVE_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyScheduledCoefficient = 0.0;`
    static URGENCY_SCHEDULED_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyWaitingCoefficient = 0.0;`
    static URGENCY_WAITING_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyBlockedCoefficient = 0.0;`
    static URGENCY_BLOCKED_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyAnnotationsCoefficient = 0.0;`
    static URGENCY_ANNOTATIONS_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyTagsCoefficient = 0.0;`
    static URGENCY_TAGS_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyDueCoefficient = 0.0;`
    static URGENCY_DUE_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyBlockingCoefficient = 0.0;`
    static URGENCY_BLOCKING_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyAgeCoefficient = 0.0;`
    static URGENCY_AGE_COEFFICIENT: Cell<f32> = const { Cell::new(0.0) };
    /// C++ `float Task::urgencyAgeMax = 0.0;`
    static URGENCY_AGE_MAX: Cell<f32> = const { Cell::new(0.0) };
}

/// C++ `enum Task::status { pending, completed, deleted, recurring, waiting }`
/// (same discriminants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// `Task::pending`
    Pending = 0,
    /// `Task::completed`
    Completed = 1,
    /// `Task::deleted`
    Deleted = 2,
    /// `Task::recurring`
    Recurring = 3,
    /// `Task::waiting` (virtual: a pending task with a future `wait`).
    Waiting = 4,
}

/// C++ `enum Task::dateState { dateNotDue, dateAfterToday, dateLaterToday,
/// dateEarlierToday, dateBeforeToday }` (same discriminants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DateState {
    /// `dateNotDue`
    NotDue = 0,
    /// `dateAfterToday`
    AfterToday = 1,
    /// `dateLaterToday`
    LaterToday = 2,
    /// `dateEarlierToday`
    EarlierToday = 3,
    /// `dateBeforeToday`
    BeforeToday = 4,
}

/// C++ `enum Task::modType { modReplace, modPrepend, modAppend, modAnnotate }`
/// (same discriminants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModType {
    /// `modReplace`
    Replace = 0,
    /// `modPrepend`
    Prepend = 1,
    /// `modAppend`
    Append = 2,
    /// `modAnnotate`
    Annotate = 3,
}

////////////////////////////////////////////////////////////////////////////////
/// The parts of `Context::getContext ()` that Task's virtual-tag and date
/// predicates read. Implemented by [`Context`] and [`NullEnv`].
pub trait TaskEnv {
    /// C++ `Context::getContext ().config.getInteger ("due")`.
    fn due_days(&self) -> i32;
    /// C++ `context.columns.find (name) != context.columns.end ()`.
    fn has_column(&self, name: &str) -> bool;
    /// The names (in registry order) of the columns whose `is_uda ()` is true.
    fn uda_columns(&self) -> Vec<String>;
    /// C++ `Context::getContext ().tdb2.latest_id ()`.
    fn latest_id(&mut self) -> i32;
}

impl TaskEnv for Context {
    fn due_days(&self) -> i32 {
        self.config.get_integer("due")
    }

    fn has_column(&self, name: &str) -> bool {
        self.columns.contains_key(name)
    }

    fn uda_columns(&self) -> Vec<String> {
        self.columns.iter().filter(|(_, col)| col.is_uda()).map(|(name, _)| name.clone()).collect()
    }

    /// Deviation: C++ `TDB2::latest_id` can throw (replica errors while
    /// loading the working set); the trait returns `i32` as `hasTag` is
    /// `bool`, so an error reads as 0 (no task has ID 0 in the working set,
    /// so `+LATEST` is false). Tdb2 has normally loaded the working set
    /// successfully before any filter runs.
    fn latest_id(&mut self) -> i32 {
        self.tdb2.latest_id().unwrap_or(0)
    }
}

/// A context-free [`TaskEnv`]: `rc.due` at its default (7), no columns, and
/// `latest_id` 0. Used where C++ would read a default Context, e.g. the
/// `impl VariantTask for Task`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullEnv;

impl TaskEnv for NullEnv {
    fn due_days(&self) -> i32 {
        7
    }

    fn has_column(&self, _name: &str) -> bool {
        false
    }

    fn uda_columns(&self) -> Vec<String> {
        Vec::new()
    }

    fn latest_id(&mut self) -> i32 {
        0
    }
}

/// A task bound to a [`TaskEnv`], so Variant's `op_match` / `op_hastag`
/// (which take a `&dyn VariantTask`) evaluate virtual tags such as `+UDA`,
/// `+DUE` or `+LATEST` against the real context. Milestone 5's DOM and
/// filter code builds one per task.
pub struct TaskWithEnv<'a> {
    task: &'a Task,
    env: RefCell<&'a mut dyn TaskEnv>,
}

impl<'a> TaskWithEnv<'a> {
    /// Binds `task` to `env`.
    pub fn new(task: &'a Task, env: &'a mut dyn TaskEnv) -> Self {
        TaskWithEnv { task, env: RefCell::new(env) }
    }

    /// The bound task.
    pub fn task(&self) -> &Task {
        self.task
    }
}

impl VariantTask for TaskWithEnv<'_> {
    fn get_annotations(&self) -> BTreeMap<String, String> {
        self.task.get_annotations()
    }

    fn has_tag(&self, tag: &str) -> bool {
        let mut env = self.env.borrow_mut();
        self.task.has_tag(tag, &mut **env)
    }
}

/// `Task&` as used by Variant.cpp. `has_tag` evaluates virtual tags against
/// a [`NullEnv`] (no context); milestone 5 passes a [`TaskWithEnv`] where
/// the context matters.
impl VariantTask for Task {
    fn get_annotations(&self) -> BTreeMap<String, String> {
        Task::get_annotations(self)
    }

    fn has_tag(&self, tag: &str) -> bool {
        Task::has_tag(self, tag, &mut NullEnv)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class Task`.
#[derive(Debug, Clone)]
pub struct Task {
    /// C++ `int id{0};`
    pub id: i32,
    /// C++ `float urgency_value{0.0};`
    pub urgency_value: f32,
    /// C++ `bool recalc_urgency{true};`
    pub recalc_urgency: bool,
    /// C++ `bool is_blocked{false};`
    pub is_blocked: bool,
    /// C++ `bool is_blocking{false};`
    pub is_blocking: bool,
    /// C++ `int annotation_count{0};`
    pub annotation_count: i32,
    /// C++ `protected: std::map<std::string, std::string> data{};`
    pub(crate) data: BTreeMap<String, String>,
}

/// C++ `Task () = default;` with the member initialisers.
impl Default for Task {
    fn default() -> Self {
        Task {
            id: 0,
            urgency_value: 0.0,
            recalc_urgency: true,
            is_blocked: false,
            is_blocking: false,
            annotation_count: 0,
            data: BTreeMap::new(),
        }
    }
}

/// C++ `bool Task::operator== (const Task&)`: the uuid (and id) are exempt
/// from comparison. The attribute map sizes must match, and every attribute
/// of `self` other than `uuid` must have the same value in `other` (read
/// with `other.get`, so a missing attribute compares as `""`).
///
/// Note: as in C++ this relation is not symmetric in corner cases (an empty
/// value in `self` matches a missing attribute in `other`).
impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        if self.data.len() != other.data.len() {
            return false;
        }

        for (name, value) in &self.data {
            if name != "uuid" && *value != other.get(name) {
                return false;
            }
        }

        true
    }
}

/// `text` as a C string, truncated at the first NUL like `c_str ()`.
fn c_string(text: &str) -> CString {
    let text = text.find('\0').map_or(text, |nul| &text[..nul]);
    CString::new(text).unwrap_or_default()
}

/// C `strtol (s.c_str (), nullptr, 10)` (`long` is 64-bit on LP64).
fn c_strtol(text: &str) -> i64 {
    let text = c_string(text);
    // SAFETY: `text` is a valid NUL-terminated string and a null `endptr` is
    // permitted.
    unsafe { libc::strtol(text.as_ptr(), std::ptr::null_mut(), 10) }
}

/// C `strtoul (s.c_str (), nullptr, 10)` (`unsigned long` is 64-bit on LP64;
/// a leading `-` negates in unsigned arithmetic, as in C).
fn c_strtoul(text: &str) -> u64 {
    let text = c_string(text);
    // SAFETY: as in `c_strtol`.
    unsafe { libc::strtoul(text.as_ptr(), std::ptr::null_mut(), 10) }
}

/// C `strtof (s.c_str (), nullptr)` (the process never calls `setlocale`, so
/// the radix is `.`).
fn c_strtof(text: &str) -> f32 {
    let text = c_string(text);
    // SAFETY: as in `c_strtol`.
    unsafe { libc::strtof(text.as_ptr(), std::ptr::null_mut()) }
}

/// C++ `Datetime (name)` for the named dates the predicates use (`now`,
/// `today`, `yesterday`, `sow`, ...). These always parse; the fallback to
/// the current time only exists because the constructor is fallible.
fn named_date(name: &str) -> Datetime {
    Datetime::from_string(name, "").unwrap_or_else(|_| Datetime::new())
}

/// C++ `std::string::substr (0, n)` on bytes (the uuid is ASCII).
fn substr_prefix(text: &str, n: usize) -> String {
    let bytes = text.as_bytes();
    String::from_utf8_lossy(&bytes[..n.min(bytes.len())]).into_owned()
}

impl Task {
    ////////////////////////////////////////////////////////////////////////////
    // Statics.

    /// C++ `Task::defaultProject` (read).
    pub fn default_project() -> String {
        DEFAULT_PROJECT.with(|v| v.borrow().clone())
    }

    /// C++ `Task::defaultProject = value`.
    pub fn set_default_project(value: &str) {
        DEFAULT_PROJECT.with(|v| *v.borrow_mut() = value.to_string());
    }

    /// C++ `Task::defaultDue` (read).
    pub fn default_due() -> String {
        DEFAULT_DUE.with(|v| v.borrow().clone())
    }

    /// C++ `Task::defaultDue = value`.
    pub fn set_default_due(value: &str) {
        DEFAULT_DUE.with(|v| *v.borrow_mut() = value.to_string());
    }

    /// C++ `Task::defaultScheduled` (read).
    pub fn default_scheduled() -> String {
        DEFAULT_SCHEDULED.with(|v| v.borrow().clone())
    }

    /// C++ `Task::defaultScheduled = value`.
    pub fn set_default_scheduled(value: &str) {
        DEFAULT_SCHEDULED.with(|v| *v.borrow_mut() = value.to_string());
    }

    /// C++ `Task::searchCaseSensitive` (read).
    pub fn search_case_sensitive() -> bool {
        SEARCH_CASE_SENSITIVE.with(Cell::get)
    }

    /// C++ `Task::searchCaseSensitive = value`.
    pub fn set_search_case_sensitive(value: bool) {
        SEARCH_CASE_SENSITIVE.with(|v| v.set(value));
    }

    /// C++ `Task::regex` (read).
    pub fn regex() -> bool {
        REGEX.with(Cell::get)
    }

    /// C++ `Task::regex = value`.
    pub fn set_regex(value: bool) {
        REGEX.with(|v| v.set(value));
    }

    /// C++ `Task::attributes` (read): attribute name -> column type.
    pub fn attributes() -> BTreeMap<String, String> {
        ATTRIBUTES.with(|v| v.borrow().clone())
    }

    /// C++ `Task::attributes = value`.
    pub fn set_attributes(value: BTreeMap<String, String>) {
        ATTRIBUTES.with(|v| *v.borrow_mut() = value);
    }

    /// Runs `f` on `Task::attributes` without copying it (C++
    /// `Task::attributes.find (...)` and friends).
    pub fn with_attributes<R>(f: impl FnOnce(&BTreeMap<String, String>) -> R) -> R {
        ATTRIBUTES.with(|v| f(&v.borrow()))
    }

    /// C++ `Task::coefficients` (read).
    pub fn coefficients() -> BTreeMap<String, f32> {
        COEFFICIENTS.with(|v| v.borrow().clone())
    }

    /// C++ `Task::coefficients = value`.
    pub fn set_coefficients(value: BTreeMap<String, f32>) {
        COEFFICIENTS.with(|v| *v.borrow_mut() = value);
    }

    /// Runs `f` on `Task::coefficients` without copying it.
    pub fn with_coefficients<R>(f: impl FnOnce(&BTreeMap<String, f32>) -> R) -> R {
        COEFFICIENTS.with(|v| f(&v.borrow()))
    }

    /// C++ `Task::customOrder` (read).
    pub fn custom_order() -> BTreeMap<String, Vec<String>> {
        CUSTOM_ORDER.with(|v| v.borrow().clone())
    }

    /// C++ `Task::customOrder = value`.
    pub fn set_custom_order(value: BTreeMap<String, Vec<String>>) {
        CUSTOM_ORDER.with(|v| *v.borrow_mut() = value);
    }

    /// Runs `f` on `Task::customOrder` without copying it (C++
    /// `Task::customOrder.find (...)`).
    pub fn with_custom_order<R>(f: impl FnOnce(&BTreeMap<String, Vec<String>>) -> R) -> R {
        CUSTOM_ORDER.with(|v| f(&v.borrow()))
    }

    /// C++ `Task::urgencyProjectCoefficient` (read).
    pub fn urgency_project_coefficient() -> f32 {
        URGENCY_PROJECT_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyProjectCoefficient = value`.
    pub fn set_urgency_project_coefficient(value: f32) {
        URGENCY_PROJECT_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyActiveCoefficient` (read).
    pub fn urgency_active_coefficient() -> f32 {
        URGENCY_ACTIVE_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyActiveCoefficient = value`.
    pub fn set_urgency_active_coefficient(value: f32) {
        URGENCY_ACTIVE_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyScheduledCoefficient` (read).
    pub fn urgency_scheduled_coefficient() -> f32 {
        URGENCY_SCHEDULED_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyScheduledCoefficient = value`.
    pub fn set_urgency_scheduled_coefficient(value: f32) {
        URGENCY_SCHEDULED_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyWaitingCoefficient` (read).
    pub fn urgency_waiting_coefficient() -> f32 {
        URGENCY_WAITING_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyWaitingCoefficient = value`.
    pub fn set_urgency_waiting_coefficient(value: f32) {
        URGENCY_WAITING_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyBlockedCoefficient` (read).
    pub fn urgency_blocked_coefficient() -> f32 {
        URGENCY_BLOCKED_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyBlockedCoefficient = value`.
    pub fn set_urgency_blocked_coefficient(value: f32) {
        URGENCY_BLOCKED_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyAnnotationsCoefficient` (read).
    pub fn urgency_annotations_coefficient() -> f32 {
        URGENCY_ANNOTATIONS_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyAnnotationsCoefficient = value`.
    pub fn set_urgency_annotations_coefficient(value: f32) {
        URGENCY_ANNOTATIONS_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyTagsCoefficient` (read).
    pub fn urgency_tags_coefficient() -> f32 {
        URGENCY_TAGS_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyTagsCoefficient = value`.
    pub fn set_urgency_tags_coefficient(value: f32) {
        URGENCY_TAGS_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyDueCoefficient` (read).
    pub fn urgency_due_coefficient() -> f32 {
        URGENCY_DUE_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyDueCoefficient = value`.
    pub fn set_urgency_due_coefficient(value: f32) {
        URGENCY_DUE_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyBlockingCoefficient` (read).
    pub fn urgency_blocking_coefficient() -> f32 {
        URGENCY_BLOCKING_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyBlockingCoefficient = value`.
    pub fn set_urgency_blocking_coefficient(value: f32) {
        URGENCY_BLOCKING_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyAgeCoefficient` (read).
    pub fn urgency_age_coefficient() -> f32 {
        URGENCY_AGE_COEFFICIENT.with(Cell::get)
    }

    /// C++ `Task::urgencyAgeCoefficient = value`.
    pub fn set_urgency_age_coefficient(value: f32) {
        URGENCY_AGE_COEFFICIENT.with(|v| v.set(value));
    }

    /// C++ `Task::urgencyAgeMax` (read).
    pub fn urgency_age_max() -> f32 {
        URGENCY_AGE_MAX.with(Cell::get)
    }

    /// C++ `Task::urgencyAgeMax = value`.
    pub fn set_urgency_age_max(value: f32) {
        URGENCY_AGE_MAX.with(|v| v.set(value));
    }

    ////////////////////////////////////////////////////////////////////////////
    // Construction.

    /// C++ `Task () = default`.
    pub fn new() -> Self {
        Task::default()
    }

    /// C++ `Task::Task (rust::Box<tc::TaskData>)`: a task read from the
    /// replica, see [`Task::parse_tc`].
    pub fn from_task_data(task: taskchampion::TaskData) -> Self {
        let mut t = Task::default();
        t.parse_tc(&task);
        t
    }

    /// C++ `void Task::parseTC (rust::Box<tc::TaskData>)`: replaces the
    /// attributes with the TaskData's properties, recounts the annotations
    /// and sets `uuid` from the TaskData's UUID (hyphenated lowercase).
    ///
    /// The C++ ends with `id = Context::getContext ().tdb2.id (data["uuid"])`.
    /// Task holds no Context, so `id` is left unchanged here (0 for a fresh
    /// task) and the caller (Tdb2, the only C++ caller) assigns
    /// `task.id = tdb2.id(uuid)` right after construction, which is
    /// observably identical.
    pub fn parse_tc(&mut self, task: &taskchampion::TaskData) {
        self.data.clear();
        for (prop, value) in task.iter() {
            self.data.insert(prop.clone(), value.clone());
        }

        // count annotations
        self.annotation_count = 0;
        for name in self.data.keys() {
            if Task::is_annotation_attr(name) {
                self.annotation_count += 1;
            }
        }

        self.data.insert("uuid".to_string(), task.get_uuid().as_hyphenated().to_string());
    }

    ////////////////////////////////////////////////////////////////////////////
    // Status.

    /// C++ `static Task::status Task::textToStatus (const std::string&)`:
    /// decided by the first character; `w` (legacy waiting) maps to pending.
    ///
    /// # Errors
    /// `The status '{1}' is not valid.` for any other first character
    /// (including an empty string, whose C++ `input[0]` is `'\0'`).
    pub fn text_to_status(input: &str) -> Result<Status> {
        match input.as_bytes().first() {
            Some(b'p') => Ok(Status::Pending),
            Some(b'c') => Ok(Status::Completed),
            Some(b'd') => Ok(Status::Deleted),
            Some(b'r') => Ok(Status::Recurring),
            // for compatibility, parse `w` as pending; Task::getStatus will
            // apply the virtual waiting status if appropriate
            Some(b'w') => Ok(Status::Pending),
            _ => Err(TwError::Message(format("The status '{1}' is not valid.", &[&input]))),
        }
    }

    /// C++ `static std::string Task::statusToText (Task::status)`.
    pub fn status_to_text(s: Status) -> &'static str {
        match s {
            Status::Pending => "pending",
            Status::Recurring => "recurring",
            Status::Waiting => "waiting",
            Status::Completed => "completed",
            Status::Deleted => "deleted",
        }
    }

    /// C++ `Task::status Task::getStatus () const`: `pending` without a
    /// `status` attribute, the virtual `waiting` for a pending task whose
    /// `wait` is in the future.
    ///
    /// # Errors
    /// The `textToStatus` error for an invalid stored status.
    pub fn get_status(&self) -> Result<Status> {
        if !self.has("status") {
            return Ok(Status::Pending);
        }

        let status = Task::text_to_status(self.get_ref("status"))?;

        // Implement the "virtual" Task::waiting status, which is not stored on-disk
        // but is defined as a pending task with a `wait` attribute in the future.
        // This is workaround for 2.6.0, remove in 3.0.0.
        if status == Status::Pending && self.is_waiting() {
            return Ok(Status::Waiting);
        }

        Ok(status)
    }

    /// `getStatus ()` as called from the `bool` predicates (`is_due`,
    /// `hasTag`, ...).
    ///
    /// Deviation: where C++ would throw "The status 'x' is not valid." out
    /// of a predicate, the status reads as `pending`. Statuses are checked
    /// by `validate` before anything is stored, so only externally
    /// corrupted data can reach this.
    fn predicate_status(&self) -> Status {
        self.get_status().unwrap_or(Status::Pending)
    }

    /// C++ `void Task::setStatus (Task::status)`: `waiting` is stored as
    /// `pending`.
    pub fn set_status(&mut self, status: Status) {
        // the 'waiting' status is a virtual version of 'pending', so translate
        // that back to 'pending' here
        let status = if status == Status::Waiting { Status::Pending } else { status };

        self.set("status", Task::status_to_text(status));

        self.recalc_urgency = true;
    }

    ////////////////////////////////////////////////////////////////////////////
    // Attribute access.

    /// C++ `const std::string Task::identifier (bool shortened = false) const`:
    /// the ID when non-zero, otherwise the (first 8 characters of the) uuid.
    pub fn identifier(&self, shortened: bool) -> String {
        if self.id != 0 {
            self.id.to_string()
        } else if shortened {
            substr_prefix(self.get_ref("uuid"), 8)
        } else {
            self.get("uuid")
        }
    }

    /// C++ `void Task::setAsNow (const std::string&)`: the current epoch.
    pub fn set_as_now(&mut self, att: &str) {
        let now = localtime::time_now().to_string();
        self.set(att, &now);

        self.recalc_urgency = true;
    }

    /// C++ `bool Task::has (const std::string&) const`.
    pub fn has(&self, name: &str) -> bool {
        self.data.contains_key(name)
    }

    /// C++ `std::vector<std::string> Task::all () const`: attribute names in
    /// map order.
    pub fn all(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    /// C++ `const std::string Task::get (const std::string&) const`: `""`
    /// when absent.
    pub fn get(&self, name: &str) -> String {
        self.get_ref(name).to_string()
    }

    /// C++ `const std::string& Task::get_ref (const std::string&) const`.
    pub fn get_ref(&self, name: &str) -> &str {
        self.data.get(name).map_or("", String::as_str)
    }

    /// C++ `int Task::get_int (const std::string&) const`: `strtol`, then the
    /// implicit `long` -> `int` conversion (truncating).
    pub fn get_int(&self, name: &str) -> i32 {
        match self.data.get(name) {
            Some(value) => c_strtol(value) as i32,
            None => 0,
        }
    }

    /// C++ `unsigned long Task::get_ulong (const std::string&) const`.
    pub fn get_ulong(&self, name: &str) -> u64 {
        match self.data.get(name) {
            Some(value) => c_strtoul(value),
            None => 0,
        }
    }

    /// C++ `float Task::get_float (const std::string&) const`.
    pub fn get_float(&self, name: &str) -> f32 {
        match self.data.get(name) {
            Some(value) => c_strtof(value),
            None => 0.0,
        }
    }

    /// C++ `time_t Task::get_date (const std::string&) const`:
    /// `(time_t) strtoul (...)`.
    pub fn get_date(&self, name: &str) -> i64 {
        match self.data.get(name) {
            Some(value) => c_strtoul(value) as i64,
            None => 0,
        }
    }

    /// C++ `void Task::set (const std::string&, const std::string&)`. As in
    /// C++ `annotation_count` is incremented for every annotation attribute
    /// set, even when it replaces an existing one.
    pub fn set(&mut self, name: &str, value: &str) {
        self.data.insert(name.to_string(), value.to_string());

        if Task::is_annotation_attr(name) {
            self.annotation_count += 1;
        }

        self.recalc_urgency = true;
    }

    /// C++ `void Task::set (const std::string&, long long)`.
    pub fn set_int(&mut self, name: &str, value: i64) {
        self.data.insert(name.to_string(), value.to_string());

        self.recalc_urgency = true;
    }

    /// C++ `void Task::remove (const std::string&)`. As in C++
    /// `annotation_count` is decremented for an annotation name even when
    /// the attribute was absent.
    pub fn remove(&mut self, name: &str) {
        if self.data.remove(name).is_some() {
            self.recalc_urgency = true;
        }

        if Task::is_annotation_attr(name) {
            self.annotation_count -= 1;
        }
    }

    /// The attribute map (C++ `data`), for code outside the crate (e.g. the
    /// `make_tc_task` binary).
    pub fn data(&self) -> &BTreeMap<String, String> {
        &self.data
    }

    ////////////////////////////////////////////////////////////////////////////
    // Date predicates.

    /// C++ `Task::dateState Task::getDateState (const std::string&) const`:
    /// where the date attribute `name` falls relative to now, today and the
    /// `rc.due` imminence window.
    pub fn get_date_state(&self, name: &str, env: &mut dyn TaskEnv) -> DateState {
        let value = self.get_date(name);
        if value > 0 {
            let reference = Datetime::from_epoch(value);
            let now = Datetime::new();
            let today = named_date("today");

            if reference < today {
                return DateState::BeforeToday;
            }

            if reference.same_day(&now) {
                if reference < now {
                    return DateState::EarlierToday;
                } else {
                    return DateState::LaterToday;
                }
            }

            let imminentperiod = env.due_days();
            if imminentperiod == 0 {
                return DateState::AfterToday;
            }

            // C++ `int * int`; overflow is undefined there, wraps here.
            let imminent_day = today + i64::from(imminentperiod.wrapping_mul(86400));
            if reference < imminent_day {
                return DateState::AfterToday;
            }
        }

        DateState::NotDue
    }

    /// C++ `bool Task::is_empty () const`: an empty task is typically a
    /// "dummy", such as in DOM evaluation.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// C++ `bool Task::is_ready () const`: pending, not blocked and either
    /// not scheduled or scheduled before now.
    pub fn is_ready(&self) -> bool {
        self.predicate_status() == Status::Pending
            && !self.is_blocked
            && (!self.has("scheduled") || named_date("now") > Datetime::from_epoch(self.get_date("scheduled")))
    }

    /// Whether the task has a `due` and is neither completed nor deleted
    /// (the common prefix of the `is_due*` predicates).
    fn has_open_due(&self) -> bool {
        if self.has("due") {
            let status = self.predicate_status();
            return status != Status::Completed && status != Status::Deleted;
        }
        false
    }

    /// C++ `bool Task::is_due () const`.
    pub fn is_due(&self, env: &mut dyn TaskEnv) -> bool {
        if self.has_open_due() {
            let state = self.get_date_state("due", env);
            if state == DateState::AfterToday || state == DateState::EarlierToday || state == DateState::LaterToday {
                return true;
            }
        }

        false
    }

    /// C++ `bool Task::is_dueyesterday () const`.
    pub fn is_dueyesterday(&self) -> bool {
        self.has_open_due() && named_date("yesterday").same_day(&Datetime::from_epoch(self.get_date("due")))
    }

    /// C++ `bool Task::is_duetoday () const`.
    pub fn is_duetoday(&self, env: &mut dyn TaskEnv) -> bool {
        if self.has_open_due() {
            let state = self.get_date_state("due", env);
            if state == DateState::EarlierToday || state == DateState::LaterToday {
                return true;
            }
        }

        false
    }

    /// C++ `bool Task::is_duetomorrow () const`.
    pub fn is_duetomorrow(&self) -> bool {
        self.has_open_due() && named_date("tomorrow").same_day(&Datetime::from_epoch(self.get_date("due")))
    }

    /// `due >= Datetime (start) && due <= Datetime (end)` for an open due.
    fn due_within(&self, start: &str, end: &str) -> bool {
        if self.has_open_due() {
            let due = Datetime::from_epoch(self.get_date("due"));
            if due >= named_date(start) && due <= named_date(end) {
                return true;
            }
        }

        false
    }

    /// C++ `bool Task::is_dueweek () const`: due between `sow` and `eow`.
    pub fn is_dueweek(&self) -> bool {
        self.due_within("sow", "eow")
    }

    /// C++ `bool Task::is_duemonth () const`: due between `som` and `eom`.
    pub fn is_duemonth(&self) -> bool {
        self.due_within("som", "eom")
    }

    /// C++ `bool Task::is_duequarter () const`: due between `soq` and `eoq`.
    pub fn is_duequarter(&self) -> bool {
        self.due_within("soq", "eoq")
    }

    /// C++ `bool Task::is_dueyear () const`: due between `soy` and `eoy`.
    pub fn is_dueyear(&self) -> bool {
        self.due_within("soy", "eoy")
    }

    /// C++ `bool Task::is_udaPresent () const`: some UDA column has a value.
    pub fn is_uda_present(&self, env: &mut dyn TaskEnv) -> bool {
        env.uda_columns().iter().any(|name| self.has(name))
    }

    /// C++ `bool Task::is_orphanPresent () const`: some attribute that is
    /// not an annotation, tag or dependency has no column.
    pub fn is_orphan_present(&self, env: &mut dyn TaskEnv) -> bool {
        for name in self.data.keys() {
            if !Task::is_annotation_attr(name)
                && !Task::is_tag_attr(name)
                && !Task::is_dep_attr(name)
                && !env.has_column(name)
            {
                return true;
            }
        }

        false
    }

    /// C++ `bool Task::is_overdue () const`.
    pub fn is_overdue(&self, env: &mut dyn TaskEnv) -> bool {
        if self.has("due") {
            let status = self.predicate_status();

            if status != Status::Completed && status != Status::Deleted && status != Status::Recurring {
                let state = self.get_date_state("due", env);
                if state == DateState::EarlierToday || state == DateState::BeforeToday {
                    return true;
                }
            }
        }

        false
    }

    /// C++ `bool Task::is_waiting () const`: the stored status is `pending`
    /// and `wait` is in the future.
    pub fn is_waiting(&self) -> bool {
        if self.has("wait") && self.get_ref("status") == "pending" {
            let now = Datetime::new();
            let wait = Datetime::from_epoch(self.get_date("wait"));
            if wait > now {
                return true;
            }
        }

        false
    }

    ////////////////////////////////////////////////////////////////////////////
    // Annotations.

    /// C++ `int Task::getAnnotationCount () const`.
    pub fn get_annotation_count(&self) -> i32 {
        self.data.keys().filter(|name| name.starts_with("annotation_")).count() as i32
    }

    /// C++ `bool Task::hasAnnotations () const`: from `annotation_count`.
    pub fn has_annotations(&self) -> bool {
        self.annotation_count != 0
    }

    /// C++ `void Task::addAnnotation (const std::string&)`: key
    /// `annotation_<now>`, incrementing the timestamp until it is unique.
    pub fn add_annotation(&mut self, description: &str) {
        let mut now = localtime::time_now();
        let mut key;

        loop {
            key = format!("annotation_{now}");
            now += 1;
            if !self.has(&key) {
                break;
            }
        }

        self.data.insert(key, description.to_string());
        self.annotation_count += 1;
        self.recalc_urgency = true;
    }

    /// C++ `void Task::removeAnnotations ()`.
    pub fn remove_annotations(&mut self) {
        // Erase old annotations.
        let before = self.data.len();
        self.data.retain(|name, _| !name.starts_with("annotation_"));
        self.annotation_count -= (before - self.data.len()) as i32;

        self.recalc_urgency = true;
    }

    /// C++ `std::map<std::string, std::string> Task::getAnnotations () const`.
    pub fn get_annotations(&self) -> BTreeMap<String, String> {
        self.data
            .iter()
            .filter(|(name, _)| name.starts_with("annotation_"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }

    /// C++ `void Task::setAnnotations (const std::map<std::string, std::string>&)`:
    /// replaces all annotations (`std::map::insert`, so an entry never
    /// overwrites an existing attribute).
    pub fn set_annotations(&mut self, annotations: &BTreeMap<String, String>) {
        // Erase old annotations.
        self.remove_annotations();

        for (name, value) in annotations {
            self.data.entry(name.clone()).or_insert_with(|| value.clone());
        }

        self.annotation_count = annotations.len() as i32;
        self.recalc_urgency = true;
    }

    ////////////////////////////////////////////////////////////////////////////
    // Dependencies (attribute side; add/remove and the TDB2 lookups need the
    // Context and live with validation).

    /// C++ `bool Task::hasDependency (const std::string&) const`.
    pub fn has_dependency(&self, uuid: &str) -> bool {
        self.has(&Task::dep2attr(uuid))
    }

    /// C++ `std::vector<std::string> Task::getDependencyUUIDs () const`: in
    /// attribute order.
    pub fn get_dependency_uuids(&self) -> Vec<String> {
        self.data.keys().filter(|name| Task::is_dep_attr(name)).map(|name| Task::attr2dep(name)).collect()
    }

    /// C++ `void Task::fixDependsAttribute ()`: fix up the old `depends`
    /// attribute to match the `dep_..` attributes (or remove it if there are
    /// no deps).
    pub fn fix_depends_attribute(&mut self) {
        let deps = self.get_dependency_uuids();
        if !deps.is_empty() {
            self.set("depends", &join(",", &deps));
        } else {
            self.remove("depends");
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    // Tags.

    /// C++ `int Task::getTagCount () const`.
    pub fn get_tag_count(&self) -> i32 {
        self.data.keys().filter(|name| Task::is_tag_attr(name)).count() as i32
    }

    /// C++ `bool Task::hasTag (const std::string&) const`: synthetic
    /// (virtual) tags for an upper-case first character, then concrete
    /// `tag_<name>` attributes.
    ///
    /// ```text
    ///              OVERDUE YESTERDAY DUE TODAY TOMORROW WEEK MONTH YEAR
    /// due:-1week      Y       -       -    -       -      ?    ?     ?
    /// due:-1day       Y       Y       -    -       -      ?    ?     ?
    /// due:today       Y       -       Y    Y       -      ?    ?     ?
    /// due:tomorrow    -       -       Y    -       Y      ?    ?     ?
    /// due:3days       -       -       Y    -       -      ?    ?     ?
    /// due:1month      -       -       -    -       -      -    -     ?
    /// due:1year       -       -       -    -       -      -    -     -
    /// ```
    pub fn has_tag(&self, tag: &str, env: &mut dyn TaskEnv) -> bool {
        // Synthetic tags - dynamically generated, but do not occupy storage space.
        // Note: This list must match that in CmdInfo::execute.
        // Note: This list must match that in ::feedback_reserved_tags.
        if tag.as_bytes().first().is_some_and(u8::is_ascii_uppercase) {
            // NOTE: This list should be kept synchronized with:
            // * the list in CmdTags.cpp for the _tags command.
            // * the list in CmdInfo.cpp for the info command.
            match tag {
                "BLOCKED" => return self.is_blocked,
                "UNBLOCKED" => return !self.is_blocked,
                "BLOCKING" => return self.is_blocking,
                "READY" => return self.is_ready(),
                "DUE" => return self.is_due(env),
                "DUETODAY" => return self.is_duetoday(env), // 2016-03-29: Deprecated in 2.6.0
                "TODAY" => return self.is_duetoday(env),
                "YESTERDAY" => return self.is_dueyesterday(),
                "TOMORROW" => return self.is_duetomorrow(),
                "OVERDUE" => return self.is_overdue(env),
                "WEEK" => return self.is_dueweek(),
                "MONTH" => return self.is_duemonth(),
                "QUARTER" => return self.is_duequarter(),
                "YEAR" => return self.is_dueyear(),
                "ACTIVE" => return self.has("start"),
                "SCHEDULED" => return self.has("scheduled"),
                "CHILD" => return self.has("parent") || self.has("template"), // 2017-01-07: Deprecated in 2.6.0
                "INSTANCE" => return self.has("template") || self.has("parent"),
                "UNTIL" => return self.has("until"),
                "ANNOTATED" => return self.has_annotations(),
                "TAGGED" => return self.get_tag_count() > 0,
                "PARENT" => return self.has("mask") || self.has("last"), // 2017-01-07: Deprecated in 2.6.0
                "TEMPLATE" => return self.has("last") || self.has("mask"),
                "WAITING" => return self.is_waiting(),
                "PENDING" => return self.predicate_status() == Status::Pending,
                "COMPLETED" => return self.predicate_status() == Status::Completed,
                "DELETED" => return self.predicate_status() == Status::Deleted,
                "UDA" => return self.is_uda_present(env),
                "ORPHAN" => return self.is_orphan_present(env),
                "LATEST" => return self.id == env.latest_id(),
                "PROJECT" => return self.has("project"),
                "PRIORITY" => return self.has("priority"),
                _ => {}
            }
        }

        // Concrete tags.
        self.has(&Task::tag2attr(tag))
    }

    /// C++ `void Task::addTag (const std::string&)`.
    pub fn add_tag(&mut self, tag: &str) {
        let attr = Task::tag2attr(tag);
        if !self.has(&attr) {
            self.set(&attr, "x");
            self.recalc_urgency = true;
            self.fix_tags_attribute();
        }
    }

    /// C++ `void Task::setTags (const std::vector<std::string>&)`: edits the
    /// tag set in place (removals first, then additions).
    pub fn set_tags(&mut self, tags: &[String]) {
        let existing = self.get_tags();

        // edit in-place, determining which should be
        // added and which should be removed
        let to_add: Vec<String> = tags.iter().filter(|tag| !existing.contains(tag)).cloned().collect();
        let to_remove: Vec<String> = self.get_tags().into_iter().filter(|tag| !tags.contains(tag)).collect();

        for tag in &to_remove {
            self.remove_tag(tag);
        }
        for tag in &to_add {
            self.add_tag(tag);
        }

        // (note: addTag / removeTag took care of recalculating urgency)
    }

    /// C++ `std::vector<std::string> Task::getTags () const`: in attribute
    /// order.
    pub fn get_tags(&self) -> Vec<String> {
        self.data.keys().filter(|name| Task::is_tag_attr(name)).map(|name| Task::attr2tag(name)).collect()
    }

    /// C++ `void Task::removeTag (const std::string&)`.
    pub fn remove_tag(&mut self, tag: &str) {
        let attr = Task::tag2attr(tag);
        if self.has(&attr) {
            self.data.remove(&attr);
            self.recalc_urgency = true;
            self.fix_tags_attribute();
        }
    }

    /// C++ `void Task::fixTagsAttribute ()`: fix up the old `tags` attribute
    /// to match the `tag_..` attributes (or remove it if there are no tags).
    pub fn fix_tags_attribute(&mut self) {
        let tags = self.get_tags();
        if !tags.is_empty() {
            self.set("tags", &join(",", &tags));
        } else {
            self.remove("tags");
        }
    }

    ////////////////////////////////////////////////////////////////////////////
    // Attribute-name helpers.

    /// C++ `static bool Task::isTagAttr (const std::string&)`.
    pub fn is_tag_attr(attr: &str) -> bool {
        attr.starts_with("tag_")
    }

    /// C++ `static std::string Task::tag2Attr (const std::string&)`.
    pub fn tag2attr(tag: &str) -> String {
        format!("tag_{tag}")
    }

    /// C++ `static std::string Task::attr2Tag (const std::string&)`: the
    /// part after `tag_` (C++ asserts `isTagAttr`; a shorter name yields
    /// `""` here).
    pub fn attr2tag(attr: &str) -> String {
        debug_assert!(Task::is_tag_attr(attr));
        attr.get(4..).unwrap_or("").to_string()
    }

    /// C++ `static bool Task::isDepAttr (const std::string&)`.
    pub fn is_dep_attr(attr: &str) -> bool {
        attr.starts_with("dep_")
    }

    /// C++ `static std::string Task::dep2Attr (const std::string&)`.
    pub fn dep2attr(uuid: &str) -> String {
        format!("dep_{uuid}")
    }

    /// C++ `static std::string Task::attr2Dep (const std::string&)`: the
    /// part after `dep_` (C++ asserts `isDepAttr`).
    pub fn attr2dep(attr: &str) -> String {
        debug_assert!(Task::is_dep_attr(attr));
        attr.get(4..).unwrap_or("").to_string()
    }

    /// C++ `static bool Task::isAnnotationAttr (const std::string&)`.
    pub fn is_annotation_attr(attr: &str) -> bool {
        attr.starts_with("annotation_")
    }

    /// C++ `std::vector<std::string> Task::getUDAOrphans () const`: the
    /// attributes (in map order) that have no column and are not
    /// annotations, tags or dependencies.
    pub fn get_uda_orphans(&self, env: &mut dyn TaskEnv) -> Vec<String> {
        self.data
            .keys()
            .filter(|name| !env.has_column(name))
            .filter(|name| !(Task::is_annotation_attr(name) || Task::is_tag_attr(name) || Task::is_dep_attr(name)))
            .cloned()
            .collect()
    }

    ////////////////////////////////////////////////////////////////////////////
    // Serialization helpers.

    /// C++ `const std::string Task::encode (const std::string&) const`:
    /// `[` -> `&open;`, `]` -> `&close;`.
    pub fn encode(&self, value: &str) -> String {
        let modified = str_replace(value, "[", "&open;");
        str_replace(&modified, "]", "&close;")
    }

    /// C++ `const std::string Task::decode (const std::string&) const`:
    /// `&open;` -> `[`, `&close;` -> `]`.
    pub fn decode(&self, value: &str) -> String {
        if !value.contains('&') {
            return value.to_string();
        }

        let modified = str_replace(value, "&open;", "[");
        str_replace(&modified, "&close;", "]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::column::Column;
    use std::rc::Rc;

    const DAY: i64 = 86400;

    /// A configurable [`TaskEnv`] for tests.
    struct TestEnv {
        due: i32,
        columns: Vec<String>,
        udas: Vec<String>,
        latest: i32,
    }

    impl TaskEnv for TestEnv {
        fn due_days(&self) -> i32 {
            self.due
        }
        fn has_column(&self, name: &str) -> bool {
            self.columns.iter().any(|c| c == name)
        }
        fn uda_columns(&self) -> Vec<String> {
            self.udas.clone()
        }
        fn latest_id(&mut self) -> i32 {
            self.latest
        }
    }

    fn env() -> TestEnv {
        TestEnv { due: 7, columns: Vec::new(), udas: Vec::new(), latest: 0 }
    }

    fn now() -> i64 {
        localtime::time_now()
    }

    fn epoch_of(name: &str) -> i64 {
        named_date(name).to_epoch()
    }

    fn pending_due(due: i64) -> Task {
        let mut t = Task::new();
        t.set("status", "pending");
        t.set_int("due", due);
        t
    }

    fn virtual_tags(t: &Task, e: &mut dyn TaskEnv) -> Vec<&'static str> {
        [
            "BLOCKED",
            "UNBLOCKED",
            "BLOCKING",
            "READY",
            "DUE",
            "DUETODAY",
            "TODAY",
            "YESTERDAY",
            "TOMORROW",
            "OVERDUE",
            "WEEK",
            "MONTH",
            "QUARTER",
            "YEAR",
            "ACTIVE",
            "SCHEDULED",
            "CHILD",
            "INSTANCE",
            "UNTIL",
            "ANNOTATED",
            "TAGGED",
            "PARENT",
            "TEMPLATE",
            "WAITING",
            "PENDING",
            "COMPLETED",
            "DELETED",
            "UDA",
            "ORPHAN",
            "LATEST",
            "PROJECT",
            "PRIORITY",
        ]
        .into_iter()
        .filter(|tag| t.has_tag(tag, e))
        .collect()
    }

    #[test]
    fn defaults() {
        let t = Task::default();
        assert_eq!(t.id, 0);
        assert_eq!(t.urgency_value, 0.0);
        assert!(t.recalc_urgency);
        assert!(!t.is_blocked);
        assert!(!t.is_blocking);
        assert_eq!(t.annotation_count, 0);
        assert!(t.is_empty());
        assert_eq!(Task::new().data().len(), 0);
    }

    #[test]
    fn statics_defaults_and_setters() {
        assert_eq!(Task::default_project(), "");
        assert_eq!(Task::default_due(), "");
        assert_eq!(Task::default_scheduled(), "");
        assert!(Task::search_case_sensitive());
        assert!(!Task::regex());
        assert!(Task::attributes().is_empty());
        assert!(Task::coefficients().is_empty());
        assert!(Task::custom_order().is_empty());
        for get in [
            Task::urgency_project_coefficient,
            Task::urgency_active_coefficient,
            Task::urgency_scheduled_coefficient,
            Task::urgency_waiting_coefficient,
            Task::urgency_blocked_coefficient,
            Task::urgency_annotations_coefficient,
            Task::urgency_tags_coefficient,
            Task::urgency_due_coefficient,
            Task::urgency_blocking_coefficient,
            Task::urgency_age_coefficient,
            Task::urgency_age_max,
        ] {
            assert_eq!(get(), 0.0);
        }

        // Thread-local: set and restore within this test thread.
        Task::set_default_project("home");
        Task::set_default_due("eom");
        Task::set_default_scheduled("tomorrow");
        Task::set_search_case_sensitive(false);
        Task::set_regex(true);
        Task::set_attributes(BTreeMap::from([("due".to_string(), "date".to_string())]));
        Task::set_coefficients(BTreeMap::from([("urgency.user.tag.x.coefficient".to_string(), 1.5f32)]));
        Task::set_custom_order(BTreeMap::from([("priority".to_string(), vec!["L".to_string()])]));
        type Setter = fn(f32);
        type Getter = fn() -> f32;
        let setters: [(Setter, Getter); 11] = [
            (Task::set_urgency_project_coefficient, Task::urgency_project_coefficient),
            (Task::set_urgency_active_coefficient, Task::urgency_active_coefficient),
            (Task::set_urgency_scheduled_coefficient, Task::urgency_scheduled_coefficient),
            (Task::set_urgency_waiting_coefficient, Task::urgency_waiting_coefficient),
            (Task::set_urgency_blocked_coefficient, Task::urgency_blocked_coefficient),
            (Task::set_urgency_annotations_coefficient, Task::urgency_annotations_coefficient),
            (Task::set_urgency_tags_coefficient, Task::urgency_tags_coefficient),
            (Task::set_urgency_due_coefficient, Task::urgency_due_coefficient),
            (Task::set_urgency_blocking_coefficient, Task::urgency_blocking_coefficient),
            (Task::set_urgency_age_coefficient, Task::urgency_age_coefficient),
            (Task::set_urgency_age_max, Task::urgency_age_max),
        ];
        for (i, (set, get)) in setters.iter().enumerate() {
            set(i as f32 + 0.5);
            assert_eq!(get(), i as f32 + 0.5);
        }
        assert_eq!(Task::default_project(), "home");
        assert_eq!(Task::default_due(), "eom");
        assert_eq!(Task::default_scheduled(), "tomorrow");
        assert!(!Task::search_case_sensitive());
        assert!(Task::regex());
        assert_eq!(Task::with_attributes(|a| a.get("due").cloned()), Some("date".to_string()));
        assert_eq!(Task::with_coefficients(|c| c.len()), 1);
        assert_eq!(Task::with_custom_order(|c| c["priority"].clone()), vec!["L".to_string()]);

        Task::set_default_project("");
        Task::set_default_due("");
        Task::set_default_scheduled("");
        Task::set_search_case_sensitive(true);
        Task::set_regex(false);
        Task::set_attributes(BTreeMap::new());
        Task::set_coefficients(BTreeMap::new());
        Task::set_custom_order(BTreeMap::new());
        for (set, _) in setters {
            set(0.0);
        }
    }

    #[test]
    fn status_conversion() {
        assert_eq!(Task::text_to_status("pending").unwrap(), Status::Pending);
        assert_eq!(Task::text_to_status("completed").unwrap(), Status::Completed);
        assert_eq!(Task::text_to_status("deleted").unwrap(), Status::Deleted);
        assert_eq!(Task::text_to_status("recurring").unwrap(), Status::Recurring);
        assert_eq!(Task::text_to_status("waiting").unwrap(), Status::Pending);
        // Only the first character counts.
        assert_eq!(Task::text_to_status("p").unwrap(), Status::Pending);
        assert_eq!(Task::text_to_status("dx").unwrap(), Status::Deleted);
        match Task::text_to_status("foo") {
            Err(TwError::Message(m)) => assert_eq!(m, "The status 'foo' is not valid."),
            other => panic!("unexpected {other:?}"),
        }
        match Task::text_to_status("") {
            Err(TwError::Message(m)) => assert_eq!(m, "The status '' is not valid."),
            other => panic!("unexpected {other:?}"),
        }
        assert!(Task::text_to_status("Pending").is_err());

        assert_eq!(Task::status_to_text(Status::Pending), "pending");
        assert_eq!(Task::status_to_text(Status::Recurring), "recurring");
        assert_eq!(Task::status_to_text(Status::Waiting), "waiting");
        assert_eq!(Task::status_to_text(Status::Completed), "completed");
        assert_eq!(Task::status_to_text(Status::Deleted), "deleted");
    }

    #[test]
    fn get_set_status() {
        let mut t = Task::new();
        assert_eq!(t.get_status().unwrap(), Status::Pending);
        t.recalc_urgency = false;
        t.set_status(Status::Waiting);
        assert_eq!(t.get("status"), "pending");
        assert!(t.recalc_urgency);
        t.set_status(Status::Completed);
        assert_eq!(t.get_status().unwrap(), Status::Completed);
        t.set_status(Status::Deleted);
        assert_eq!(t.get_status().unwrap(), Status::Deleted);
        t.set_status(Status::Recurring);
        assert_eq!(t.get_status().unwrap(), Status::Recurring);

        // Virtual waiting status.
        t.set_status(Status::Pending);
        t.set_int("wait", now() + DAY);
        assert_eq!(t.get_status().unwrap(), Status::Waiting);
        assert!(t.is_waiting());
        t.set_int("wait", now() - DAY);
        assert_eq!(t.get_status().unwrap(), Status::Pending);
        assert!(!t.is_waiting());
        // Stored "waiting" is pending, but is_waiting needs "pending" literally.
        t.set("status", "waiting");
        t.set_int("wait", now() + DAY);
        assert!(!t.is_waiting());
        assert_eq!(t.get_status().unwrap(), Status::Pending);

        t.set("status", "bogus");
        assert!(t.get_status().is_err());
        // Predicates treat an invalid status as pending.
        assert!(t.has_tag("PENDING", &mut NullEnv));
    }

    #[test]
    fn identifier() {
        let mut t = Task::new();
        t.set("uuid", "a360fc44-315c-4366-b70c-ea7e7520b749");
        assert_eq!(t.identifier(false), "a360fc44-315c-4366-b70c-ea7e7520b749");
        assert_eq!(t.identifier(true), "a360fc44");
        t.id = 12;
        assert_eq!(t.identifier(false), "12");
        assert_eq!(t.identifier(true), "12");
        let empty = Task::new();
        assert_eq!(empty.identifier(true), "");
    }

    #[test]
    fn typed_getters() {
        let mut t = Task::new();
        t.set("a", "  42abc");
        t.set("b", "-7");
        t.set("c", "3.5e1x");
        t.set("d", "99999999999");
        t.set("e", "junk");
        t.set("f", "0x10");

        assert!(t.has("a"));
        assert!(!t.has("z"));
        assert_eq!(t.all(), vec!["a", "b", "c", "d", "e", "f"]);
        assert_eq!(t.get("z"), "");
        assert_eq!(t.get_ref("a"), "  42abc");
        assert_eq!(t.get_ref("z"), "");

        assert_eq!(t.get_int("a"), 42);
        assert_eq!(t.get_int("b"), -7);
        assert_eq!(t.get_int("e"), 0);
        assert_eq!(t.get_int("f"), 0);
        assert_eq!(t.get_int("z"), 0);
        // long -> int truncation.
        assert_eq!(t.get_int("d"), 99_999_999_999i64 as i32);

        assert_eq!(t.get_ulong("a"), 42);
        assert_eq!(t.get_ulong("b"), (-7i64) as u64);
        assert_eq!(t.get_ulong("z"), 0);

        assert_eq!(t.get_float("c"), 35.0);
        assert_eq!(t.get_float("e"), 0.0);
        assert_eq!(t.get_float("z"), 0.0);

        assert_eq!(t.get_date("d"), 99_999_999_999);
        assert_eq!(t.get_date("b"), -7);
        assert_eq!(t.get_date("z"), 0);
    }

    #[test]
    fn set_remove_and_recalc() {
        let mut t = Task::new();
        t.recalc_urgency = false;
        t.set_int("due", 1234567890);
        assert_eq!(t.get("due"), "1234567890");
        assert!(t.recalc_urgency);
        t.set_int("n", -5);
        assert_eq!(t.get("n"), "-5");

        t.recalc_urgency = false;
        t.remove("missing");
        assert!(!t.recalc_urgency);
        t.remove("due");
        assert!(t.recalc_urgency);
        assert!(!t.has("due"));

        let before = now();
        t.recalc_urgency = false;
        t.set_as_now("start");
        let start = t.get_date("start");
        assert!(start >= before && start <= now());
        assert!(t.recalc_urgency);
    }

    #[test]
    fn annotation_count_via_set_remove() {
        let mut t = Task::new();
        t.set("annotation_1", "one");
        assert_eq!(t.annotation_count, 1);
        // C++ increments even when replacing.
        t.set("annotation_1", "uno");
        assert_eq!(t.annotation_count, 2);
        assert_eq!(t.get_annotation_count(), 1);
        t.remove("annotation_1");
        assert_eq!(t.annotation_count, 1);
        // ... and decrements even when absent.
        t.remove("annotation_2");
        assert_eq!(t.annotation_count, 0);
    }

    #[test]
    fn annotations() {
        let mut t = Task::new();
        assert!(!t.has_annotations());
        assert_eq!(t.get_annotation_count(), 0);

        t.recalc_urgency = false;
        let before = now();
        t.add_annotation("first");
        t.add_annotation("second");
        assert!(t.recalc_urgency);
        assert_eq!(t.annotation_count, 2);
        assert!(t.has_annotations());
        assert_eq!(t.get_annotation_count(), 2);
        let annos = t.get_annotations();
        assert_eq!(annos.len(), 2);
        let keys: Vec<i64> = annos.keys().map(|k| k["annotation_".len()..].parse().unwrap()).collect();
        // Unique timestamps: the second is one second after the first (or
        // later, if the clock moved).
        assert!(keys[0] >= before);
        assert!(keys[1] > keys[0]);
        assert_eq!(annos.values().cloned().collect::<Vec<_>>(), vec!["first", "second"]);

        t.set("description", "d");
        t.remove_annotations();
        assert_eq!(t.annotation_count, 0);
        assert!(!t.has_annotations());
        assert_eq!(t.get_annotations().len(), 0);
        assert!(t.has("description"));

        let mut set = BTreeMap::new();
        set.insert("annotation_100".to_string(), "a".to_string());
        set.insert("annotation_200".to_string(), "b".to_string());
        set.insert("description".to_string(), "not overwritten".to_string());
        t.add_annotation("old");
        t.recalc_urgency = false;
        t.set_annotations(&set);
        assert!(t.recalc_urgency);
        assert_eq!(t.annotation_count, 3); // annotations.size ()
        assert_eq!(t.get_annotation_count(), 2);
        assert_eq!(t.get("annotation_100"), "a");
        assert_eq!(t.get("description"), "d");
        assert_eq!(VariantTask::get_annotations(&t).len(), 2);
    }

    #[test]
    fn tags() {
        let mut t = Task::new();
        assert_eq!(t.get_tag_count(), 0);
        t.recalc_urgency = false;
        t.add_tag("home");
        assert!(t.recalc_urgency);
        assert!(t.has("tag_home"));
        assert_eq!(t.get("tag_home"), "x");
        assert_eq!(t.get("tags"), "home");
        t.add_tag("garden");
        assert_eq!(t.get_tags(), vec!["garden", "home"]);
        assert_eq!(t.get("tags"), "garden,home");
        assert_eq!(t.get_tag_count(), 2);
        assert!(t.has_tag("home", &mut NullEnv));
        assert!(!t.has_tag("work", &mut NullEnv));
        assert!(VariantTask::has_tag(&t, "garden"));

        t.recalc_urgency = false;
        t.add_tag("home");
        assert!(!t.recalc_urgency);

        t.remove_tag("nope");
        assert!(!t.recalc_urgency);
        t.remove_tag("garden");
        assert!(t.recalc_urgency);
        assert_eq!(t.get("tags"), "home");
        t.remove_tag("home");
        assert!(!t.has("tags"));
        assert_eq!(t.get_tag_count(), 0);

        t.set_tags(&["a".to_string(), "b".to_string()]);
        assert_eq!(t.get_tags(), vec!["a", "b"]);
        t.set_tags(&["b".to_string(), "c".to_string()]);
        assert_eq!(t.get_tags(), vec!["b", "c"]);
        assert_eq!(t.get("tags"), "b,c");
        t.set_tags(&[]);
        assert!(t.get_tags().is_empty());
        assert!(!t.has("tags"));

        // A concrete upper-case tag that is not virtual.
        t.add_tag("FOO");
        assert!(t.has_tag("FOO", &mut NullEnv));
        // A concrete tag shadowed by a virtual one.
        t.add_tag("PROJECT");
        assert!(!t.has_tag("PROJECT", &mut NullEnv));
        assert!(!t.has_tag("", &mut NullEnv));
    }

    #[test]
    fn fix_tags_attribute_rebuilds() {
        let mut t = Task::new();
        t.set("tags", "stale");
        t.fix_tags_attribute();
        assert!(!t.has("tags"));
        t.set("tag_x", "x");
        t.set("tag_a", "x");
        t.fix_tags_attribute();
        assert_eq!(t.get("tags"), "a,x");
    }

    #[test]
    fn attr_helpers() {
        assert!(Task::is_tag_attr("tag_x"));
        assert!(Task::is_tag_attr("tag_"));
        assert!(!Task::is_tag_attr("tags"));
        assert!(!Task::is_tag_attr("xtag_"));
        assert_eq!(Task::tag2attr("next"), "tag_next");
        assert_eq!(Task::attr2tag("tag_next"), "next");
        assert!(Task::is_dep_attr("dep_abc"));
        assert!(!Task::is_dep_attr("depends"));
        assert_eq!(Task::dep2attr("abc"), "dep_abc");
        assert_eq!(Task::attr2dep("dep_abc"), "abc");
        assert!(Task::is_annotation_attr("annotation_1"));
        assert!(Task::is_annotation_attr("annotation_"));
        assert!(!Task::is_annotation_attr("annotation"));
    }

    #[test]
    fn dependencies() {
        let mut t = Task::new();
        assert!(t.get_dependency_uuids().is_empty());
        t.set("dep_bbb", "x");
        t.set("dep_aaa", "x");
        assert!(t.has_dependency("aaa"));
        assert!(!t.has_dependency("ccc"));
        assert_eq!(t.get_dependency_uuids(), vec!["aaa", "bbb"]);
        t.fix_depends_attribute();
        assert_eq!(t.get("depends"), "aaa,bbb");
        t.remove("dep_aaa");
        t.remove("dep_bbb");
        t.fix_depends_attribute();
        assert!(!t.has("depends"));
    }

    #[test]
    fn equality_matches_cpp() {
        let mut a = Task::new();
        a.set("uuid", "1");
        a.set("description", "x");
        let mut b = Task::new();
        b.set("uuid", "2");
        b.set("description", "x");
        b.id = 5;
        b.urgency_value = 3.0;
        assert!(a == b);

        b.set("description", "y");
        assert!(a != b);
        b.set("description", "x");
        b.set("project", "p");
        assert!(a != b); // size differs

        // Asymmetric corner case kept from C++.
        let mut c = Task::new();
        c.set("uuid", "1");
        let mut d = Task::new();
        d.set("other", "v");
        assert!(c == d);
        assert!(d != c);

        assert!(Task::new() == Task::new());
    }

    #[test]
    fn encode_decode() {
        let t = Task::new();
        assert_eq!(t.encode("a[b]c"), "a&open;b&close;c");
        assert_eq!(t.decode("a&open;b&close;c"), "a[b]c");
        assert_eq!(t.decode("plain"), "plain");
        assert_eq!(t.decode("&amp;"), "&amp;");
        assert_eq!(t.decode(&t.encode("[[x]]")), "[[x]]");
    }

    #[test]
    fn date_state() {
        let mut e = env();
        let t = Task::new();
        assert_eq!(t.get_date_state("due", &mut e), DateState::NotDue);

        let today = epoch_of("today");
        let mut t = Task::new();
        t.set_int("due", today - 1);
        assert_eq!(t.get_date_state("due", &mut e), DateState::BeforeToday);
        t.set_int("due", today + 1);
        assert_eq!(t.get_date_state("due", &mut e), DateState::EarlierToday);
        t.set_int("due", epoch_of("tomorrow") - 1);
        assert_eq!(t.get_date_state("due", &mut e), DateState::LaterToday);
        t.set_int("due", epoch_of("tomorrow") + 3600);
        assert_eq!(t.get_date_state("due", &mut e), DateState::AfterToday);
        t.set_int("due", today + 30 * DAY);
        assert_eq!(t.get_date_state("due", &mut e), DateState::NotDue);
        e.due = 0;
        assert_eq!(t.get_date_state("due", &mut e), DateState::AfterToday);
        e.due = 7;
        t.set_int("due", today + 7 * DAY + 1);
        assert_eq!(t.get_date_state("due", &mut e), DateState::NotDue);
        t.set_int("due", today + 7 * DAY - 1);
        assert_eq!(t.get_date_state("due", &mut e), DateState::AfterToday);
        t.set_int("due", 0);
        assert_eq!(t.get_date_state("due", &mut e), DateState::NotDue);
    }

    #[test]
    fn due_predicates() {
        let mut e = env();
        let today = epoch_of("today");

        // Overdue by a week.
        let t = pending_due(today - 7 * DAY);
        assert!(t.is_overdue(&mut e));
        assert!(!t.is_due(&mut e));
        assert!(!t.is_duetoday(&mut e));
        assert!(!t.is_dueyesterday());
        assert!(!t.is_duetomorrow());

        // Yesterday.
        let t = pending_due(epoch_of("yesterday") + 3600);
        assert!(t.is_overdue(&mut e));
        assert!(t.is_dueyesterday());
        assert!(!t.is_due(&mut e));

        // Today (end of the day: later today).
        let t = pending_due(epoch_of("tomorrow") - 1);
        assert!(t.is_due(&mut e));
        assert!(t.is_duetoday(&mut e));
        assert!(!t.is_overdue(&mut e));
        assert!(!t.is_duetomorrow());
        assert!(t.is_dueweek());
        assert!(t.is_duemonth());
        assert!(t.is_duequarter());
        assert!(t.is_dueyear());

        // Today, earlier: overdue and due today.
        let t = pending_due(today + 1);
        if today + 1 < now() {
            assert!(t.is_overdue(&mut e));
            assert!(t.is_duetoday(&mut e));
        }

        // Tomorrow.
        let t = pending_due(epoch_of("tomorrow") + 3600);
        assert!(t.is_due(&mut e));
        assert!(t.is_duetomorrow());
        assert!(!t.is_duetoday(&mut e));
        assert!(!t.is_overdue(&mut e));

        // In a year and a day: none.
        let t = pending_due(epoch_of("eoy") + DAY);
        assert!(!t.is_due(&mut e));
        assert!(!t.is_dueyear());
        assert!(!t.is_duequarter());
        assert!(!t.is_duemonth());
        assert!(!t.is_dueweek());

        // Range boundaries are inclusive.
        let t = pending_due(epoch_of("sow"));
        assert!(t.is_dueweek());
        let t = pending_due(epoch_of("eom"));
        assert!(t.is_duemonth());
        let t = pending_due(epoch_of("soq"));
        assert!(t.is_duequarter());
        let t = pending_due(epoch_of("soy") - 1);
        assert!(!t.is_dueyear());

        // Completed/deleted tasks are never due.
        for status in ["completed", "deleted"] {
            let mut t = pending_due(epoch_of("tomorrow") - 1);
            t.set("status", status);
            assert!(!t.is_due(&mut e));
            assert!(!t.is_duetoday(&mut e));
            assert!(!t.is_overdue(&mut e));
            assert!(!t.is_dueyear());
            let mut t = pending_due(epoch_of("yesterday") + 1);
            t.set("status", status);
            assert!(!t.is_dueyesterday());
            assert!(!t.is_overdue(&mut e));
        }

        // Recurring templates are due but never overdue.
        let mut t = pending_due(epoch_of("yesterday") + 3600);
        t.set("status", "recurring");
        assert!(!t.is_overdue(&mut e));
        assert!(t.is_dueyesterday());

        // No due.
        let t = Task::new();
        assert!(!t.is_due(&mut e));
        assert!(!t.is_overdue(&mut e));
        assert!(!t.is_dueweek());
    }

    #[test]
    fn ready() {
        let mut t = Task::new();
        t.set("status", "pending");
        assert!(t.is_ready());
        t.set_int("scheduled", now() - DAY);
        assert!(t.is_ready());
        t.set_int("scheduled", now() + DAY);
        assert!(!t.is_ready());
        t.remove("scheduled");
        t.is_blocked = true;
        assert!(!t.is_ready());
        t.is_blocked = false;
        t.set("status", "completed");
        assert!(!t.is_ready());
        // Waiting is not pending.
        t.set("status", "pending");
        t.set_int("wait", now() + DAY);
        assert!(!t.is_ready());
    }

    #[test]
    fn virtual_tags_attributes() {
        let mut e = env();
        let mut t = Task::new();
        t.set("status", "pending");
        assert_eq!(virtual_tags(&t, &mut e), vec!["UNBLOCKED", "READY", "PENDING", "ORPHAN", "LATEST"]);

        t.id = 3;
        t.is_blocked = true;
        t.is_blocking = true;
        t.set_as_now("start");
        t.set_int("scheduled", now() + DAY);
        t.set("parent", "p");
        t.set_int("until", now() + 10 * DAY);
        t.add_annotation("a");
        t.add_tag("t");
        t.set("project", "home");
        t.set("priority", "H");
        e.latest = 3;
        assert_eq!(
            virtual_tags(&t, &mut e),
            vec![
                "BLOCKED",
                "BLOCKING",
                "ACTIVE",
                "SCHEDULED",
                "CHILD",
                "INSTANCE",
                "UNTIL",
                "ANNOTATED",
                "TAGGED",
                "PENDING",
                "ORPHAN",
                "LATEST",
                "PROJECT",
                "PRIORITY"
            ]
        );

        let mut t = Task::new();
        t.set("status", "recurring");
        t.set("mask", "--");
        t.id = 1;
        assert_eq!(virtual_tags(&t, &mut e), vec!["UNBLOCKED", "PARENT", "TEMPLATE", "ORPHAN"]);
        let mut t = Task::new();
        t.set("last", "1");
        assert!(t.has_tag("TEMPLATE", &mut e));
        assert!(t.has_tag("PARENT", &mut e));
        let mut t = Task::new();
        t.set("template", "u");
        assert!(t.has_tag("INSTANCE", &mut e));
        assert!(t.has_tag("CHILD", &mut e));
    }

    #[test]
    fn virtual_tags_status() {
        let mut e = env();
        let mut t = Task::new();
        t.set("status", "completed");
        assert!(t.has_tag("COMPLETED", &mut e));
        assert!(!t.has_tag("PENDING", &mut e));
        assert!(!t.has_tag("READY", &mut e));
        t.set("status", "deleted");
        assert!(t.has_tag("DELETED", &mut e));
        t.set("status", "pending");
        t.set_int("wait", now() + DAY);
        assert!(t.has_tag("WAITING", &mut e));
        assert!(!t.has_tag("PENDING", &mut e));
        // No status attribute reads as pending.
        let t = Task::new();
        assert!(t.has_tag("PENDING", &mut e));
    }

    #[test]
    fn virtual_tags_dates() {
        let mut e = env();
        let today = epoch_of("today");

        let t = pending_due(epoch_of("tomorrow") - 1);
        assert!(t.has_tag("DUE", &mut e));
        assert!(t.has_tag("DUETODAY", &mut e));
        assert!(t.has_tag("TODAY", &mut e));
        assert!(!t.has_tag("OVERDUE", &mut e));
        assert!(!t.has_tag("TOMORROW", &mut e));
        assert!(!t.has_tag("YESTERDAY", &mut e));
        assert!(t.has_tag("MONTH", &mut e));
        assert!(t.has_tag("QUARTER", &mut e));
        assert!(t.has_tag("YEAR", &mut e));
        assert_eq!(t.has_tag("WEEK", &mut e), t.is_dueweek());

        let t = pending_due(epoch_of("tomorrow") + 1);
        assert!(t.has_tag("TOMORROW", &mut e));
        assert!(t.has_tag("DUE", &mut e));
        assert!(!t.has_tag("TODAY", &mut e));

        let t = pending_due(epoch_of("yesterday") + 1);
        assert!(t.has_tag("YESTERDAY", &mut e));
        assert!(t.has_tag("OVERDUE", &mut e));
        assert!(!t.has_tag("DUE", &mut e));

        // The rc.due window comes from the environment.
        let t = pending_due(today + 5 * DAY);
        assert!(t.has_tag("DUE", &mut e));
        e.due = 2;
        assert!(!t.has_tag("DUE", &mut e));
        e.due = 0;
        assert!(t.has_tag("DUE", &mut e));
        // NullEnv uses the default rc.due=7.
        assert!(t.has_tag("DUE", &mut NullEnv));
        let t = pending_due(today + 9 * DAY);
        assert!(!t.has_tag("DUE", &mut NullEnv));
    }

    #[test]
    fn uda_and_orphans() {
        let mut e = env();
        e.columns = vec!["description".into(), "status".into(), "estimate".into()];
        e.udas = vec!["estimate".into()];

        let mut t = Task::new();
        t.set("description", "d");
        t.set("status", "pending");
        t.add_tag("x");
        t.add_annotation("a");
        t.set("dep_abc", "x");
        assert!(!t.has_tag("UDA", &mut e));
        // `tags` itself has no column here, so it is an orphan.
        assert!(t.has_tag("ORPHAN", &mut e));
        assert_eq!(t.get_uda_orphans(&mut e), vec!["tags"]);
        e.columns.push("tags".into());
        assert!(!t.is_orphan_present(&mut e));
        assert!(t.get_uda_orphans(&mut e).is_empty());

        t.set("estimate", "3");
        assert!(t.is_uda_present(&mut e));
        assert!(t.has_tag("UDA", &mut e));

        t.set("zzz", "1");
        t.set("aaa", "1");
        assert!(t.is_orphan_present(&mut e));
        assert_eq!(t.get_uda_orphans(&mut e), vec!["aaa", "zzz"]);

        // NullEnv: no columns, so everything but tags/annotations/deps is orphan.
        assert!(!t.is_uda_present(&mut NullEnv));
        assert_eq!(t.get_uda_orphans(&mut NullEnv), vec!["aaa", "description", "estimate", "status", "tags", "zzz"]);
    }

    #[test]
    fn latest() {
        let mut e = env();
        let mut t = Task::new();
        t.id = 4;
        e.latest = 4;
        assert!(t.has_tag("LATEST", &mut e));
        e.latest = 5;
        assert!(!t.has_tag("LATEST", &mut e));
        // NullEnv latest_id is 0.
        assert!(!t.has_tag("LATEST", &mut NullEnv));
    }

    #[test]
    fn task_with_env_adapter() {
        let mut e = env();
        e.columns = vec!["estimate".into()];
        e.udas = vec!["estimate".into()];
        e.latest = 2;
        let mut t = Task::new();
        t.id = 2;
        t.set("estimate", "1");
        t.add_annotation("note");
        {
            let adapter = TaskWithEnv::new(&t, &mut e);
            let vt: &dyn VariantTask = &adapter;
            assert!(vt.has_tag("UDA"));
            assert!(vt.has_tag("LATEST"));
            assert!(vt.has_tag("ANNOTATED"));
            assert_eq!(vt.get_annotations().len(), 1);
            assert_eq!(adapter.task().id, 2);
        }
        // Plain Task as VariantTask uses NullEnv.
        assert!(!VariantTask::has_tag(&t, "UDA"));
        assert!(!VariantTask::has_tag(&t, "LATEST"));
    }

    struct UdaCol(bool);
    impl Column for UdaCol {
        fn is_uda(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn context_env() {
        let mut ctx = Context::default();
        ctx.config.set("due", "3");
        ctx.columns.insert("estimate".to_string(), Rc::new(UdaCol(true)));
        ctx.columns.insert("description".to_string(), Rc::new(UdaCol(false)));
        assert_eq!(TaskEnv::due_days(&ctx), 3);
        assert!(ctx.has_column("description"));
        assert!(!ctx.has_column("project"));
        assert_eq!(ctx.uda_columns(), vec!["estimate"]);
        assert_eq!(TaskEnv::latest_id(&mut ctx), 0);

        let mut t = Task::new();
        t.set("estimate", "2");
        assert!(t.has_tag("UDA", &mut ctx));
    }

    #[test]
    fn parse_tc_from_task_data() {
        let uuid = taskchampion::Uuid::parse_str("a360fc44-315c-4366-b70c-ea7e7520b749").unwrap();
        let mut ops = taskchampion::Operations::new();
        let mut td = taskchampion::TaskData::create(uuid, &mut ops);
        td.update("description", Some("hello".to_string()), &mut ops);
        td.update("status", Some("pending".to_string()), &mut ops);
        td.update("annotation_1", Some("one".to_string()), &mut ops);
        td.update("annotation_2", Some("two".to_string()), &mut ops);
        td.update("tag_x", Some(String::new()), &mut ops);
        // A stored "uuid" property is overridden by the TaskData's UUID.
        td.update("uuid", Some("bogus".to_string()), &mut ops);

        let t = Task::from_task_data(td.clone());
        assert_eq!(t.id, 0);
        assert_eq!(t.get("uuid"), "a360fc44-315c-4366-b70c-ea7e7520b749");
        assert_eq!(t.get("description"), "hello");
        assert_eq!(t.annotation_count, 2);
        assert_eq!(t.get_tags(), vec!["x"]);
        assert_eq!(t.all(), vec!["annotation_1", "annotation_2", "description", "status", "tag_x", "uuid"]);
        assert!(t.recalc_urgency);

        // parse_tc replaces previous data and keeps the id.
        let mut u = Task::new();
        u.id = 9;
        u.set("project", "old");
        u.parse_tc(&td);
        assert!(!u.has("project"));
        assert_eq!(u.id, 9);
        assert_eq!(u.get("description"), "hello");
    }
}
