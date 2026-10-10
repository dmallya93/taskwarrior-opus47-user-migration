//! Rust translation of `src/Task.h` / `src/Task.cpp`.
//!
//! This part holds the core data model: the process-wide `Task::*` statics,
//! the [`Status`] / [`DateState`] / [`ModType`] enums, the ordered attribute
//! map ([`Task::data`], C++ `std::map<std::string, std::string> data`),
//! typed getters and setters, the virtual-tag predicates, tags, annotations,
//! dependency attributes, `encode`/`decode`, and `parseTC` (the
//! `Task (rust::Box<tc::TaskData>)` constructor) over the native
//! [`taskchampion::TaskData`], the JSON/legacy parsers ([`Task::parse`],
//! [`Task::parse_json`], [`Task::parse_json_object`], [`Task::parse_legacy`],
//! [`Task::determine_version`]) and [`Task::compose_json`], urgency ([`Task::urgency`], every coefficient
//! term, the user tag/project/keyword/UDA coefficients and
//! `urgency.inherit`, in C++ `float`/`double` evaluation order), the
//! command-line modification [`Task::modify`] / [`Task::substitute`], and
//! the later-milestone stub [`Task::diff`] (milestone 7).
//!
//! API mapping:
//! * C++ statics are thread-local getter/setter pairs on [`Task`]
//!   (`Task::defaultProject` -> [`Task::default_project`] /
//!   [`Task::set_default_project`], ...), defaults as in C++.
//! * `getStatus` -> [`Task::get_status`] (`Result`, since `textToStatus`
//!   throws). The predicates that call it in C++ (`is_ready`, `is_due*`,
//!   `is_overdue`) and `hasTag` (which also reads `tdb2.latest_id ()`)
//!   return `Result<bool>`, so those throws propagate with the C++ text.
//! * C++ methods that read `Context::getContext ()` take a
//!   `&mut dyn` [`TaskEnv`] (`rc.due`, the column registry,
//!   `tdb2.latest_id ()`). [`Context`] implements it; [`NullEnv`] is the
//!   context-free stand-in, and [`TaskWithEnv`] binds a task to an
//!   environment so DOM/filter code can hand it to Variant as a
//!   [`VariantTask`].
//! * Overloads become distinct names: `set (name, long long)` ->
//!   [`Task::set_int`]; `Task (rust::Box<tc::TaskData>)` ->
//!   [`Task::from_task_data`]; `Task (const std::string&)` ->
//!   [`Task::from_input`]; `Task (const json::object*)` ->
//!   [`Task::from_json_object`]; `parseJSON (const json::object*)` ->
//!   [`Task::parse_json_object`]; `composeJSON (false)` also as the
//!   Context-free [`Task::compose_json_plain`].
//! * The parsers take the Context because `addDependency` (circular check,
//!   footnotes) and the orphan debug message need it.
//! * `isupper`, `strtol`, `strtoul`, `strtof` follow the C locale, through
//!   libc for the numeric conversions.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::rc::Rc;

use crate::columns::column::MODIFICATION_LABEL;
use crate::context::Context;
use crate::feedback::{feedback_reserved_tags, feedback_special_tags};
use crate::shared::datetime::Datetime;
use crate::shared::format::{format, stream_double};
use crate::shared::json::{self, JType, JsonValue};
use crate::lexer::{Lexer, Type as LexerType};
use crate::shared::rx::RX;
use crate::shared::localtime;
use crate::shared::shared::{find_bytes_from, join, split, str_replace};
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
    /// C++ `Context::getContext ().tdb2.latest_id ()`; its errors propagate
    /// as the C++ throw would.
    fn latest_id(&mut self) -> Result<i32>;
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

    /// C++ `TDB2::latest_id` can throw (replica errors while loading the
    /// working set, or no replica open); the error propagates out of
    /// `hasTag ("LATEST")`.
    fn latest_id(&mut self) -> Result<i32> {
        self.tdb2.latest_id()
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

    fn latest_id(&mut self) -> Result<i32> {
        Ok(0)
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

    fn has_tag(&self, tag: &str) -> Result<bool> {
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

    fn has_tag(&self, tag: &str) -> Result<bool> {
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

/// C++ `#define APPROACHING_INFINITY 1000  // Close enough.  This isn't rocket surgery.`
const APPROACHING_INFINITY: i32 = 1000;

/// The `Task::substitute` infinite-loop guard message.
fn infinite_loop_error() -> TwError {
    TwError::Message(format(
        "Terminated substitution because more than {1} changes were made - infinite loop protection.",
        &[&APPROACHING_INFINITY],
    ))
}

/// C++ `std::string::replace (pos, n, str)` on bytes: `pos` (a `size_type`;
/// a negative value is the wrapped-around huge one) past `size ()` throws
/// `std::out_of_range`, which no Taskwarrior handler names (`catch (...)`,
/// [`TwError::Runtime`] carrying the libstdc++ `what ()`); `n` is clamped to
/// the remaining length.
fn cpp_string_replace(s: &mut Vec<u8>, pos: i64, n: i64, to: &[u8]) -> Result<()> {
    let size = s.len();
    let pos = pos as u64;
    if pos > size as u64 {
        return Err(TwError::Runtime(format!(
            "basic_string::replace: __pos (which is {pos}) > this->size() (which is {size})"
        )));
    }
    let pos = pos as usize;
    let n = (n.max(0) as usize).min(size - pos);
    s.splice(pos..pos + n, to.iter().copied());
    Ok(())
}

/// A C++ `std::string` (bytes) stored back into a Rust `String`: lossless
/// unless a byte-offset replacement split a UTF-8 sequence.
fn bytes_to_string(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
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

    /// C++ `Task::attributes[name] = type` (as `Context::staticInitialization`
    /// does for every column).
    pub fn insert_attribute(name: &str, type_: &str) {
        ATTRIBUTES.with(|v| v.borrow_mut().insert(name.to_string(), type_.to_string()));
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
    ///
    /// # Errors
    /// `The status '{1}' is not valid.` (C++ `getStatus` throws).
    pub fn is_ready(&self) -> Result<bool> {
        Ok(self.get_status()? == Status::Pending
            && !self.is_blocked
            && (!self.has("scheduled") || named_date("now") > Datetime::from_epoch(self.get_date("scheduled"))))
    }

    /// Whether the task has a `due` and is neither completed nor deleted
    /// (the common prefix of the `is_due*` predicates).
    fn has_open_due(&self) -> Result<bool> {
        if self.has("due") {
            let status = self.get_status()?;
            return Ok(status != Status::Completed && status != Status::Deleted);
        }
        Ok(false)
    }

    /// C++ `bool Task::is_due () const`.
    ///
    /// # Errors
    /// `The status '{1}' is not valid.` (C++ `getStatus` throws).
    pub fn is_due(&self, env: &mut dyn TaskEnv) -> Result<bool> {
        if self.has_open_due()? {
            let state = self.get_date_state("due", env);
            if state == DateState::AfterToday || state == DateState::EarlierToday || state == DateState::LaterToday {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// C++ `bool Task::is_dueyesterday () const`.
    pub fn is_dueyesterday(&self) -> Result<bool> {
        Ok(self.has_open_due()? && named_date("yesterday").same_day(&Datetime::from_epoch(self.get_date("due"))))
    }

    /// C++ `bool Task::is_duetoday () const`.
    pub fn is_duetoday(&self, env: &mut dyn TaskEnv) -> Result<bool> {
        if self.has_open_due()? {
            let state = self.get_date_state("due", env);
            if state == DateState::EarlierToday || state == DateState::LaterToday {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// C++ `bool Task::is_duetomorrow () const`.
    pub fn is_duetomorrow(&self) -> Result<bool> {
        Ok(self.has_open_due()? && named_date("tomorrow").same_day(&Datetime::from_epoch(self.get_date("due"))))
    }

    /// `due >= Datetime (start) && due <= Datetime (end)` for an open due.
    fn due_within(&self, start: &str, end: &str) -> Result<bool> {
        if self.has_open_due()? {
            let due = Datetime::from_epoch(self.get_date("due"));
            if due >= named_date(start) && due <= named_date(end) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// C++ `bool Task::is_dueweek () const`: due between `sow` and `eow`.
    pub fn is_dueweek(&self) -> Result<bool> {
        self.due_within("sow", "eow")
    }

    /// C++ `bool Task::is_duemonth () const`: due between `som` and `eom`.
    pub fn is_duemonth(&self) -> Result<bool> {
        self.due_within("som", "eom")
    }

    /// C++ `bool Task::is_duequarter () const`: due between `soq` and `eoq`.
    pub fn is_duequarter(&self) -> Result<bool> {
        self.due_within("soq", "eoq")
    }

    /// C++ `bool Task::is_dueyear () const`: due between `soy` and `eoy`.
    pub fn is_dueyear(&self) -> Result<bool> {
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
    pub fn is_overdue(&self, env: &mut dyn TaskEnv) -> Result<bool> {
        if self.has("due") {
            let status = self.get_status()?;

            if status != Status::Completed && status != Status::Deleted && status != Status::Recurring {
                let state = self.get_date_state("due", env);
                if state == DateState::EarlierToday || state == DateState::BeforeToday {
                    return Ok(true);
                }
            }
        }

        Ok(false)
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
    pub fn has_tag(&self, tag: &str, env: &mut dyn TaskEnv) -> Result<bool> {
        // Synthetic tags - dynamically generated, but do not occupy storage space.
        // Note: This list must match that in CmdInfo::execute.
        // Note: This list must match that in ::feedback_reserved_tags.
        if tag.as_bytes().first().is_some_and(u8::is_ascii_uppercase) {
            // NOTE: This list should be kept synchronized with:
            // * the list in CmdTags.cpp for the _tags command.
            // * the list in CmdInfo.cpp for the info command.
            match tag {
                "BLOCKED" => return Ok(self.is_blocked),
                "UNBLOCKED" => return Ok(!self.is_blocked),
                "BLOCKING" => return Ok(self.is_blocking),
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
                "ACTIVE" => return Ok(self.has("start")),
                "SCHEDULED" => return Ok(self.has("scheduled")),
                "CHILD" => return Ok(self.has("parent") || self.has("template")), // 2017-01-07: Deprecated in 2.6.0
                "INSTANCE" => return Ok(self.has("template") || self.has("parent")),
                "UNTIL" => return Ok(self.has("until")),
                "ANNOTATED" => return Ok(self.has_annotations()),
                "TAGGED" => return Ok(self.get_tag_count() > 0),
                "PARENT" => return Ok(self.has("mask") || self.has("last")), // 2017-01-07: Deprecated in 2.6.0
                "TEMPLATE" => return Ok(self.has("last") || self.has("mask")),
                "WAITING" => return Ok(self.is_waiting()),
                "PENDING" => return Ok(self.get_status()? == Status::Pending),
                "COMPLETED" => return Ok(self.get_status()? == Status::Completed),
                "DELETED" => return Ok(self.get_status()? == Status::Deleted),
                "UDA" => return Ok(self.is_uda_present(env)),
                "ORPHAN" => return Ok(self.is_orphan_present(env)),
                "LATEST" => return Ok(self.id == env.latest_id()?),
                "PROJECT" => return Ok(self.has("project")),
                "PRIORITY" => return Ok(self.has("priority")),
                _ => {}
            }
        }

        // Concrete tags.
        Ok(self.has(&Task::tag2attr(tag)))
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

////////////////////////////////////////////////////////////////////////////////
// Validation and dependencies (the parts of C++ `Task` that need the Context).
//
// C++ `Task::validate` calls `context.columns["project"/"due"/"scheduled"]->validate (value)`;
// that is [`column_validate`] over `Context::columns`.

/// C++ `Context::getContext ().columns[name]->validate (value)`.
///
/// Deviation: C++ `operator[]` yields a null `Column*` for an unregistered
/// name and the call crashes; here an unregistered column validates nothing
/// (`false`). `Context::initialize` always registers the built-in columns.
fn column_validate(ctx: &Context, name: &str, value: &str) -> bool {
    ctx.columns.get(name).is_some_and(|column| column.validate(value))
}

impl Task {
    ////////////////////////////////////////////////////////////////////////////
    // Dependencies.

    /// C++ `void Task::addDependency (int depid)`.
    ///
    /// # Errors
    /// `Could not create a dependency on task {1} - not found.` when the ID is
    /// not in the working set, plus the errors of [`Task::add_dependency`].
    pub fn add_dependency_id(&mut self, ctx: &mut Context, depid: i32) -> Result<()> {
        // Check that id is resolvable.
        let uuid = ctx.tdb2.uuid(depid)?;
        if uuid.is_empty() {
            return Err(TwError::Message(format(
                "Could not create a dependency on task {1} - not found.",
                &[&depid],
            )));
        }

        // the add_dependency(&str) overload will check this, too, but here we
        // can give an more natural error message containing the id the user
        // provided.
        if self.has_dependency(&uuid) {
            ctx.footnote(&format("Task {1} already depends on task {2}.", &[&self.id, &depid]));
            return Ok(());
        }

        self.add_dependency(ctx, &uuid)
    }

    /// C++ `void Task::addDependency (const std::string& uuid)`.
    ///
    /// # Errors
    /// `A task cannot be dependent on itself.`, or `Circular dependency
    /// detected and disallowed.` (as in C++, the `dep_` attribute has already
    /// been stored when that is raised).
    pub fn add_dependency(&mut self, ctx: &mut Context, uuid: &str) -> Result<()> {
        if uuid == self.get_ref("uuid") {
            return Err(TwError::Message("A task cannot be dependent on itself.".to_string()));
        }

        if self.has_dependency(uuid) {
            ctx.footnote(&format("Task {1} already depends on task {2}.", &[&self.get("uuid"), &uuid]));
            return Ok(());
        }

        // Store the dependency.
        self.set(&Task::dep2attr(uuid), "x");

        // Prevent circular dependencies.
        if crate::dependency::dependency_is_circular(&mut ctx.tdb2, self)? {
            return Err(TwError::Message("Circular dependency detected and disallowed.".to_string()));
        }

        self.recalc_urgency = true;
        self.fix_depends_attribute();
        Ok(())
    }

    /// C++ `void Task::removeDependency (int id)`.
    ///
    /// # Errors
    /// `Could not delete a dependency on task {1} - not found.` (with the ID).
    pub fn remove_dependency_id(&mut self, ctx: &mut Context, id: i32) -> Result<()> {
        let uuid = ctx.tdb2.uuid(id)?;

        // The remove_dependency(&str) method will check this too, but here we
        // can give a more natural error message containing the id provided by the user
        if uuid.is_empty() || !self.has(&Task::dep2attr(&uuid)) {
            return Err(TwError::Message(format(
                "Could not delete a dependency on task {1} - not found.",
                &[&id],
            )));
        }
        self.remove_dependency(&uuid)
    }

    /// C++ `void Task::removeDependency (const std::string& uuid)`.
    ///
    /// # Errors
    /// `Could not delete a dependency on task {1} - not found.` (with the UUID).
    pub fn remove_dependency(&mut self, uuid: &str) -> Result<()> {
        let depattr = Task::dep2attr(uuid);
        if self.has(&depattr) {
            self.remove(&depattr);
        } else {
            return Err(TwError::Message(format(
                "Could not delete a dependency on task {1} - not found.",
                &[&uuid],
            )));
        }

        self.recalc_urgency = true;
        self.fix_depends_attribute();
        Ok(())
    }

    /// C++ `std::vector<int> Task::getDependencyIDs () const`: the working-set
    /// ID (0 when not in the working set) of each dependency, in attribute
    /// order.
    pub fn get_dependency_ids(&self, tdb2: &mut crate::tdb2::Tdb2) -> Result<Vec<i32>> {
        let mut ids = Vec::new();
        for attr in self.data.keys() {
            if !Task::is_dep_attr(attr) {
                continue;
            }
            let dep = Task::attr2dep(attr);
            ids.push(tdb2.id(&dep)?);
        }

        Ok(ids)
    }

    /// C++ `std::vector<Task> Task::getDependencyTasks () const`: the pending
    /// tasks (neither completed nor deleted) this task depends on, in
    /// pending-task order.
    ///
    /// # Errors
    /// Replica errors, or an invalid status on a pending task (C++
    /// `getStatus` throws).
    pub fn get_dependency_tasks(&self, tdb2: &mut crate::tdb2::Tdb2) -> Result<Vec<Task>> {
        let uuids = self.get_dependency_uuids();

        // NOTE: this may seem inefficient, but note that `TDB2::get` performs a
        // linear search on each invocation, so scanning *once* is quite a bit more
        // efficient.
        let mut blocking = Vec::new();
        if !uuids.is_empty() {
            for it in tdb2.pending_tasks()? {
                let status = it.get_status()?;
                if status != Status::Completed
                    && status != Status::Deleted
                    && uuids.iter().any(|u| u == it.get_ref("uuid"))
                {
                    blocking.push(it.clone());
                }
            }
        }

        Ok(blocking)
    }

    /// C++ `std::vector<Task> Task::getBlockedTasks () const`: the pending
    /// tasks (neither completed nor deleted) that depend on this task.
    ///
    /// # Errors
    /// As [`Task::get_dependency_tasks`].
    pub fn get_blocked_tasks(&self, tdb2: &mut crate::tdb2::Tdb2) -> Result<Vec<Task>> {
        let uuid = self.get("uuid");

        let mut blocked = Vec::new();
        for it in tdb2.pending_tasks()? {
            let status = it.get_status()?;
            if status != Status::Completed && status != Status::Deleted && it.has_dependency(&uuid) {
                blocked.push(it.clone());
            }
        }

        Ok(blocked)
    }

    ////////////////////////////////////////////////////////////////////////////
    // Validation.

    /// C++ `void Task::validate_add ()`: validate a task for addition, raising
    /// user-visible errors for inconsistent or incorrect inputs. This is
    /// called before [`Task::validate`]. C++ reads nothing from the Context
    /// here; `ctx` is taken for symmetry with `validate` (M4 API contract).
    ///
    /// # Errors
    /// `A task must have a description.`, `Cannot add a task that is blank.`,
    /// `A recurring task must also have a 'due' date.`
    pub fn validate_add(&self, _ctx: &mut Context) -> Result<()> {
        // There is no fixing a missing description.
        if !self.has("description") {
            return Err(TwError::Message("A task must have a description.".to_string()));
        } else if self.get_ref("description").is_empty() {
            return Err(TwError::Message("Cannot add a task that is blank.".to_string()));
        }

        // Cannot have an old-style recur frequency with no due date - when would it recur?
        if self.has("recur") && (!self.has("due") || self.get_ref("due").is_empty()) {
            return Err(TwError::Message("A recurring task must also have a 'due' date.".to_string()));
        }

        Ok(())
    }

    /// C++ `void Task::validate (bool applyDefault = true)`.
    ///
    /// The purpose of Task::validate is three-fold:
    ///   1) To provide missing attributes where possible
    ///   2) To provide suitable warnings about odd states
    ///   3) To update status depending on other attributes
    ///
    /// As required by TaskChampion, no combination of properties and values is an
    /// error. This function will try to make sensible defaults and resolve inconsistencies.
    /// Critically, note that despite the name this is not a read-only function.
    ///
    /// # Errors
    /// `The status '{1}' is not valid.`, `Not a valid UUID '{1}'.`, and the
    /// `Datetime` parse error for an unparseable `default.due` /
    /// `default.scheduled` (C++ throws from the `Datetime` constructor).
    pub fn validate(&mut self, ctx: &mut Context, apply_default: bool) -> Result<()> {
        let mut status = Status::Pending;
        if !self.get_ref("status").is_empty() {
            status = self.get_status()?;
        }

        // 1) Provide missing attributes where possible
        // Provide a UUID if necessary. Validate if present.
        let uid = self.get("uuid");
        if self.has("uuid") && !uid.is_empty() {
            let mut lex = crate::lexer::Lexer::new(&uid);
            let mut token = String::new();
            let mut type_ = crate::lexer::Type::uuid;
            // `uuid` is not a property in the TaskChampion model, so an invalid UUID is
            // actually an error.
            if !lex.is_uuid(&mut token, &mut type_, true) {
                return Err(TwError::Message(format("Not a valid UUID '{1}'.", &[&uid])));
            }
        } else {
            self.set("uuid", &crate::util::uuid());
        }

        // Obsolete (C++ "remove for 3.0.0"), kept as in 3.5.0:
        // Recurring tasks get a special status.
        if status == Status::Pending
            && self.has("due")
            && self.has("recur")
            && (!self.has("parent") || self.get_ref("parent").is_empty())
            && (!self.has("template") || self.get_ref("template").is_empty())
        {
            status = Status::Recurring;
        }
        // (C++ keeps, commented out, the planned 3.0.0 form of this test, which
        // drops the `parent` condition.)

        // Tasks with a wait: date get a special status.
        else if status == Status::Pending && self.has("wait") && !self.get_ref("wait").is_empty() {
            status = Status::Waiting;
        }
        // By default, tasks are pending.
        else if !self.has("status") || self.get_ref("status").is_empty() {
            status = Status::Pending;
        }

        // Default to 'periodic' type recurrence.
        if status == Status::Recurring && (!self.has("rtype") || self.get_ref("rtype").is_empty()) {
            self.set("rtype", "periodic");
        }

        // Store the derived status.
        self.set_status(status);

        // Provide an entry date unless user already specified one.
        if !self.has("entry") || self.get_ref("entry").is_empty() {
            self.set_as_now("entry");
        }

        // Completed tasks need an end date, so inherit the entry date.
        if (status == Status::Completed || status == Status::Deleted)
            && (!self.has("end") || self.get_ref("end").is_empty())
        {
            self.set_as_now("end");
        }

        // Pending tasks cannot have an end date, remove if present
        if status == Status::Pending && !self.get_ref("end").is_empty() {
            self.remove("end");
        }

        // Provide a modified date unless user already specified one.
        if !self.has("modified") || self.get_ref("modified").is_empty() {
            self.set_as_now("modified");
        }

        if apply_default && (!self.has("parent") || self.get_ref("parent").is_empty()) {
            // Override with default.project, if not specified.
            let default_project = Task::default_project();
            if !default_project.is_empty() && !self.has("project") && column_validate(ctx, "project", &default_project) {
                self.set("project", &default_project);
            }

            // Override with default.due, if not specified.
            let default_due = Task::default_due();
            if !default_due.is_empty() && !self.has("due") && column_validate(ctx, "due", &default_due) {
                let dur = crate::shared::duration::Duration::from_string(&default_due);
                if dur.to_time_t() != 0 {
                    self.set_int("due", (Datetime::new() + dur.to_time_t()).to_epoch());
                } else {
                    self.set_int("due", Datetime::from_string(&default_due, "")?.to_epoch());
                }
            }

            // Override with default.scheduled, if not specified.
            let default_scheduled = Task::default_scheduled();
            if !default_scheduled.is_empty() && !self.has("scheduled") && column_validate(ctx, "scheduled", &default_scheduled) {
                let dur = crate::shared::duration::Duration::from_string(&default_scheduled);
                if dur.to_time_t() != 0 {
                    self.set_int("scheduled", (Datetime::new() + dur.to_time_t()).to_epoch());
                } else {
                    self.set_int("scheduled", Datetime::from_string(&default_scheduled, "")?.to_epoch());
                }
            }

            // If a UDA has a default value in the configuration,
            // override with uda.(uda).default, if not specified.
            // Gather a list of all UDAs with a .default value
            let mut udas: Vec<String> = Vec::new();
            for key in ctx.config.keys() {
                if key.starts_with("uda.") && key.contains(".default") {
                    if let Some(period) = key[4..].find('.') {
                        udas.push(key[4..4 + period].to_string());
                    }
                }
            }

            if !udas.is_empty() {
                // For each of those, setup the default value on the task now,
                // of course only if we don't have one on the command line already
                for uda in &udas {
                    let def_val = ctx.config.get(&format!("uda.{uda}.default"));

                    // If the default is empty, or we already have a value, skip it
                    if !def_val.is_empty() && self.get_ref(uda).is_empty() {
                        self.set(uda, &def_val);
                    }
                }
            }
        }

        // 2) To provide suitable warnings about odd states

        // Date relationships.
        self.validate_before(ctx, "wait", "due");
        self.validate_before(ctx, "entry", "start");
        self.validate_before(ctx, "entry", "end");
        self.validate_before(ctx, "wait", "scheduled");
        self.validate_before(ctx, "scheduled", "start");
        self.validate_before(ctx, "scheduled", "due");
        self.validate_before(ctx, "scheduled", "end");

        if !self.has("description") || self.get_ref("description").is_empty() {
            ctx.footnote(&format("Warning: task has no description.", &[]));
        }

        // Cannot have an old-style recur frequency with no due date - when would it recur?
        if self.has("recur") && (!self.has("due") || self.get_ref("due").is_empty()) {
            ctx.footnote(&format("Warning: recurring task has no due date.", &[]));
            self.remove("recur");
        }

        // Old-style recur durations must be valid.
        if self.has("recur") {
            let value = self.get("recur");
            if !value.is_empty() {
                let mut p = crate::shared::duration::Duration::new();
                let mut i = 0usize;
                if !p.parse(&value, &mut i) {
                    // (C++: the ideal location to map unsupported old recurrence
                    // periods to supported values.)
                    ctx.footnote(&format("Warning: The recurrence value '{1}' is not valid.", &[&value]));
                    self.remove("recur");
                }
            }
        }

        Ok(())
    }

    /// C++ `void Task::validate_before (const std::string& left, const std::string& right)`:
    /// footnote when the `left` date is after a non-zero `right` date.
    fn validate_before(&self, ctx: &mut Context, left: &str, right: &str) {
        if self.has(left) && self.has(right) {
            let date_left = Datetime::from_epoch(self.get_date(left));
            let date_right = Datetime::from_epoch(self.get_date(right));

            // if date is zero, then it is being removed (e.g. "due: wait:1day")
            if date_left > date_right && date_right.to_epoch() != 0 {
                ctx.footnote(&format(
                    "Warning: You have specified that the '{1}' date is after the '{2}' date.",
                    &[&left, &right],
                ));
            }
        }
    }
}

/// C++ `static const float epsilon = 0.000001;` (a double literal narrowed
/// to `float`).
const EPSILON: f32 = 0.000001_f64 as f32;

/// C++ `std::max (a, b)` for floats: `(a < b) ? b : a` (keeps `a` when the
/// comparison is false, e.g. with NaN), unlike [`f32::max`].
fn cpp_max(a: f32, b: f32) -> f32 {
    if a < b {
        b
    } else {
        a
    }
}

/// C++ `s.substr (pos, len)` for the urgency key parsing: `len` is a
/// `size_t` that may have wrapped (e.g. `end - 21` when `end < 21`), so it is
/// clamped to the rest of the string. Callers guarantee `pos <= s.len ()`
/// and that `pos` / `pos + len` lie on character boundaries (they follow
/// ASCII prefixes or the position of an ASCII `.`).
fn cpp_substr(s: &str, pos: usize, len: usize) -> &str {
    let rest = &s[pos..];
    if len >= rest.len() {
        rest
    } else {
        &rest[..len]
    }
}

/// C++ `s.substr (pos, lit.size ()) == lit` for the urgency key prefixes,
/// compared bytewise (as C++ does), so a multi-byte character straddling
/// `pos + lit.len ()` cannot cause a slicing panic. Requires `pos <= s.len ()`.
fn cpp_substr_is(s: &str, pos: usize, lit: &str) -> bool {
    s.as_bytes()[pos..].starts_with(lit.as_bytes())
}

/// C++ `value += cond ? (float_term * float_coefficient) : 0.0;`: the
/// conditional has type `double` (because of the `0.0` literal), so the sum
/// is formed in double and narrowed back to `float`.
fn add_term(value: f32, term: impl FnOnce() -> f32, coefficient: f32) -> f32 {
    let addend: f64 = if coefficient.abs() > EPSILON { (term() * coefficient) as f64 } else { 0.0 };
    (value as f64 + addend) as f32
}

impl Task {
    ////////////////////////////////////////////////////////////////////////////
    // Urgency.

    /// C++ `float Task::urgency_c () const`.
    ///
    /// Urgency is defined as a polynomial, the value of which is calculated in
    /// this function, according to:
    ///
    /// ```text
    ///   U = A.t  + B.t  + C.t  ...
    ///          a      b      c
    ///
    ///   U       = urgency
    ///   A       = coefficient for term a
    ///   t sub a = numeric scale from 0 -> 1, with 1 being the highest
    ///             urgency, derived from one task attribute and mapped
    ///             to the numeric scale
    /// ```
    ///
    /// See rfc31-urgency.txt for full details. The arithmetic mirrors the C++
    /// `float`/`double` promotions exactly, so results are bit-identical.
    ///
    /// `ctx` supplies `rc.urgency.inherit`, the environment for `hasTag`
    /// (virtual tags in `urgency.user.tag.<TAG>.coefficient`) and the
    /// replica for [`Task::urgency_inherit`].
    ///
    /// # Errors
    /// Only from [`Task::urgency_inherit`] (replica access, or an invalid
    /// status on a pending task where C++ `getStatus` throws).
    pub fn urgency_c(&self, ctx: &mut Context) -> Result<f32> {
        let mut value: f32 = 0.0;
        value = add_term(value, || self.urgency_project(), Task::urgency_project_coefficient());
        value = add_term(value, || self.urgency_active(), Task::urgency_active_coefficient());
        value = add_term(value, || self.urgency_scheduled(), Task::urgency_scheduled_coefficient());
        value = add_term(value, || self.urgency_waiting(), Task::urgency_waiting_coefficient());
        value = add_term(value, || self.urgency_blocked(), Task::urgency_blocked_coefficient());
        value = add_term(value, || self.urgency_annotations(), Task::urgency_annotations_coefficient());
        value = add_term(value, || self.urgency_tags(), Task::urgency_tags_coefficient());
        value = add_term(value, || self.urgency_due(), Task::urgency_due_coefficient());
        value = add_term(value, || self.urgency_blocking(), Task::urgency_blocking_coefficient());
        value = add_term(value, || self.urgency_age(), Task::urgency_age_coefficient());

        value = self.urgency_user_terms(value, ctx)?;

        if self.is_blocking && ctx.config.get_boolean("urgency.inherit") {
            let prev = value;
            value = cpp_max(value, self.urgency_inherit(ctx)?);

            // This is a hackish way of making sure parent tasks are sorted above
            // child tasks.  For reports that hide blocked tasks, this is not needed.
            if prev <= value {
                value = (value as f64 + 0.01) as f32;
            }
        }

        Ok(value)
    }

    /// The tag-, project-, keyword- and UDA-specific coefficients of
    /// `urgency_c`, added to `value` in `Task::coefficients` (sorted key)
    /// order:
    /// * `urgency.user.project.<project>.coefficient`: the project or any of
    ///   its sub-projects (`<project>.`-prefixed);
    /// * `urgency.user.tag.<tag>.coefficient`: `hasTag (<tag>)`, including
    ///   virtual tags;
    /// * `urgency.user.keyword.<keyword>.coefficient`: the description
    ///   contains `<keyword>`;
    /// * `urgency.uda.<name>.coefficient`: the attribute is present;
    /// * `urgency.uda.<name>.<value>.coefficient`: the attribute equals
    ///   `<value>`.
    ///
    /// `.coefficient` is found anywhere in the key (C++ `find`), and the
    /// `substr` lengths wrap exactly as the C++ `size_t` arithmetic does.
    fn urgency_user_terms(&self, mut value: f32, env: &mut dyn TaskEnv) -> Result<f32> {
        let task_project_name = self.get("project");
        // Tag- and project-specific coefficients.
        Task::with_coefficients(|coefficients| -> Result<()> {
            for (key, &coefficient) in coefficients {
                // C++ `if (fabs (var.second) > epsilon)`: a NaN coefficient
                // (`strtod ("nan")`) is not greater, so it is skipped too.
                if coefficient.is_nan() || coefficient.abs() <= EPSILON {
                    continue;
                }

                if key.starts_with("urgency.user.") {
                    // urgency.user.project.<project>.coefficient
                    if cpp_substr_is(key, 13, "project.") {
                        if let Some(end) = key.find(".coefficient") {
                            let project = cpp_substr(key, 21, end.wrapping_sub(21));

                            if task_project_name == project
                                || task_project_name.starts_with(&format!("{project}."))
                            {
                                value += coefficient;
                            }
                        }
                    }

                    // urgency.user.tag.<tag>.coefficient
                    if cpp_substr_is(key, 13, "tag.") {
                        if let Some(end) = key.find(".coefficient") {
                            let tag = cpp_substr(key, 17, end.wrapping_sub(17));

                            if self.has_tag(tag, env)? {
                                value += coefficient;
                            }
                        }
                    }

                    // urgency.user.keyword.<keyword>.coefficient
                    if cpp_substr_is(key, 13, "keyword.") {
                        if let Some(end) = key.find(".coefficient") {
                            let keyword = cpp_substr(key, 21, end.wrapping_sub(21));

                            if self.get_ref("description").contains(keyword) {
                                value += coefficient;
                            }
                        }
                    }
                } else if key.starts_with("urgency.uda.") {
                    // urgency.uda.<name>.coefficient
                    // urgency.uda.<name>.<value>.coefficient
                    if let Some(end) = key.find(".coefficient") {
                        let uda = cpp_substr(key, 12, end.wrapping_sub(12));
                        match uda.find('.') {
                            None => {
                                // urgency.uda.<name>.coefficient
                                if self.has(uda) {
                                    value += coefficient;
                                }
                            }
                            Some(dot) => {
                                // urgency.uda.<name>.<value>.coefficient
                                let (name, wanted) = (&uda[..dot], &uda[dot + 1..]);
                                if self.get_ref(name) == wanted {
                                    value += coefficient;
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        })?;

        Ok(value)
    }

    /// C++ `float Task::urgency ()`: the cached [`Task::urgency_c`], recomputed
    /// only when `recalc_urgency` is set.
    ///
    /// Returns `Result` (unlike the C++ `float`) because `urgency.inherit`
    /// reads the replica through `ctx.tdb2`.
    ///
    /// # Errors
    /// As [`Task::urgency_c`]; the cache is left untouched on error.
    pub fn urgency(&mut self, ctx: &mut Context) -> Result<f32> {
        if self.recalc_urgency {
            self.urgency_value = self.urgency_c(ctx)?;

            // Return the sum of all terms.
            self.recalc_urgency = false;
        }

        Ok(self.urgency_value)
    }

    /// C++ `float Task::urgency_inherit () const`: the highest urgency among
    /// the pending tasks this task blocks (`getBlockedTasks`), `-FLT_MAX`
    /// when there are none. Each blocked task is a copy whose urgency is
    /// computed afresh, recursively inheriting along the chain.
    ///
    /// # Errors
    /// As [`Task::get_blocked_tasks`].
    pub fn urgency_inherit(&self, ctx: &mut Context) -> Result<f32> {
        let mut v = f32::MIN; // -FLT_MAX
        // Calling getBlockedTasks is rather expensive.
        // It is called recursively for each dependency in the chain here.
        for mut task in self.get_blocked_tasks(&mut ctx.tdb2)? {
            // Find highest urgency in all blocked tasks.
            v = cpp_max(v, task.urgency(ctx)?);
        }

        Ok(v)
    }

    /// C++ `float Task::urgency_project () const`.
    pub fn urgency_project(&self) -> f32 {
        if self.has("project") {
            return 1.0;
        }

        0.0
    }

    /// C++ `float Task::urgency_active () const`.
    pub fn urgency_active(&self) -> f32 {
        if self.has("start") {
            return 1.0;
        }

        0.0
    }

    /// C++ `float Task::urgency_scheduled () const`: scheduled in the past.
    pub fn urgency_scheduled(&self) -> f32 {
        self.urgency_scheduled_at(localtime::time_now())
    }

    /// [`Task::urgency_scheduled`] with `time (nullptr)` supplied.
    fn urgency_scheduled_at(&self, now: i64) -> f32 {
        if self.has("scheduled") && self.get_date("scheduled") < now {
            return 1.0;
        }

        0.0
    }

    /// C++ `float Task::urgency_waiting () const`.
    pub fn urgency_waiting(&self) -> f32 {
        if self.is_waiting() {
            return 1.0;
        }

        0.0
    }

    /// C++ `float Task::urgency_blocked () const`: a task is blocked only if
    /// the task it depends upon is pending/waiting.
    pub fn urgency_blocked(&self) -> f32 {
        if self.is_blocked {
            return 1.0;
        }

        0.0
    }

    /// C++ `float Task::urgency_annotations () const`.
    pub fn urgency_annotations(&self) -> f32 {
        if self.annotation_count >= 3 {
            1.0
        } else if self.annotation_count == 2 {
            0.9_f64 as f32
        } else if self.annotation_count == 1 {
            0.8_f64 as f32
        } else {
            0.0
        }
    }

    /// C++ `float Task::urgency_tags () const`.
    pub fn urgency_tags(&self) -> f32 {
        match self.get_tag_count() {
            0 => 0.0,
            1 => 0.8_f64 as f32,
            2 => 0.9_f64 as f32,
            _ => 1.0,
        }
    }

    /// C++ `float Task::urgency_due () const`.
    ///
    /// ```text
    ///     Past                  Present                              Future
    ///     Overdue               Due                                     Due
    ///
    ///     -7 -6 -5 -4 -3 -2 -1  0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 days
    ///
    /// <-- 1.0                         linear                            0.2 -->
    ///     capped                                                        capped
    /// ```
    pub fn urgency_due(&self) -> f32 {
        self.urgency_due_at(&Datetime::new())
    }

    /// [`Task::urgency_due`] with `Datetime now` supplied.
    fn urgency_due_at(&self, now: &Datetime) -> f32 {
        if self.has("due") {
            let due = Datetime::from_epoch(self.get_date("due"));

            // Map a range of 21 days to the value 0.2 - 1.0
            // C++: float days_overdue = (now - due) / 86400.0;  (time_t / double)
            let days_overdue: f32 = ((now - &due) as f64 / 86400.0) as f32;
            return if days_overdue as f64 >= 7.0 {
                1.0 // < 1 wk ago
            } else if days_overdue as f64 >= -14.0 {
                (((days_overdue as f64 + 14.0) * 0.8 / 21.0) + 0.2) as f32
            } else {
                0.2_f64 as f32 // > 2 wks
            };
        }

        0.0
    }

    /// C++ `float Task::urgency_age () const`.
    pub fn urgency_age(&self) -> f32 {
        self.urgency_age_at(&Datetime::new())
    }

    /// [`Task::urgency_age`] with `Datetime now` supplied.
    fn urgency_age_at(&self, now: &Datetime) -> f32 {
        if !self.has("entry") {
            return 1.0;
        }

        let entry = Datetime::from_epoch(self.get_date("entry"));
        // C++: int age = (now - entry) / 86400;  (time_t division, in days)
        let age = ((now - &entry) / 86400) as i32;

        let age_max = Task::urgency_age_max();
        // `age > urgencyAgeMax` compares int and float in float.
        if age_max == 0.0 || age as f32 > age_max {
            return 1.0;
        }

        // C++: return (1.0 * age / Task::urgencyAgeMax);  (double, narrowed)
        (1.0 * age as f64 / age_max as f64) as f32
    }

    /// C++ `float Task::urgency_blocking () const`.
    pub fn urgency_blocking(&self) -> f32 {
        if self.is_blocking {
            return 1.0;
        }

        0.0
    }

    ////////////////////////////////////////////////////////////////////////////
    // Command-line modification (C++ `#ifdef PRODUCT_TASKWARRIOR` section).

    /// C++ `void Task::modify (modType type, bool text_required = false)`:
    /// reads the parse tree (`ctx.cli2.args`) and applies every argument
    /// tagged `MODIFICATION` to this task, in order: `name:value` pairs (an
    /// empty value removes the attribute, otherwise the column's `modify`),
    /// `/from/to/flags` substitutions, `+tag` / `-tag`, and every other
    /// argument accumulated as description text, which `mod_type` then
    /// applies (replace, prepend, append or annotate).
    ///
    /// C++ `Context::getContext ().withCurrentTask (this)` makes this task
    /// the DOM context while the parse tree is read. `current_task` is an
    /// `Rc` snapshot rather than a live pointer, so it is refreshed from
    /// `self` before every column `modify` (the only DOM consumer here),
    /// which is when C++ would read through the pointer; the previous value
    /// is restored on every exit path, as the C++ RAII guard does.
    ///
    /// The C++ `Context::getContext ().columns[name]` lookup is
    /// `std::map::operator[]`, which inserts a null entry for an unknown
    /// name before the throw; that entry is unobservable (the command fails
    /// with the error below) and is not reproduced.
    ///
    /// # Errors
    /// * `The '{1}' attribute does not allow a value of '{2}'.` for an
    ///   unknown or non-modifiable column.
    /// * `Unrecognized column type '{1}' for column '{2}'`.
    /// * `Additional text must be provided.` when `text_required` and no
    ///   modification was made.
    /// * Whatever the column `modify`, `feedback_reserved_tags`,
    ///   `removeDependency`, [`Task::substitute`] or `getStatus` throw.
    pub fn modify(&mut self, ctx: &mut Context, mod_type: ModType, text_required: bool) -> Result<()> {
        // while reading the parse tree, consider DOM references in the context of
        // this task
        let previous = ctx.current_task.replace(Rc::new(self.clone()));
        let result = self.modify_in_context(ctx, mod_type, text_required);
        ctx.current_task = previous;
        result
    }

    /// The body of [`Task::modify`], run with `ctx.current_task` bound.
    fn modify_in_context(&mut self, ctx: &mut Context, mod_type: ModType, text_required: bool) -> Result<()> {
        let label = MODIFICATION_LABEL;

        // Need this for later comparison.
        let original_status = self.get_status()?;

        let mut text = String::new();
        let mut mods = false;
        let args = ctx.cli2.args.clone();
        for a in &args {
            if a.has_tag("MODIFICATION") {
                if a.lextype == LexerType::pair {
                    // 'canonical' is the canonical name. Needs to be said.
                    // 'value' requires eval.
                    let name = a.attribute("canonical");
                    let mut value = a.attribute("value");
                    if value.is_empty() || value == "''" || value == "\"\"" {
                        // Special case: Handle bulk removal of 'tags' and 'depends" virtual
                        // attributes
                        if name == "depends" {
                            for dep in self.get_dependency_uuids() {
                                self.remove_dependency(&dep)?;
                            }
                        } else if name == "tags" {
                            for tag in self.get_tags() {
                                self.remove_tag(&tag);
                            }
                        }

                        // ::composeF4 will skip if the value is blank, but the presence of
                        // the attribute will prevent ::validate from applying defaults.
                        if (self.has(&name) && !self.get(&name).is_empty())
                            || (name == "due" && ctx.config.has("default.due"))
                            || (name == "scheduled" && ctx.config.has("default.scheduled"))
                            || (name == "project" && ctx.config.has("default.project"))
                        {
                            mods = true;
                            self.set(&name, "");
                        }

                        ctx.debug(&format!("{label}{name} <-- ''"));
                    } else {
                        Lexer::dequote(&mut value);

                        // Get the column info. Some columns are not modifiable.
                        let column = match ctx.columns.get(&name).cloned() {
                            Some(column) if column.modifiable() => column,
                            _ => {
                                return Err(TwError::Message(format(
                                    "The '{1}' attribute does not allow a value of '{2}'.",
                                    &[&name, &value],
                                )))
                            }
                        };

                        // Delegate modification to the column object or their base classes.
                        let column_type = column.type_().to_string();
                        if name == "depends"
                            || name == "tags"
                            || name == "recur"
                            || column_type == "date"
                            || column_type == "duration"
                            || column_type == "numeric"
                            || column_type == "string"
                            || column_type == "uuid"
                        {
                            // The live `this` C++ `currentTask` points at.
                            ctx.current_task = Some(Rc::new(self.clone()));
                            column.modify(self, &value, ctx)?;
                            mods = true;
                        } else {
                            return Err(TwError::Message(format(
                                "Unrecognized column type '{1}' for column '{2}'",
                                &[&column_type, &name],
                            )));
                        }
                    }
                }
                // Perform description/annotation substitution.
                else if a.lextype == LexerType::substitution {
                    ctx.debug(&format!("{label}substitute {}", a.attribute("raw")));
                    self.substitute(&a.attribute("from"), &a.attribute("to"), &a.attribute("flags"))?;
                    mods = true;
                }
                // Tags need special handling because they are essentially a vector stored
                // in a single string, therefore Task::{add,remove}Tag must be called as
                // appropriate.
                else if a.lextype == LexerType::tag {
                    let tag = a.attribute("name");
                    feedback_reserved_tags(&tag)?;

                    if a.attribute("sign") == "+" {
                        ctx.debug(&format!("{label}tags <-- add '{tag}'"));
                        self.add_tag(&tag);
                        feedback_special_tags(ctx, self, &tag);
                    } else {
                        ctx.debug(&format!("{label}tags <-- remove '{tag}'"));
                        self.remove_tag(&tag);
                    }

                    mods = true;
                }
                // Unknown args are accumulated as though they were WORDs.
                else {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&a.attribute("raw"));
                }
            }
        }

        // Task::modType determines what happens to the WORD arguments, if there are
        //  any.
        if !text.is_empty() {
            Lexer::dequote(&mut text);

            match mod_type {
                ModType::Replace => {
                    ctx.debug(&format!("{label}description <-- '{text}'"));
                    self.set("description", &text);
                }
                ModType::Prepend => {
                    ctx.debug(&format!("{label}description <-- '{text}' + description"));
                    let description = format!("{text} {}", self.get("description"));
                    self.set("description", &description);
                }
                ModType::Append => {
                    ctx.debug(&format!("{label}description <-- description + '{text}'"));
                    let description = format!("{} {text}", self.get("description"));
                    self.set("description", &description);
                }
                ModType::Annotate => {
                    ctx.debug(&format!("{label}new annotation <-- '{text}'"));
                    self.add_annotation(&text);
                }
            }
        } else if !mods && text_required {
            return Err(TwError::Message("Additional text must be provided.".into()));
        }

        // Modifying completed/deleted tasks generates a message, if the modification
        // does not change status.
        let status = self.get_status()?;
        if (status == Status::Completed || status == Status::Deleted) && status == original_status {
            let uuid = substr_prefix(&self.get("uuid"), 8);
            ctx.footnote(&format(
                "Note: Modified task {1} is {2}. You may wish to make this task pending with: task {3} modify status:pending",
                &[&uuid, &self.get("status"), &uuid],
            ));
        }

        Ok(())
    }

    /// C++ `void Task::substitute (const std::string& from, const std::string&
    /// to, const std::string& flags)`: applies a `/from/to/flags`
    /// substitution to the description and then (unless a non-global
    /// substitution already happened) the annotations. With `rc.regex`
    /// (`Task::regex`) `from` is a regex (`RX (from,
    /// Task::searchCaseSensitive)`), otherwise a literal searched with
    /// `::find (..., Task::searchCaseSensitive)`. Flag `g` replaces every
    /// match; otherwise only the first one, in the description or else in
    /// the first annotation that matches.
    ///
    /// The C++ offset arithmetic is reproduced byte for byte, including its
    /// quirks: the literal branch searches the already-modified text but
    /// replaces at `pos + skew`, and the regex annotation branch indexes
    /// `start[i + skew]`. Every `std::string::replace` goes through
    /// [`cpp_string_replace`], so a position past the end throws
    /// `std::out_of_range` exactly where C++ does (reaching `catch (...)`,
    /// [`TwError::Runtime`]). A `start[i + skew]` index outside the vector
    /// is undefined behaviour in C++ (no bounds check); here it is reported
    /// as [`TwError::Runtime`] too.
    ///
    /// The text is edited as bytes (C++ `std::string`); if a replacement at a
    /// byte offset splits a UTF-8 sequence the stored value is converted
    /// lossily.
    ///
    /// # Errors
    /// * `Terminated substitution because more than {1} changes were made -
    ///   infinite loop protection.` (literal branch, more than
    ///   `APPROACHING_INFINITY` = 1000 changes).
    /// * [`TwError::Runtime`] for an invalid regex (`RX` rethrowing
    ///   `std::regex_error`), or an out-of-range replace as described above.
    pub fn substitute(&mut self, from: &str, to: &str, flags: &str) -> Result<()> {
        let global = flags.contains('g');
        let sensitive = Task::search_case_sensitive();
        let to_b = to.as_bytes();
        let from_b = from.as_bytes();

        // Get the data to modify.
        let mut description = self.get("description").into_bytes();
        let annotations = self.get_annotations();
        let mut anno_bytes: Vec<(String, Vec<u8>)> =
            annotations.into_iter().map(|(k, v)| (k, v.into_bytes())).collect();

        // Count the changes, so we know whether to proceed to annotations, after
        // modifying description.
        let mut changes = 0;
        let mut done = false;

        // Regex support is optional.
        if Task::regex() {
            // Create the regex.
            let rx = RX::new(from, sensitive);
            let mut start: Vec<i32> = Vec::new();
            let mut end: Vec<i32> = Vec::new();

            // Perform all subs on description.
            let subject = String::from_utf8_lossy(&description).into_owned();
            if rx.matches_offsets(&mut start, &mut end, &subject)? {
                let mut skew: i64 = 0;
                let mut i = 0;
                while i < start.len() && !done {
                    let n = (end[i] - start[i]) as i64;
                    cpp_string_replace(&mut description, start[i] as i64 + skew, n, to_b)?;
                    skew += to_b.len() as i64 - n;
                    changes += 1;

                    if !global {
                        done = true;
                    }
                    i += 1;
                }
            }

            if !done {
                // Perform all subs on annotations.
                for (_, value) in anno_bytes.iter_mut() {
                    start.clear();
                    end.clear();
                    let subject = String::from_utf8_lossy(value).into_owned();
                    if rx.matches_offsets(&mut start, &mut end, &subject)? {
                        let mut skew: i64 = 0;
                        let mut i = 0;
                        while i < start.len() && !done {
                            // C++ `start[i + skew]`: unsigned int index.
                            let index = (i as i64 + skew) as u32 as usize;
                            let Some(&at) = start.get(index) else {
                                return Err(TwError::Runtime(format!(
                                    "Task::substitute: start[{index}] out of range (size {})",
                                    start.len()
                                )));
                            };
                            let n = (end[i] - start[i]) as i64;
                            cpp_string_replace(value, at as i64, n, to_b)?;
                            skew += to_b.len() as i64 - n;
                            changes += 1;

                            if !global {
                                done = true;
                            }
                            i += 1;
                        }
                    }
                }
            }
        } else {
            // Perform all subs on description.
            let mut counter = 0;
            let mut pos: usize = 0;
            let mut skew: i64 = 0;

            while !done {
                let Some(found) = find_bytes_from(&description, from_b, pos, sensitive) else {
                    break;
                };
                pos = found;
                cpp_string_replace(&mut description, pos as i64 + skew, from_b.len() as i64, to_b)?;
                skew += to_b.len() as i64 - from_b.len() as i64;

                pos += to_b.len();
                changes += 1;

                if !global {
                    done = true;
                }

                counter += 1;
                if counter > APPROACHING_INFINITY {
                    return Err(infinite_loop_error());
                }
            }

            if !done {
                // Perform all subs on annotations.
                counter = 0;
                for (_, value) in anno_bytes.iter_mut() {
                    pos = 0;
                    skew = 0;
                    while !done {
                        let Some(found) = find_bytes_from(value, from_b, pos, sensitive) else {
                            break;
                        };
                        pos = found;
                        cpp_string_replace(value, pos as i64 + skew, from_b.len() as i64, to_b)?;
                        skew += to_b.len() as i64 - from_b.len() as i64;

                        pos += to_b.len();
                        changes += 1;

                        if !global {
                            done = true;
                        }

                        counter += 1;
                        if counter > APPROACHING_INFINITY {
                            return Err(infinite_loop_error());
                        }
                    }
                }
            }
        }

        if changes != 0 {
            self.set("description", &bytes_to_string(description));
            let annotations: BTreeMap<String, String> =
                anno_bytes.into_iter().map(|(k, v)| (k, bytes_to_string(v))).collect();
            self.set_annotations(&annotations);
            self.recalc_urgency = true;
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    // Later-milestone stubs.

    /// C++ `std::string Task::diff (const Task& after) const`: summarizes the
    /// differences to `after` for display, in the future tense.
    ///
    /// Restored by milestone 7. Nothing calls it yet.
    ///
    /// # Errors
    /// Always `TwError::NotImplemented("Task::diff")` until milestone 7.
    pub fn diff(&self, _after: &Task) -> Result<String> {
        Err(TwError::NotImplemented("Task::diff".into()))
    }
}


////////////////////////////////////////////////////////////////////////////////
// Parsing (JSON, legacy) and JSON composition.

/// C++ `std::string::operator[]` on bytes for the legacy format probes: an
/// index at or past the end reads `'\0'` (the C++ only defines `[length ()]`;
/// further out it is undefined, and `'\0'` is the benign reading).
fn byte_at(bytes: &[u8], index: usize) -> u8 {
    bytes.get(index).copied().unwrap_or(0)
}

/// C++ `std::string::npos`.
const NPOS: usize = usize::MAX;

/// C++ `std::string::find (needle, pos)` on bytes: [`NPOS`] when absent or
/// when `pos` is past the end.
fn cpp_find(haystack: &str, needle: &str, pos: usize) -> usize {
    let hay = haystack.as_bytes();
    let needle = needle.as_bytes();
    if pos > hay.len() || needle.len() > hay.len() - pos {
        return NPOS;
    }

    (pos..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle).unwrap_or(NPOS)
}

/// C++ `i.second->dump ()` followed by `Lexer::dequote (text)`.
fn dequoted_dump(value: &JsonValue) -> String {
    let mut text = value.dump();
    Lexer::dequote(&mut text);
    text
}

/// C++ `((json::string*) v)->_data`: the raw (still escaped) string of a
/// string node. The C++ casts without checking; for a node of another type
/// that is undefined behaviour, so the Rust port uses the dequoted dump
/// instead (the closest well-defined reading).
fn raw_string(value: &JsonValue) -> String {
    match value.as_str() {
        Some(text) => text.to_string(),
        None => dequoted_dump(value),
    }
}

/// C++ `Task::attributes[name]` as a lookup. The C++ `operator[]` inserts an
/// empty entry for an unknown name; nothing ever iterates the static map, so
/// that insertion is unobservable and is not reproduced.
fn attribute_type(name: &str) -> String {
    Task::with_attributes(|attributes| attributes.get(name).cloned().unwrap_or_default())
}

impl Task {
    /// C++ `Task::Task (const std::string& input)`: a task parsed from a JSON
    /// line with [`Task::parse`].
    ///
    /// The C++ constructor reaches the Context through `addDependency` and
    /// the orphan debug message, so the Rust one takes it explicitly.
    ///
    /// # Errors
    /// Those of [`Task::parse`].
    pub fn from_input(ctx: &mut Context, input: &str) -> Result<Task> {
        let mut task = Task::default();
        task.parse(ctx, input)?;
        Ok(task)
    }

    /// C++ `Task::Task (const json::object* obj)`: a task built from a parsed
    /// JSON object with [`Task::parse_json_object`] (no `tags`/`depends`
    /// fix-ups, unlike [`Task::from_input`]).
    ///
    /// # Errors
    /// Those of [`Task::parse_json_object`].
    pub fn from_json_object(ctx: &mut Context, obj: &JsonValue) -> Result<Task> {
        let mut task = Task::default();
        task.parse_json_object(ctx, obj)?;
        Ok(task)
    }

    /// C++ `void Task::parse (const std::string& input)`: a JSON parse, then
    /// the legacy `tags`/`depends` comma-separated attributes are mirrored
    /// into `tag_..`/`dep_..` attributes and back.
    ///
    /// # Errors
    /// Those of [`Task::parse_json`].
    pub fn parse(&mut self, ctx: &mut Context, input: &str) -> Result<()> {
        self.parse_json(ctx, input)?;

        // for compatibility, include all tags in `tags` as `tag_..` attributes
        if let Some(tags) = self.data.get("tags").cloned() {
            for tag in split(&tags, ',') {
                self.data.insert(Task::tag2attr(&tag), "x".to_string());
            }
        }
        // ..and similarly, update `tags` to match the `tag_..` attributes
        self.fix_tags_attribute();

        // same for `depends` / `dep_..`
        if let Some(depends) = self.data.get("depends").cloned() {
            for dep in split(&depends, ',') {
                self.data.insert(Task::dep2attr(&dep), "x".to_string());
            }
        }
        self.fix_depends_attribute();

        self.recalc_urgency = true;
        Ok(())
    }

    /// C++ `void Task::parseJSON (const std::string& line)`: parses the whole
    /// line; a root that is not an object (an array) is silently ignored.
    ///
    /// # Errors
    /// The [`json::parse`] errors (verbatim C++ text), and those of
    /// [`Task::parse_json_object`].
    pub fn parse_json(&mut self, ctx: &mut Context, line: &str) -> Result<()> {
        // Parse the whole thing.
        let root = json::parse(line)?;
        if root.jtype() == JType::Object {
            self.parse_json_object(ctx, &root)?;
        }
        Ok(())
    }

    /// C++ `void Task::parseJSON (const json::object* root_obj)`. Note that
    /// all fields undergo encode/decode.
    ///
    /// `Task::attributes` (name -> type) decides the handling of each element;
    /// names without a type are UDA orphans (or `annotations`). A `root_obj`
    /// that is not an object contributes nothing (the C++ cast would be
    /// undefined behaviour).
    ///
    /// # Errors
    /// * `Annotations is malformed: {1}` when `annotations` is not an array;
    /// * `Annotation is missing a description: {1}` for an annotation without
    ///   a `description` (an element that is not an object counts as such);
    /// * `'{1}' is not a valid date in the '{2}' format.` for a date (or
    ///   `modification`, or annotation `entry`) that does not parse;
    /// * those of [`Task::add_dependency`] for `depends`.
    pub fn parse_json_object(&mut self, ctx: &mut Context, root_obj: &JsonValue) -> Result<()> {
        let Some(elements) = root_obj.as_object() else {
            return Ok(());
        };

        // For each object element...
        for (name, value) in elements {
            // If the attribute is a recognized column.
            let type_ = attribute_type(name);
            if !type_.is_empty() {
                // Any specified id is ignored.
                // Urgency, if present, is ignored.
                if name == "id" || name == "urgency" {
                }
                // TW-1274 Standardization.
                else if name == "modification" {
                    let text = dequoted_dump(value);
                    let d = Datetime::from_string(&text, "")?;
                    self.set("modified", &d.to_epoch_string());
                }
                // Dates are converted from ISO to epoch.
                else if type_ == "date" {
                    let text = dequoted_dump(value);
                    let d = Datetime::from_string(&text, "")?;
                    let epoch = if text.is_empty() { String::new() } else { d.to_epoch_string() };
                    self.set(name, &epoch);
                }
                // Tags are an array of JSON strings.
                else if name == "tags" && value.jtype() == JType::Array {
                    for t in value.as_array().into_iter().flatten() {
                        self.add_tag(&raw_string(t));
                    }
                }
                // Dependencies can be exported as an array of strings.
                // 2016-02-21: This will be the only option in future releases.
                //             See other 2016-02-21 comments for details.
                else if name == "depends" && value.jtype() == JType::Array {
                    for t in value.as_array().into_iter().flatten() {
                        self.add_dependency(ctx, &raw_string(t))?;
                    }
                }
                // Dependencies can be exported as a single comma-separated string.
                // 2016-02-21: Deprecated - see other 2016-02-21 comments for details.
                else if name == "depends" && value.jtype() == JType::String {
                    // Fix for issue#2689: taskserver sometimes encodes the depends
                    // property as a string of the format `[\"uuid\",\"uuid\"]`
                    // The string includes the backslash-escaped `"` characters, making
                    // it invalid JSON.  Since we know the characters we're looking for,
                    // we'll just filter out everything else.
                    //
                    // (`front ()`/`back ()` of an empty string are undefined in C++;
                    // an empty string is simply not bracketed here.)
                    let mut deps_str = raw_string(value);
                    if deps_str.starts_with('[') && deps_str.ends_with(']') {
                        deps_str = deps_str
                            .chars()
                            .filter(|&c| c.is_ascii_digit() || ('a'..='f').contains(&c) || c == ',' || c == '-')
                            .collect();
                    }
                    let uuids = split(&deps_str, ',');

                    for uuid in &uuids {
                        self.add_dependency(ctx, uuid)?;
                    }
                }
                // Strings are decoded.
                else if type_ == "string" {
                    let text = dequoted_dump(value);
                    self.set(name, &json::decode(&text));
                }
                // Other types are simply added.
                else {
                    let text = dequoted_dump(value);
                    self.set(name, &text);
                }
            }
            // UDA orphans and annotations do not have columns.
            else {
                // Annotations are an array of JSON objects with 'entry' and
                // 'description' values and must be converted.
                if name == "annotations" {
                    let mut annos: BTreeMap<String, String> = BTreeMap::new();

                    // Fail if 'annotations' is not an array
                    let Some(atts) = value.as_array() else {
                        return Err(TwError::Message(format("Annotations is malformed: {1}", &[&value.dump()])));
                    };

                    for annotation in atts {
                        // Extract description. Fail if not present.
                        let Some(what) = annotation.get("description") else {
                            return Err(TwError::Message(format(
                                "Annotation is missing a description: {1}",
                                &[&annotation.dump()],
                            )));
                        };

                        // Extract 64-bit annotation entry value
                        // Time travelers from 2038, we have your back.
                        //
                        // Extract entry. Use current time if not present.
                        let mut ann_timestamp: i64 = match annotation.get("entry") {
                            Some(when) => Datetime::from_string(&raw_string(when), "")?.to_epoch(),
                            None => Datetime::new().to_epoch(),
                        };

                        let mut key = format!("annotation_{ann_timestamp}");

                        // Increment the entry timestamp in case of a conflict. Same
                        // behaviour as CmdAnnotate.
                        while annos.contains_key(&key) {
                            ann_timestamp += 1;
                            key = format!("annotation_{ann_timestamp}");
                        }

                        annos.entry(key).or_insert_with(|| json::decode(&raw_string(what)));
                    }

                    self.set_annotations(&annos);
                }
                // UDA Orphan - must be preserved.
                else {
                    // The C++ streams the `json::value*` itself, i.e. its
                    // address; `{:p}` prints the address of the node likewise.
                    ctx.debug(&format!(
                        "Task::parseJSON found orphan '{name}' with value '{:p}' --> preserved\n",
                        value
                    ));
                    let text = dequoted_dump(value);
                    self.set(name, &json::decode(&text));
                }
            }
        }

        Ok(())
    }

    /// C++ `void Task::parseLegacy (const std::string& line)`: no legacy
    /// formats are currently supported as of 2.4.0. Format 4 is accepted (and
    /// only marks urgency for recalculation); formats 1-3 and unrecognized
    /// lines are errors. The unrecognized case logs a debug message to `ctx`
    /// when one is given.
    ///
    /// # Errors
    /// The C++ text for formats 1, 2 and 3, or `Unrecognized Taskwarrior file
    /// format or blank line in data.`.
    pub fn parse_legacy(&mut self, ctx: Option<&mut Context>, line: &str) -> Result<()> {
        match Task::determine_version(line) {
            // File format version 1, from 2006-11-27 - 2007-12-31, v0.x+ - v0.9.3
            1 => {
                return Err(TwError::Message(
                    "Taskwarrior no longer supports file format 1, originally used between 27 November 2006 \
                     and 31 December 2007."
                        .to_string(),
                ))
            }

            // File format version 2, from 2008-1-1 - 2009-3-23, v0.9.3 - v1.5.0
            2 => {
                return Err(TwError::Message(
                    "Taskwarrior no longer supports file format 2, originally used between 1 January 2008 \
                     and 12 April 2009."
                        .to_string(),
                ))
            }

            // File format version 3, from 2009-3-23 - 2009-05-16, v1.6.0 - v1.7.1
            3 => {
                return Err(TwError::Message(
                    "Taskwarrior no longer supports file format 3, originally used between 23 March 2009 and \
                     16 May 2009."
                        .to_string(),
                ))
            }

            // File format version 4, from 2009-05-16 - today, v1.7.1+
            4 => {}

            _ => {
                if let Some(ctx) = ctx {
                    ctx.debug(&format!("Invalid fileformat at line '{line}'"));
                }
                return Err(TwError::Message(
                    "Unrecognized Taskwarrior file format or blank line in data.".to_string(),
                ));
            }
        }

        self.recalc_urgency = true;
        Ok(())
    }

    /// C++ `static int Task::determineVersion (const std::string& line)`:
    /// 1-4 for the historical file formats, 0 for "no idea". Positions past
    /// the end of `line` read as `'\0'`.
    pub fn determine_version(line: &str) -> i32 {
        let b = line.as_bytes();
        let last = byte_at(b, b.len().wrapping_sub(1));

        // Version 2 looks like:
        //
        //   uuid status [tags] [attributes] description\n
        //
        // Where uuid looks like:
        //
        //   27755d92-c5e9-4c21-bd8e-c3dd9e6d3cf7
        //
        // Scan for the hyphens in the uuid, the following space, and a valid status
        // character.
        if byte_at(b, 8) == b'-'
            && byte_at(b, 13) == b'-'
            && byte_at(b, 18) == b'-'
            && byte_at(b, 23) == b'-'
            && byte_at(b, 36) == b' '
            && matches!(byte_at(b, 37), b'-' | b'+' | b'X' | b'r')
        {
            // Version 3 looks like:
            //
            //   uuid status [tags] [attributes] [annotations] description\n
            //
            // Scan for the number of [] pairs. (As in C++, `npos + 1` wraps
            // to 0.)
            let tag_atts = cpp_find(line, "] [", 0);
            let atts_anno = cpp_find(line, "] [", tag_atts.wrapping_add(1));
            let anno_desc = cpp_find(line, "] ", atts_anno.wrapping_add(1));
            if tag_atts != NPOS && atts_anno != NPOS && anno_desc != NPOS {
                3
            } else {
                2
            }
        }
        // Version 4 looks like:
        //
        //   [name:"value" ...]
        //
        // Scan for [, ] and :".
        else if byte_at(b, 0) == b'[' && last == b']' && line.contains("uuid:\"") {
            4
        }
        // Version 1 looks like:
        //
        //   [tags] [attributes] description\n
        //   X [tags] [attributes] description\n
        //
        // Scan for the first character being either the bracket or X.
        else if line.starts_with("X [") || (byte_at(b, 0) == b'[' && last != b']' && b.len() > 3) {
            1
        }
        // Version 5?
        //
        // Fortunately, with the hindsight that will come with version 5, the
        // identifying characteristics of 1, 2, 3 and 4 may be modified such that if 5
        // has a UUID followed by a status, then there is still a way to differentiate
        // between 2, 3, 4 and 5.
        //
        // The danger is that a version 3 binary reads and misinterprets a version 4
        // file.  This is why it is a good idea to rely on an explicit version
        // declaration rather than chance positioning.

        // Zero means 'no idea'.
        else {
            0
        }
    }

    /// C++ `std::string Task::composeJSON (bool decorate = false)`. With
    /// `decorate` the output starts with `"id":<id>` and ends with
    /// `"urgency":<urgency ()>` (C++ `out << float`: `%g` at precision 6).
    ///
    /// # Errors
    /// Those of [`Task::urgency`] (when decorating) and of
    /// [`Task::compose_json_plain`].
    pub fn compose_json(&mut self, ctx: &mut Context, decorate: bool) -> Result<String> {
        let urgency = if decorate { Some(self.urgency(ctx)?) } else { None };
        self.compose_json_impl(decorate, urgency)
    }

    /// C++ `composeJSON ()` / `composeJSON (false)`: the undecorated form,
    /// which needs no Context.
    ///
    /// # Errors
    /// `'{1}' is not a valid date in the '' format.` when a date attribute (or
    /// an annotation timestamp) does not parse, as the C++ `Datetime`
    /// constructor throws.
    pub fn compose_json_plain(&self) -> Result<String> {
        self.compose_json_impl(false, None)
    }

    fn compose_json_impl(&self, decorate: bool, urgency: Option<f32>) -> Result<String> {
        let mut out = String::new();
        out.push('{');

        // ID inclusion is optional, but not a good idea, because it remains correct
        // only until the next gc.
        if decorate {
            out.push_str(&format!("\"id\":{},", self.id));
        }

        // First the non-annotations.
        let mut attributes_written = 0;
        for (name, value) in &self.data {
            // Annotations are not written out here.
            if name.starts_with("annotation_") {
                continue;
            }

            // Tags and dependencies are handled below
            if name == "tags" || Task::is_tag_attr(name) {
                continue;
            }
            if name == "depends" || Task::is_dep_attr(name) {
                continue;
            }

            // If value is an empty string, do not ever output it
            if value.is_empty() {
                continue;
            }

            let mut type_ = attribute_type(name);
            if type_.is_empty() {
                type_ = "string".to_string();
            }

            // Date fields are written as ISO 8601.
            if type_ == "date" {
                let epoch = self.get_date(name);
                if epoch != 0 {
                    let d = Datetime::from_string(value, "")?;
                    if attributes_written != 0 {
                        out.push(',');
                    }

                    out.push('"');
                    out.push_str(if name == "modification" { "modified" } else { name });
                    out.push_str("\":\"");
                    // Date was deleted, do not export parsed empty string
                    out.push_str(&d.to_iso());
                    out.push('"');

                    attributes_written += 1;
                }
            } else if type_ == "numeric" {
                if attributes_written != 0 {
                    out.push(',');
                }

                out.push('"');
                out.push_str(name);
                out.push_str("\":");
                out.push_str(value);

                attributes_written += 1;
            }
            // Everything else is a quoted value.
            else {
                if attributes_written != 0 {
                    out.push(',');
                }

                out.push('"');
                out.push_str(name);
                out.push_str("\":\"");
                if type_ == "string" {
                    out.push_str(&json::encode(value));
                } else {
                    out.push_str(value);
                }
                out.push('"');

                attributes_written += 1;
            }
        }

        // Now the annotations, if any.
        if self.annotation_count != 0 {
            out.push_str(",\"annotations\":[");

            let mut annotations_written = 0;
            for (name, value) in &self.data {
                if let Some(stamp) = name.strip_prefix("annotation_") {
                    if annotations_written != 0 {
                        out.push(',');
                    }

                    let d = Datetime::from_string(stamp, "")?;
                    out.push_str("{\"entry\":\"");
                    out.push_str(&d.to_iso());
                    out.push_str("\",\"description\":\"");
                    out.push_str(&json::encode(value));
                    out.push_str("\"}");

                    annotations_written += 1;
                }
            }

            out.push(']');
        }

        let tags = self.get_tags();
        if !tags.is_empty() {
            out.push_str(",\"tags\":[");
            for (count, tag) in tags.iter().enumerate() {
                if count != 0 {
                    out.push(',');
                }
                out.push('"');
                out.push_str(tag);
                out.push('"');
            }
            out.push(']');
        }

        let depends = self.get_dependency_uuids();
        if !depends.is_empty() {
            out.push_str(",\"depends\":[");
            for (count, dep) in depends.iter().enumerate() {
                if count != 0 {
                    out.push(',');
                }
                out.push('"');
                out.push_str(dep);
                out.push('"');
            }
            out.push(']');
        }

        // Include urgency.
        if let Some(urgency) = urgency {
            out.push_str(",\"urgency\":");
            out.push_str(&stream_double(f64::from(urgency)));
        }

        out.push('}');
        Ok(out)
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
        fn latest_id(&mut self) -> Result<i32> {
            Ok(self.latest)
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
        .filter(|tag| t.has_tag(tag, e).unwrap())
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
        // As in C++, hasTag propagates the getStatus throw.
        assert_eq!(
            t.has_tag("PENDING", &mut NullEnv).unwrap_err().to_string(),
            "The status 'bogus' is not valid."
        );
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
        assert!(t.has_tag("home", &mut NullEnv).unwrap());
        assert!(!t.has_tag("work", &mut NullEnv).unwrap());
        assert!(VariantTask::has_tag(&t, "garden").unwrap());

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
        assert!(t.has_tag("FOO", &mut NullEnv).unwrap());
        // A concrete tag shadowed by a virtual one.
        t.add_tag("PROJECT");
        assert!(!t.has_tag("PROJECT", &mut NullEnv).unwrap());
        assert!(!t.has_tag("", &mut NullEnv).unwrap());
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
        assert!(t.is_overdue(&mut e).unwrap());
        assert!(!t.is_due(&mut e).unwrap());
        assert!(!t.is_duetoday(&mut e).unwrap());
        assert!(!t.is_dueyesterday().unwrap());
        assert!(!t.is_duetomorrow().unwrap());

        // Yesterday.
        let t = pending_due(epoch_of("yesterday") + 3600);
        assert!(t.is_overdue(&mut e).unwrap());
        assert!(t.is_dueyesterday().unwrap());
        assert!(!t.is_due(&mut e).unwrap());

        // Today (end of the day: later today).
        let t = pending_due(epoch_of("tomorrow") - 1);
        assert!(t.is_due(&mut e).unwrap());
        assert!(t.is_duetoday(&mut e).unwrap());
        assert!(!t.is_overdue(&mut e).unwrap());
        assert!(!t.is_duetomorrow().unwrap());
        assert!(t.is_dueweek().unwrap());
        assert!(t.is_duemonth().unwrap());
        assert!(t.is_duequarter().unwrap());
        assert!(t.is_dueyear().unwrap());

        // Today, earlier: overdue and due today.
        let t = pending_due(today + 1);
        if today + 1 < now() {
            assert!(t.is_overdue(&mut e).unwrap());
            assert!(t.is_duetoday(&mut e).unwrap());
        }

        // Tomorrow.
        let t = pending_due(epoch_of("tomorrow") + 3600);
        assert!(t.is_due(&mut e).unwrap());
        assert!(t.is_duetomorrow().unwrap());
        assert!(!t.is_duetoday(&mut e).unwrap());
        assert!(!t.is_overdue(&mut e).unwrap());

        // In a year and a day: none.
        let t = pending_due(epoch_of("eoy") + DAY);
        assert!(!t.is_due(&mut e).unwrap());
        assert!(!t.is_dueyear().unwrap());
        assert!(!t.is_duequarter().unwrap());
        assert!(!t.is_duemonth().unwrap());
        assert!(!t.is_dueweek().unwrap());

        // Range boundaries are inclusive.
        let t = pending_due(epoch_of("sow"));
        assert!(t.is_dueweek().unwrap());
        let t = pending_due(epoch_of("eom"));
        assert!(t.is_duemonth().unwrap());
        let t = pending_due(epoch_of("soq"));
        assert!(t.is_duequarter().unwrap());
        let t = pending_due(epoch_of("soy") - 1);
        assert!(!t.is_dueyear().unwrap());

        // Completed/deleted tasks are never due.
        for status in ["completed", "deleted"] {
            let mut t = pending_due(epoch_of("tomorrow") - 1);
            t.set("status", status);
            assert!(!t.is_due(&mut e).unwrap());
            assert!(!t.is_duetoday(&mut e).unwrap());
            assert!(!t.is_overdue(&mut e).unwrap());
            assert!(!t.is_dueyear().unwrap());
            let mut t = pending_due(epoch_of("yesterday") + 1);
            t.set("status", status);
            assert!(!t.is_dueyesterday().unwrap());
            assert!(!t.is_overdue(&mut e).unwrap());
        }

        // Recurring templates are due but never overdue.
        let mut t = pending_due(epoch_of("yesterday") + 3600);
        t.set("status", "recurring");
        assert!(!t.is_overdue(&mut e).unwrap());
        assert!(t.is_dueyesterday().unwrap());

        // No due.
        let t = Task::new();
        assert!(!t.is_due(&mut e).unwrap());
        assert!(!t.is_overdue(&mut e).unwrap());
        assert!(!t.is_dueweek().unwrap());
    }

    #[test]
    fn ready() {
        let mut t = Task::new();
        t.set("status", "pending");
        assert!(t.is_ready().unwrap());
        t.set_int("scheduled", now() - DAY);
        assert!(t.is_ready().unwrap());
        t.set_int("scheduled", now() + DAY);
        assert!(!t.is_ready().unwrap());
        t.remove("scheduled");
        t.is_blocked = true;
        assert!(!t.is_ready().unwrap());
        t.is_blocked = false;
        t.set("status", "completed");
        assert!(!t.is_ready().unwrap());
        // Waiting is not pending.
        t.set("status", "pending");
        t.set_int("wait", now() + DAY);
        assert!(!t.is_ready().unwrap());
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
        assert!(t.has_tag("TEMPLATE", &mut e).unwrap());
        assert!(t.has_tag("PARENT", &mut e).unwrap());
        let mut t = Task::new();
        t.set("template", "u");
        assert!(t.has_tag("INSTANCE", &mut e).unwrap());
        assert!(t.has_tag("CHILD", &mut e).unwrap());
    }

    #[test]
    fn virtual_tags_status() {
        let mut e = env();
        let mut t = Task::new();
        t.set("status", "completed");
        assert!(t.has_tag("COMPLETED", &mut e).unwrap());
        assert!(!t.has_tag("PENDING", &mut e).unwrap());
        assert!(!t.has_tag("READY", &mut e).unwrap());
        t.set("status", "deleted");
        assert!(t.has_tag("DELETED", &mut e).unwrap());
        t.set("status", "pending");
        t.set_int("wait", now() + DAY);
        assert!(t.has_tag("WAITING", &mut e).unwrap());
        assert!(!t.has_tag("PENDING", &mut e).unwrap());
        // No status attribute reads as pending.
        let t = Task::new();
        assert!(t.has_tag("PENDING", &mut e).unwrap());
    }

    #[test]
    fn virtual_tags_dates() {
        let mut e = env();
        let today = epoch_of("today");

        let t = pending_due(epoch_of("tomorrow") - 1);
        assert!(t.has_tag("DUE", &mut e).unwrap());
        assert!(t.has_tag("DUETODAY", &mut e).unwrap());
        assert!(t.has_tag("TODAY", &mut e).unwrap());
        assert!(!t.has_tag("OVERDUE", &mut e).unwrap());
        assert!(!t.has_tag("TOMORROW", &mut e).unwrap());
        assert!(!t.has_tag("YESTERDAY", &mut e).unwrap());
        assert!(t.has_tag("MONTH", &mut e).unwrap());
        assert!(t.has_tag("QUARTER", &mut e).unwrap());
        assert!(t.has_tag("YEAR", &mut e).unwrap());
        assert_eq!(t.has_tag("WEEK", &mut e).unwrap(), t.is_dueweek().unwrap());

        let t = pending_due(epoch_of("tomorrow") + 1);
        assert!(t.has_tag("TOMORROW", &mut e).unwrap());
        assert!(t.has_tag("DUE", &mut e).unwrap());
        assert!(!t.has_tag("TODAY", &mut e).unwrap());

        let t = pending_due(epoch_of("yesterday") + 1);
        assert!(t.has_tag("YESTERDAY", &mut e).unwrap());
        assert!(t.has_tag("OVERDUE", &mut e).unwrap());
        assert!(!t.has_tag("DUE", &mut e).unwrap());

        // The rc.due window comes from the environment.
        let t = pending_due(today + 5 * DAY);
        assert!(t.has_tag("DUE", &mut e).unwrap());
        e.due = 2;
        assert!(!t.has_tag("DUE", &mut e).unwrap());
        e.due = 0;
        assert!(t.has_tag("DUE", &mut e).unwrap());
        // NullEnv uses the default rc.due=7.
        assert!(t.has_tag("DUE", &mut NullEnv).unwrap());
        let t = pending_due(today + 9 * DAY);
        assert!(!t.has_tag("DUE", &mut NullEnv).unwrap());
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
        assert!(!t.has_tag("UDA", &mut e).unwrap());
        // `tags` itself has no column here, so it is an orphan.
        assert!(t.has_tag("ORPHAN", &mut e).unwrap());
        assert_eq!(t.get_uda_orphans(&mut e), vec!["tags"]);
        e.columns.push("tags".into());
        assert!(!t.is_orphan_present(&mut e));
        assert!(t.get_uda_orphans(&mut e).is_empty());

        t.set("estimate", "3");
        assert!(t.is_uda_present(&mut e));
        assert!(t.has_tag("UDA", &mut e).unwrap());

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
        assert!(t.has_tag("LATEST", &mut e).unwrap());
        e.latest = 5;
        assert!(!t.has_tag("LATEST", &mut e).unwrap());
        // NullEnv latest_id is 0.
        assert!(!t.has_tag("LATEST", &mut NullEnv).unwrap());
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
            assert!(vt.has_tag("UDA").unwrap());
            assert!(vt.has_tag("LATEST").unwrap());
            assert!(vt.has_tag("ANNOTATED").unwrap());
            assert_eq!(vt.get_annotations().len(), 1);
            assert_eq!(adapter.task().id, 2);
        }
        // Plain Task as VariantTask uses NullEnv.
        assert!(!VariantTask::has_tag(&t, "UDA").unwrap());
        assert!(!VariantTask::has_tag(&t, "LATEST").unwrap());
    }

    #[test]
    fn invalid_status_propagates_from_predicates() {
        // C++ `getStatus ()` throws from these predicates and from hasTag.
        let mut e = env();
        let mut t = Task::new();
        t.set("status", "bogus");
        t.set_int("due", now() + DAY);
        let expected = "The status 'bogus' is not valid.";
        assert_eq!(t.is_ready().unwrap_err().to_string(), expected);
        assert_eq!(t.is_due(&mut e).unwrap_err().to_string(), expected);
        assert_eq!(t.is_dueyesterday().unwrap_err().to_string(), expected);
        assert_eq!(t.is_duetoday(&mut e).unwrap_err().to_string(), expected);
        assert_eq!(t.is_duetomorrow().unwrap_err().to_string(), expected);
        assert_eq!(t.is_dueweek().unwrap_err().to_string(), expected);
        assert_eq!(t.is_duemonth().unwrap_err().to_string(), expected);
        assert_eq!(t.is_duequarter().unwrap_err().to_string(), expected);
        assert_eq!(t.is_dueyear().unwrap_err().to_string(), expected);
        assert_eq!(t.is_overdue(&mut e).unwrap_err().to_string(), expected);
        for tag in [
            "READY", "DUE", "DUETODAY", "TODAY", "YESTERDAY", "TOMORROW", "OVERDUE", "WEEK", "MONTH", "QUARTER",
            "YEAR", "PENDING", "COMPLETED", "DELETED",
        ] {
            assert_eq!(t.has_tag(tag, &mut e).unwrap_err().to_string(), expected, "{tag}");
        }
        // Virtual tags that do not read the status, and concrete tags, still work.
        assert!(!t.has_tag("BLOCKED", &mut e).unwrap());
        assert!(!t.has_tag("WAITING", &mut e).unwrap());
        assert!(!t.has_tag("home", &mut e).unwrap());
        // Without a due date the is_due* predicates never read the status.
        t.remove("due");
        assert!(!t.is_due(&mut e).unwrap());
        assert!(!t.is_overdue(&mut e).unwrap());
        // Through the Variant interface, as Eval's `_hastag_` uses it.
        let v = crate::variant::Variant::default();
        let err = v.op_hastag(&crate::variant::Variant::from_string("PENDING"), &t).unwrap_err();
        assert_eq!(err.to_string(), expected);
        let err = v.op_notag(&crate::variant::Variant::from_string("PENDING"), &t).unwrap_err();
        assert_eq!(err.to_string(), expected);
    }

    /// A [`TaskEnv`] whose `latest_id` fails, like a TDB2 whose working set
    /// cannot be loaded.
    struct FailingLatest;
    impl TaskEnv for FailingLatest {
        fn due_days(&self) -> i32 {
            7
        }
        fn has_column(&self, _name: &str) -> bool {
            false
        }
        fn uda_columns(&self) -> Vec<String> {
            Vec::new()
        }
        fn latest_id(&mut self) -> Result<i32> {
            Err(TwError::Message("working set unavailable".into()))
        }
    }

    #[test]
    fn latest_id_error_propagates() {
        let t = Task::new();
        assert_eq!(t.has_tag("LATEST", &mut FailingLatest).unwrap_err().to_string(), "working set unavailable");
        // Only +LATEST reads latest_id.
        assert!(t.has_tag("PENDING", &mut FailingLatest).unwrap());
        let mut env = FailingLatest;
        let adapter = TaskWithEnv::new(&t, &mut env);
        let err = crate::variant::Variant::default()
            .op_hastag(&crate::variant::Variant::from_string("LATEST"), &adapter)
            .unwrap_err();
        assert_eq!(err.to_string(), "working set unavailable");
    }

    struct UdaCol(crate::columns::column::ColumnBase);
    impl UdaCol {
        fn new(uda: bool) -> Self {
            let mut base = crate::columns::column::ColumnBase::new();
            base.uda = uda;
            UdaCol(base)
        }
    }
    impl Column for UdaCol {
        fn base(&self) -> &crate::columns::column::ColumnBase {
            &self.0
        }

        fn base_mut(&mut self) -> &mut crate::columns::column::ColumnBase {
            &mut self.0
        }
    }

    #[test]
    fn context_env() {
        let mut ctx = Context::default();
        ctx.config.set("due", "3");
        ctx.columns.insert("estimate".to_string(), Rc::new(UdaCol::new(true)));
        ctx.columns.insert("description".to_string(), Rc::new(UdaCol::new(false)));
        assert_eq!(TaskEnv::due_days(&ctx), 3);
        assert!(ctx.has_column("description"));
        assert!(!ctx.has_column("project"));
        assert_eq!(ctx.uda_columns(), vec!["estimate"]);
        // No replica open: C++ `TDB2::replica ()` asserts; the error propagates
        // out of latest_id and +LATEST.
        assert_eq!(TaskEnv::latest_id(&mut ctx).unwrap_err().to_string(), "replica not open");

        let mut t = Task::new();
        t.set("estimate", "2");
        assert!(t.has_tag("UDA", &mut ctx).unwrap());
        assert_eq!(t.has_tag("LATEST", &mut ctx).unwrap_err().to_string(), "replica not open");

        // With a replica, +LATEST compares against the working set.
        let dir = tempfile::TempDir::new().unwrap();
        ctx.tdb2.open_replica_at(dir.path().to_str().unwrap(), true, true).unwrap();
        assert_eq!(TaskEnv::latest_id(&mut ctx).unwrap(), 0);
        assert!(t.has_tag("LATEST", &mut ctx).unwrap());
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

/// Tests for validation and dependencies (the Context-bound part of Task)
/// and `src/dependency.rs`.
#[cfg(test)]
mod validate_tests {
    use super::*;
    use crate::dependency::{
        dependency_chain_on_complete_to, dependency_chain_on_start_to, dependency_is_circular,
    };
    use crate::test_util::env_lock;
    use crate::tdb2;
    use tempfile::TempDir;

    /// Restores the `Task::default*`, `Datetime::standaloneDateEnabled` and
    /// `Duration::standaloneSecondsEnabled` thread-local statics when dropped.
    struct DefaultsGuard;

    impl Drop for DefaultsGuard {
        fn drop(&mut self) {
            Task::set_default_project("");
            Task::set_default_due("");
            Task::set_default_scheduled("");
            Datetime::set_standalone_date_enabled(true);
            crate::shared::duration::Duration::set_standalone_seconds_enabled(true);
        }
    }

    /// A Context whose replica lives in a temporary data directory. The
    /// TempDir field is declared before the lock guard so it is removed
    /// before the lock is released.
    struct Fixture {
        ctx: Context,
        _dir: TempDir,
        _defaults: DefaultsGuard,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    fn fixture() -> Fixture {
        // Context::new reads $HOME; serialize with env-mutating tests.
        let lock = env_lock();
        let dir = TempDir::new().unwrap();
        let mut ctx = Context::new();
        // As Context::staticInitialization does.
        Datetime::set_standalone_date_enabled(false);
        crate::shared::duration::Duration::set_standalone_seconds_enabled(false);
        // As Context::initialize does (Task::validate reads the registry).
        crate::columns::column::factory_all(&mut ctx.columns, &ctx.config).unwrap();
        ctx.tdb2.open_replica_at(dir.path().to_str().unwrap(), true, true).unwrap();
        Fixture { ctx, _dir: dir, _defaults: DefaultsGuard, _lock: lock }
    }

    fn err(r: Result<()>) -> String {
        r.unwrap_err().to_string()
    }

    fn described(description: &str) -> Task {
        let mut t = Task::new();
        t.set("description", description);
        t
    }

    /// Adds a task through `tdb2::add` (validate + hooks + storage).
    fn add(ctx: &mut Context, description: &str) -> Task {
        let mut t = described(description);
        tdb2::add(ctx, &mut t).unwrap();
        t
    }

    #[test]
    fn validate_add_errors() {
        let mut f = fixture();
        let t = Task::new();
        assert_eq!(err(t.validate_add(&mut f.ctx)), "A task must have a description.");
        let t = described("");
        assert_eq!(err(t.validate_add(&mut f.ctx)), "Cannot add a task that is blank.");
        let mut t = described("x");
        t.set("recur", "weekly");
        assert_eq!(err(t.validate_add(&mut f.ctx)), "A recurring task must also have a 'due' date.");
        t.set("due", "");
        assert_eq!(err(t.validate_add(&mut f.ctx)), "A recurring task must also have a 'due' date.");
        t.set("due", "1700000000");
        assert!(t.validate_add(&mut f.ctx).is_ok());
    }

    #[test]
    fn validate_status_and_uuid() {
        let mut f = fixture();
        let mut t = described("x");
        t.set("status", "bogus");
        assert_eq!(err(t.validate(&mut f.ctx, true)), "The status 'bogus' is not valid.");

        let mut t = described("x");
        t.set("uuid", "not-a-uuid");
        assert_eq!(err(t.validate(&mut f.ctx, true)), "Not a valid UUID 'not-a-uuid'.");

        // Missing (or empty) uuid is generated; status defaults to pending.
        let mut t = described("x");
        t.set("uuid", "");
        t.validate(&mut f.ctx, true).unwrap();
        assert_eq!(t.get("uuid").len(), 36);
        assert_eq!(t.get("status"), "pending");
        assert!(t.has("entry"));
        assert!(t.has("modified"));
        assert!(f.ctx.footnotes.is_empty());

        // A valid uuid is kept, as are given entry / modified dates.
        let mut t = described("x");
        t.set("uuid", "00000000-0000-0000-0000-000000000001");
        t.set("entry", "1700000000");
        t.set("modified", "1700000001");
        t.validate(&mut f.ctx, true).unwrap();
        assert_eq!(t.get("uuid"), "00000000-0000-0000-0000-000000000001");
        assert_eq!(t.get("entry"), "1700000000");
        assert_eq!(t.get("modified"), "1700000001");
    }

    #[test]
    fn validate_derived_status() {
        let mut f = fixture();

        // Recurring: due + recur, no parent/template; rtype defaults to periodic.
        let mut t = described("x");
        t.set("due", "1700000000");
        t.set("recur", "weekly");
        t.validate(&mut f.ctx, false).unwrap();
        assert_eq!(t.get("status"), "recurring");
        assert_eq!(t.get("rtype"), "periodic");

        // A given rtype is kept.
        let mut t = described("x");
        t.set("due", "1700000000");
        t.set("recur", "weekly");
        t.set("rtype", "chained");
        t.validate(&mut f.ctx, false).unwrap();
        assert_eq!(t.get("rtype"), "chained");

        // A child (parent set) stays pending.
        let mut t = described("x");
        t.set("due", "1700000000");
        t.set("recur", "weekly");
        t.set("parent", "00000000-0000-0000-0000-000000000009");
        t.validate(&mut f.ctx, false).unwrap();
        assert_eq!(t.get("status"), "pending");
        assert!(!t.has("rtype"));

        // Waiting is stored as pending.
        let mut t = described("x");
        t.set("wait", "4000000000");
        t.validate(&mut f.ctx, false).unwrap();
        assert_eq!(t.get("status"), "pending");

        // Completed/deleted tasks get an end date; pending ones lose theirs.
        for s in ["completed", "deleted"] {
            let mut t = described("x");
            t.set("status", s);
            t.validate(&mut f.ctx, false).unwrap();
            assert_eq!(t.get("status"), s);
            assert!(!t.get("end").is_empty());
        }
        let mut t = described("x");
        t.set("status", "pending");
        t.set("end", "1700000000");
        t.validate(&mut f.ctx, false).unwrap();
        assert!(!t.has("end"));
        let mut t = described("x");
        t.set("status", "completed");
        t.set("end", "1700000000");
        t.validate(&mut f.ctx, false).unwrap();
        assert_eq!(t.get("end"), "1700000000");
    }

    #[test]
    fn validate_defaults() {
        let mut f = fixture();
        Task::set_default_project("home");
        Task::set_default_due("2d");
        Task::set_default_scheduled("2030-01-01");
        f.ctx.config.set("uda.size.type", "string");
        f.ctx.config.set("uda.size.default", "M");
        f.ctx.config.set("uda.empty.default", "");
        f.ctx.config.set("uda.prio.default", "H");

        let before = localtime::time_now();
        let mut t = described("x");
        t.set("prio", "L");
        t.validate(&mut f.ctx, true).unwrap();
        let after = localtime::time_now();
        assert_eq!(t.get("project"), "home");
        let due = t.get_date("due");
        assert!(due >= before + 2 * 86400 && due <= after + 2 * 86400, "{due}");
        assert_eq!(t.get_date("scheduled"), Datetime::from_string("2030-01-01", "").unwrap().to_epoch());
        assert_eq!(t.get("size"), "M");
        assert_eq!(t.get("prio"), "L", "existing value is not overridden");
        assert!(!t.has("empty"), "empty default is skipped");

        // Given values are kept.
        let mut t = described("x");
        t.set("project", "work");
        t.set("due", "1700000000");
        t.set("scheduled", "1600000000");
        t.validate(&mut f.ctx, true).unwrap();
        assert_eq!(t.get("project"), "work");
        assert_eq!(t.get("due"), "1700000000");
        assert_eq!(t.get("scheduled"), "1600000000");

        // No defaults without apply_default, or for a child task.
        let mut t = described("x");
        t.validate(&mut f.ctx, false).unwrap();
        assert!(!t.has("project") && !t.has("due") && !t.has("scheduled") && !t.has("size"));
        let mut t = described("x");
        t.set("parent", "00000000-0000-0000-0000-000000000009");
        t.validate(&mut f.ctx, true).unwrap();
        assert!(!t.has("project") && !t.has("due") && !t.has("size"));

        // An unparseable default.due raises the Datetime error.
        Task::set_default_due("notadate");
        let mut t = described("x");
        assert!(t.validate(&mut f.ctx, true).is_err());
    }

    #[test]
    fn validate_warnings() {
        let mut f = fixture();
        let mut t = Task::new();
        t.set("entry", "1700000000");
        t.set("wait", "1700000300");
        t.set("due", "1700000200");
        t.set("start", "1600000000");
        t.set("scheduled", "1700000400");
        t.validate(&mut f.ctx, false).unwrap();
        assert_eq!(
            f.ctx.footnotes,
            vec![
                "Warning: You have specified that the 'wait' date is after the 'due' date.".to_string(),
                "Warning: You have specified that the 'entry' date is after the 'start' date.".to_string(),
                "Warning: You have specified that the 'scheduled' date is after the 'start' date.".to_string(),
                "Warning: You have specified that the 'scheduled' date is after the 'due' date.".to_string(),
                "Warning: task has no description.".to_string(),
            ]
        );

        // A zero right-hand date is being removed: no warning.
        f.ctx.footnotes.clear();
        let mut t = described("x");
        t.set("wait", "1700000300");
        t.set("due", "0");
        t.validate(&mut f.ctx, false).unwrap();
        assert!(f.ctx.footnotes.is_empty(), "{:?}", f.ctx.footnotes);

        // recur without due is removed.
        let mut t = described("x");
        t.set("recur", "weekly");
        t.validate(&mut f.ctx, false).unwrap();
        assert!(!t.has("recur"));
        assert_eq!(f.ctx.footnotes, vec!["Warning: recurring task has no due date.".to_string()]);

        // An invalid recurrence period is removed.
        f.ctx.footnotes.clear();
        let mut t = described("x");
        t.set("due", "1700000000");
        t.set("recur", "bogus");
        t.validate(&mut f.ctx, false).unwrap();
        assert!(!t.has("recur"));
        assert_eq!(f.ctx.footnotes, vec!["Warning: The recurrence value 'bogus' is not valid.".to_string()]);
    }

    #[test]
    fn tdb2_add_applies_defaults_and_modify_validates() {
        let mut f = fixture();
        Task::set_default_project("home");
        f.ctx.config.set("uda.size.default", "M");
        let t = add(&mut f.ctx, "one");
        assert_eq!(t.id, 1);
        let stored = f.ctx.tdb2.get_by_id(1).unwrap().unwrap();
        assert_eq!(stored.get("project"), "home");
        assert_eq!(stored.get("size"), "M");
        assert_eq!(stored.get("status"), "pending");

        // modify validates without defaults.
        Task::set_default_project("other");
        let mut t = stored;
        t.remove("project");
        t.set("end", "1700000000");
        tdb2::modify(&mut f.ctx, &mut t).unwrap();
        let stored = f.ctx.tdb2.get_by_id(1).unwrap().unwrap();
        assert!(!stored.has("project"));
        assert!(!stored.has("end"));

        // add raises validate errors before touching storage.
        let mut bad = described("x");
        bad.set("status", "zzz");
        assert_eq!(err(tdb2::add(&mut f.ctx, &mut bad)), "The status 'zzz' is not valid.");
        assert_eq!(f.ctx.tdb2.latest_id().unwrap(), 1);
    }

    #[test]
    fn add_remove_dependencies() {
        let mut f = fixture();
        let mut t1 = add(&mut f.ctx, "one");
        let t2 = add(&mut f.ctx, "two");
        let u2 = t2.get("uuid");

        assert_eq!(
            err(t1.add_dependency_id(&mut f.ctx, 7)),
            "Could not create a dependency on task 7 - not found."
        );
        let u1 = t1.get("uuid");
        assert_eq!(err(t1.add_dependency(&mut f.ctx, &u1)), "A task cannot be dependent on itself.");

        t1.recalc_urgency = false;
        t1.add_dependency_id(&mut f.ctx, 2).unwrap();
        assert!(t1.has_dependency(&u2));
        assert_eq!(t1.get("depends"), u2);
        assert!(t1.recalc_urgency);

        t1.add_dependency_id(&mut f.ctx, 2).unwrap();
        assert_eq!(f.ctx.footnotes, vec!["Task 1 already depends on task 2.".to_string()]);
        t1.add_dependency(&mut f.ctx, &u2).unwrap();
        assert_eq!(f.ctx.footnotes[1], format!("Task {u1} already depends on task {u2}."));

        assert_eq!(t1.get_dependency_ids(&mut f.ctx.tdb2).unwrap(), vec![2]);
        tdb2::modify(&mut f.ctx, &mut t1).unwrap();

        // Blocking / blocked lookups.
        let deps = t1.get_dependency_tasks(&mut f.ctx.tdb2).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].get("uuid"), u2);
        let blocked = t2.get_blocked_tasks(&mut f.ctx.tdb2).unwrap();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].get("uuid"), u1);
        assert!(t2.get_dependency_tasks(&mut f.ctx.tdb2).unwrap().is_empty());
        assert!(t1.get_blocked_tasks(&mut f.ctx.tdb2).unwrap().is_empty());

        // Circular dependency: 2 -> 1 while 1 -> 2 is stored.
        let mut t2 = f.ctx.tdb2.get_by_id(2).unwrap().unwrap();
        assert_eq!(
            err(t2.add_dependency_id(&mut f.ctx, 1)),
            "Circular dependency detected and disallowed."
        );
        assert!(dependency_is_circular(&mut f.ctx.tdb2, &t2).unwrap());
        let fresh = Task::new();
        assert!(!dependency_is_circular(&mut f.ctx.tdb2, &fresh).unwrap());

        // Removal.
        assert_eq!(
            err(t1.remove_dependency_id(&mut f.ctx, 9)),
            "Could not delete a dependency on task 9 - not found."
        );
        assert_eq!(
            err(t1.remove_dependency_id(&mut f.ctx, 1)),
            "Could not delete a dependency on task 1 - not found."
        );
        t1.remove_dependency_id(&mut f.ctx, 2).unwrap();
        assert!(!t1.has_dependency(&u2));
        assert!(!t1.has("depends"));
        assert_eq!(
            err(t1.remove_dependency(&u2)),
            format!("Could not delete a dependency on task {u2} - not found.")
        );
    }

    #[test]
    fn circular_through_chain() {
        let mut f = fixture();
        let mut t1 = add(&mut f.ctx, "one");
        let mut t2 = add(&mut f.ctx, "two");
        let mut t3 = add(&mut f.ctx, "three");
        t1.add_dependency_id(&mut f.ctx, 2).unwrap();
        tdb2::modify(&mut f.ctx, &mut t1).unwrap();
        t2.add_dependency_id(&mut f.ctx, 3).unwrap();
        tdb2::modify(&mut f.ctx, &mut t2).unwrap();
        assert_eq!(
            err(t3.add_dependency_id(&mut f.ctx, 1)),
            "Circular dependency detected and disallowed."
        );
    }

    #[test]
    fn chain_on_start_and_complete() {
        let mut f = fixture();
        let mut t1 = add(&mut f.ctx, "one");
        let mut t2 = add(&mut f.ctx, "two");
        let t3 = add(&mut f.ctx, "three");
        let u3 = t3.get("uuid");
        t1.add_dependency_id(&mut f.ctx, 2).unwrap();
        tdb2::modify(&mut f.ctx, &mut t1).unwrap();
        t2.add_dependency_id(&mut f.ctx, 3).unwrap();
        tdb2::modify(&mut f.ctx, &mut t2).unwrap();

        // Without dependency.reminder nothing is printed.
        let mut out = Vec::new();
        dependency_chain_on_start_to(&mut f.ctx, &t2, &mut out).unwrap();
        assert!(out.is_empty());

        f.ctx.config.set("dependency.reminder", "on");
        dependency_chain_on_start_to(&mut f.ctx, &t2, &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "Task 2 is blocked by:\n  3 three\n");

        // Completing 2 breaks the chain 1 -> 2 -> 3; repaired to 1 -> 3.
        f.ctx.config.set("dependency.confirmation", "off");
        let mut out = Vec::new();
        dependency_chain_on_complete_to(&mut f.ctx, &t2, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Task 2 is blocked by:\n  3 three\nand is blocking:\n  1 one\n"
        );
        let t1 = f.ctx.tdb2.get_by_id(1).unwrap().unwrap();
        assert_eq!(t1.get_dependency_uuids(), vec![u3.clone()]);
        assert_eq!(t1.get("depends"), u3);

        // The tail of a chain prints nothing.
        let mut out = Vec::new();
        dependency_chain_on_complete_to(&mut f.ctx, &t3, &mut out).unwrap();
        assert!(out.is_empty());
    }
}

/// Tests for urgency (the coefficient polynomial, the user coefficients and
/// `urgency.inherit`) and the later-milestone stubs.
#[cfg(test)]
mod urgency_tests {
    use super::*;
    use crate::tdb2;
    use crate::test_util::env_lock;
    use tempfile::TempDir;

    const DAY: i64 = 86400;

    /// Resets the urgency coefficient thread-local statics when dropped, so
    /// later assertions in the same thread see the defaults.
    struct CoefficientsGuard;

    impl Drop for CoefficientsGuard {
        fn drop(&mut self) {
            set_all_coefficients(0.0);
            Task::set_urgency_age_max(0.0);
            Task::set_coefficients(BTreeMap::new());
        }
    }

    fn set_all_coefficients(value: f32) {
        Task::set_urgency_project_coefficient(value);
        Task::set_urgency_active_coefficient(value);
        Task::set_urgency_scheduled_coefficient(value);
        Task::set_urgency_waiting_coefficient(value);
        Task::set_urgency_blocked_coefficient(value);
        Task::set_urgency_annotations_coefficient(value);
        Task::set_urgency_tags_coefficient(value);
        Task::set_urgency_due_coefficient(value);
        Task::set_urgency_blocking_coefficient(value);
        Task::set_urgency_age_coefficient(value);
    }

    fn coefficients(pairs: &[(&str, f32)]) {
        Task::set_coefficients(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect());
    }

    /// A Context whose replica lives in a temporary data directory. Fields
    /// drop in declaration order: the TempDir is removed before the lock is
    /// released.
    struct Fixture {
        ctx: Context,
        _dir: TempDir,
        _coefficients: CoefficientsGuard,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    fn fixture() -> Fixture {
        // Context::new reads $HOME; serialize with env-mutating tests.
        let lock = env_lock();
        let dir = TempDir::new().unwrap();
        let mut ctx = Context::new();
        ctx.tdb2.open_replica_at(dir.path().to_str().unwrap(), true, true).unwrap();
        Fixture { ctx, _dir: dir, _coefficients: CoefficientsGuard, _lock: lock }
    }

    fn user_terms(t: &Task) -> f32 {
        t.urgency_user_terms(0.0, &mut NullEnv).unwrap()
    }

    #[test]
    fn epsilon_and_max() {
        assert_eq!(EPSILON, 0.000001_f64 as f32);
        assert_eq!(cpp_max(1.0, 2.0), 2.0);
        assert_eq!(cpp_max(2.0, 1.0), 2.0);
        // std::max keeps the first argument when the comparison is false.
        assert!(cpp_max(f32::NAN, 1.0).is_nan());
        assert_eq!(cpp_max(1.0, f32::NAN), 1.0);
        assert_eq!(cpp_substr("abcdef", 2, 2), "cd");
        assert_eq!(cpp_substr("abcdef", 2, usize::MAX), "cdef");
        assert_eq!(cpp_substr("abcdef", 6, 3), "");
        assert!(cpp_substr_is("abcdef", 2, "cd"));
        assert!(!cpp_substr_is("abcdef", 4, "efg"));
        assert!(cpp_substr_is("abcdef", 6, ""));
    }

    #[test]
    fn term_project_active_blocked_blocking() {
        let mut t = Task::new();
        assert_eq!(t.urgency_project(), 0.0);
        assert_eq!(t.urgency_active(), 0.0);
        assert_eq!(t.urgency_blocked(), 0.0);
        assert_eq!(t.urgency_blocking(), 0.0);
        t.set("project", "home");
        t.set("start", "1700000000");
        t.is_blocked = true;
        t.is_blocking = true;
        assert_eq!(t.urgency_project(), 1.0);
        assert_eq!(t.urgency_active(), 1.0);
        assert_eq!(t.urgency_blocked(), 1.0);
        assert_eq!(t.urgency_blocking(), 1.0);
        // An empty value still counts as present (C++ `has`).
        let mut t = Task::new();
        t.set("project", "");
        assert_eq!(t.urgency_project(), 1.0);
    }

    #[test]
    fn term_scheduled() {
        let now = 1_700_000_000;
        let mut t = Task::new();
        assert_eq!(t.urgency_scheduled_at(now), 0.0);
        t.set_int("scheduled", now - 1);
        assert_eq!(t.urgency_scheduled_at(now), 1.0);
        t.set_int("scheduled", now);
        assert_eq!(t.urgency_scheduled_at(now), 0.0);
        t.set_int("scheduled", now + 1);
        assert_eq!(t.urgency_scheduled_at(now), 0.0);
        // Against the real clock.
        t.set_int("scheduled", localtime::time_now() - DAY);
        assert_eq!(t.urgency_scheduled(), 1.0);
        t.set_int("scheduled", localtime::time_now() + DAY);
        assert_eq!(t.urgency_scheduled(), 0.0);
    }

    #[test]
    fn term_waiting() {
        let mut t = Task::new();
        t.set("status", "pending");
        assert_eq!(t.urgency_waiting(), 0.0);
        t.set_int("wait", localtime::time_now() + DAY);
        assert_eq!(t.urgency_waiting(), 1.0);
        t.set_int("wait", localtime::time_now() - DAY);
        assert_eq!(t.urgency_waiting(), 0.0);
        t.set_int("wait", localtime::time_now() + DAY);
        t.set("status", "completed");
        assert_eq!(t.urgency_waiting(), 0.0);
    }

    #[test]
    fn term_annotations() {
        let mut t = Task::new();
        assert_eq!(t.urgency_annotations(), 0.0);
        t.set("annotation_1700000001", "a");
        assert_eq!(t.urgency_annotations(), 0.8_f32);
        t.set("annotation_1700000002", "b");
        assert_eq!(t.urgency_annotations(), 0.9_f32);
        t.set("annotation_1700000003", "c");
        assert_eq!(t.urgency_annotations(), 1.0);
        t.set("annotation_1700000004", "d");
        assert_eq!(t.urgency_annotations(), 1.0);
        // Driven by annotation_count, not by the attributes.
        t.annotation_count = 0;
        assert_eq!(t.urgency_annotations(), 0.0);
    }

    #[test]
    fn term_tags() {
        let mut t = Task::new();
        assert_eq!(t.urgency_tags(), 0.0);
        t.add_tag("a");
        assert_eq!(t.urgency_tags(), 0.8_f32);
        t.add_tag("b");
        assert_eq!(t.urgency_tags(), 0.9_f32);
        t.add_tag("c");
        assert_eq!(t.urgency_tags(), 1.0);
        t.add_tag("d");
        assert_eq!(t.urgency_tags(), 1.0);
    }

    #[test]
    fn term_due() {
        let now = Datetime::from_epoch(1_700_000_000);
        let n = now.to_epoch();
        let mut t = Task::new();
        assert_eq!(t.urgency_due_at(&now), 0.0);

        let due = |t: &mut Task, at: i64| {
            t.set_int("due", at);
            t.urgency_due_at(&now)
        };
        // >= 7 days overdue: capped at 1.0.
        assert_eq!(due(&mut t, n - 7 * DAY), 1.0);
        assert_eq!(due(&mut t, n - 30 * DAY), 1.0);
        // Due now: (14 * 0.8 / 21) + 0.2 = 0.7333...
        assert_eq!(due(&mut t, n), ((14.0_f64 * 0.8 / 21.0) + 0.2) as f32);
        assert!((due(&mut t, n) - 0.733_333_3).abs() < 1e-6);
        // 3.5 days overdue: (17.5 * 0.8 / 21) + 0.2 = 0.8666...
        assert_eq!(due(&mut t, n - 302_400), 0.866_666_7_f32);
        // Exactly 14 days in the future is still on the linear part.
        assert_eq!(due(&mut t, n + 14 * DAY), ((0.0_f64 * 0.8 / 21.0) + 0.2) as f32);
        // Beyond two weeks: capped at 0.2.
        assert_eq!(due(&mut t, n + 14 * DAY + 1), 0.2_f32);
        assert_eq!(due(&mut t, n + 100 * DAY), 0.2_f32);
        // One second overdue: days_overdue is narrowed to float first.
        let days = (1.0_f64 / 86400.0) as f32;
        assert_eq!(due(&mut t, n - 1), (((days as f64 + 14.0) * 0.8 / 21.0) + 0.2) as f32);
        // Just under 7 days stays linear.
        let days = ((7 * DAY - 1) as f64 / 86400.0) as f32;
        assert_eq!(due(&mut t, n - 7 * DAY + 1), (((days as f64 + 14.0) * 0.8 / 21.0) + 0.2) as f32);

        // Against the real clock.
        t.set_int("due", localtime::time_now() - 30 * DAY);
        assert_eq!(t.urgency_due(), 1.0);
        t.set_int("due", localtime::time_now() + 30 * DAY);
        assert_eq!(t.urgency_due(), 0.2_f32);
    }

    #[test]
    fn term_age() {
        let _guard = CoefficientsGuard;
        let now = Datetime::from_epoch(1_700_000_000);
        let n = now.to_epoch();
        let mut t = Task::new();
        // No entry: 1.0.
        assert_eq!(t.urgency_age_at(&now), 1.0);

        // urgencyAgeMax == 0: 1.0.
        t.set_int("entry", n - 10 * DAY - 5);
        assert_eq!(t.urgency_age_at(&now), 1.0);

        Task::set_urgency_age_max(365.0);
        // 10 days (integer division) / 365.
        assert_eq!(t.urgency_age_at(&now), 0.027_397_26_f32);
        assert_eq!(t.urgency_age_at(&now), (1.0 * 10.0_f64 / 365.0) as f32);
        // Less than a day old: 0.
        t.set_int("entry", n - DAY + 1);
        assert_eq!(t.urgency_age_at(&now), 0.0);
        // Exactly the maximum is not "older than".
        t.set_int("entry", n - 365 * DAY);
        assert_eq!(t.urgency_age_at(&now), 1.0);
        t.set_int("entry", n - 366 * DAY);
        assert_eq!(t.urgency_age_at(&now), 1.0);
        // In the future: time_t division truncates toward zero.
        t.set_int("entry", n + DAY / 2);
        assert_eq!(t.urgency_age_at(&now), 0.0);
        t.set_int("entry", n + DAY + DAY / 2);
        assert_eq!(t.urgency_age_at(&now), (-1.0_f64 / 365.0) as f32);

        // A fractional maximum compares in float.
        Task::set_urgency_age_max(9.5);
        t.set_int("entry", n - 9 * DAY);
        assert_eq!(t.urgency_age_at(&now), (9.0_f64 / 9.5) as f32);
        t.set_int("entry", n - 10 * DAY);
        assert_eq!(t.urgency_age_at(&now), 1.0);

        // Against the real clock.
        Task::set_urgency_age_max(365.0);
        t.set_int("entry", localtime::time_now() - 400 * DAY);
        assert_eq!(t.urgency_age(), 1.0);
    }

    #[test]
    fn user_tag_coefficients() {
        let _guard = CoefficientsGuard;
        coefficients(&[
            ("urgency.user.tag.next.coefficient", 15.0),
            ("urgency.user.tag.later.coefficient", -2.5),
            ("urgency.user.tag.ACTIVE.coefficient", 0.5),
            ("urgency.user.tag.tiny.coefficient", 0.000_000_1),
            ("urgency.user.tag.weight.next", 100.0),
        ]);
        let mut t = Task::new();
        assert_eq!(user_terms(&t), 0.0);
        t.add_tag("next");
        assert_eq!(user_terms(&t), 15.0);
        t.add_tag("later");
        assert_eq!(user_terms(&t), 12.5);
        // Virtual tags are matched through hasTag.
        t.set("start", "1700000000");
        assert_eq!(user_terms(&t), 13.0);
        // Coefficients within epsilon of zero are ignored.
        t.add_tag("tiny");
        assert_eq!(user_terms(&t), 13.0);
        // Keys without ".coefficient" are ignored.
        t.add_tag("weight");
        assert_eq!(user_terms(&t), 13.0);

        // The first ".coefficient" ends the name.
        coefficients(&[("urgency.user.tag.a.coefficient.coefficient", 1.0)]);
        let mut t = Task::new();
        t.add_tag("a");
        assert_eq!(user_terms(&t), 1.0);
    }

    #[test]
    fn user_project_coefficients() {
        let _guard = CoefficientsGuard;
        coefficients(&[("urgency.user.project.home.coefficient", 5.0)]);
        let mut t = Task::new();
        assert_eq!(user_terms(&t), 0.0);
        for (project, expected) in
            [("home", 5.0), ("home.garden", 5.0), ("home.garden.shed", 5.0), ("homework", 0.0), ("work.home", 0.0), ("Home", 0.0)]
        {
            t.set("project", project);
            assert_eq!(user_terms(&t), expected, "project {project}");
        }

        // A sub-project coefficient matches only that branch; both apply.
        coefficients(&[
            ("urgency.user.project.home.coefficient", 5.0),
            ("urgency.user.project.home.garden.coefficient", 1.5),
        ]);
        t.set("project", "home.garden");
        assert_eq!(user_terms(&t), 6.5);
        t.set("project", "home.kitchen");
        assert_eq!(user_terms(&t), 5.0);

        // C++ `end - 21` wraps for "urgency.user.project.coefficient", making
        // the project name "coefficient".
        coefficients(&[("urgency.user.project.coefficient", 3.0)]);
        t.set("project", "coefficient");
        assert_eq!(user_terms(&t), 3.0);
        t.set("project", "home");
        assert_eq!(user_terms(&t), 0.0);
    }

    #[test]
    fn user_keyword_coefficients() {
        let _guard = CoefficientsGuard;
        coefficients(&[
            ("urgency.user.keyword.urgent.coefficient", 4.0),
            ("urgency.user.keyword.call.coefficient", 1.25),
        ]);
        let mut t = Task::new();
        assert_eq!(user_terms(&t), 0.0);
        t.set("description", "something urgent");
        assert_eq!(user_terms(&t), 4.0);
        t.set("description", "call the plumber, urgently");
        assert_eq!(user_terms(&t), 5.25);
        // Case-sensitive substring match.
        t.set("description", "Urgent");
        assert_eq!(user_terms(&t), 0.0);

        // An empty keyword matches every description (C++ `find ("")`).
        coefficients(&[("urgency.user.keyword..coefficient", 2.0)]);
        let t = Task::new();
        assert_eq!(user_terms(&t), 2.0);
    }

    #[test]
    fn uda_coefficients() {
        let _guard = CoefficientsGuard;
        coefficients(&[
            ("urgency.uda.estimate.coefficient", 1.5),
            ("urgency.uda.priority.H.coefficient", 6.0),
            ("urgency.uda.priority.M.coefficient", 3.9),
            ("urgency.uda.priority.L.coefficient", 1.8),
        ]);
        let mut t = Task::new();
        assert_eq!(user_terms(&t), 0.0);
        t.set("estimate", "");
        assert_eq!(user_terms(&t), 1.5);
        t.remove("estimate");
        t.set("priority", "H");
        assert_eq!(user_terms(&t), 6.0);
        t.set("priority", "M");
        assert_eq!(user_terms(&t), 3.9_f32);
        t.set("priority", "L");
        assert_eq!(user_terms(&t), 1.8_f32);
        t.set("priority", "h");
        assert_eq!(user_terms(&t), 0.0);
        t.set("estimate", "3");
        t.set("priority", "H");
        assert_eq!(user_terms(&t), 1.5_f32 + 6.0_f32);

        // A dotted value: only the first dot separates name and value.
        coefficients(&[("urgency.uda.size.x.l.coefficient", 2.0)]);
        let mut t = Task::new();
        t.set("size", "x.l");
        assert_eq!(user_terms(&t), 2.0);

        // C++ `end - 12` wraps for "urgency.uda.coefficient": the attribute
        // "coefficient" must be present.
        coefficients(&[("urgency.uda.coefficient", 1.0)]);
        let mut t = Task::new();
        assert_eq!(user_terms(&t), 0.0);
        t.set("coefficient", "x");
        assert_eq!(user_terms(&t), 1.0);

        // Non-ASCII keys compare bytewise (no slicing panic): the multi-byte
        // character straddles byte 21 / 17 of the prefix checks.
        coefficients(&[
            ("urgency.user.project\u{e9}.coefficient", 9.0),
            ("urgency.user.tag\u{e9}.coefficient", 9.0),
            ("urgency.user.keyword\u{e9}.coefficient", 9.0),
            ("urgency.user.tag.\u{e9}t\u{e9}.coefficient", 2.0),
            ("urgency.uda.\u{e9}.\u{e9}.coefficient", 3.0),
        ]);
        let mut t = Task::new();
        t.add_tag("\u{e9}t\u{e9}");
        t.set("\u{e9}", "\u{e9}");
        assert_eq!(user_terms(&t), 5.0);

        // Neither "urgency.user." nor "urgency.uda.": ignored.
        coefficients(&[("urgency.due.coefficient", 9.0), ("urgency.uda", 9.0)]);
        let mut t = Task::new();
        t.set("due", "1");
        assert_eq!(user_terms(&t), 0.0);
    }

    #[test]
    fn user_terms_skip_nan_coefficients() {
        let _guard = CoefficientsGuard;
        // C++ `fabs (NaN) > epsilon` is false, so a NaN coefficient (from
        // `strtod ("nan")`) never contributes.
        coefficients(&[
            ("urgency.user.tag.x.coefficient", f32::NAN),
            ("urgency.user.project.p.coefficient", f32::NAN),
            ("urgency.user.keyword.k.coefficient", f32::NAN),
            ("urgency.uda.u.coefficient", f32::NAN),
            ("urgency.uda.v.coefficient", 2.0),
        ]);
        let mut t = Task::new();
        t.add_tag("x");
        t.set("project", "p");
        t.set("description", "k");
        t.set("u", "1");
        t.set("v", "1");
        assert_eq!(user_terms(&t), 2.0);
    }

    #[test]
    fn user_terms_add_in_key_order() {
        let _guard = CoefficientsGuard;
        // Sorted: urgency.uda.* < urgency.user.keyword.* < urgency.user.project.* < urgency.user.tag.*
        coefficients(&[
            ("urgency.user.tag.x.coefficient", 0.1),
            ("urgency.user.project.p.coefficient", 0.2),
            ("urgency.user.keyword.k.coefficient", 0.3),
            ("urgency.uda.u.coefficient", 1e8),
        ]);
        let mut t = Task::new();
        t.add_tag("x");
        t.set("project", "p");
        t.set("description", "k");
        t.set("u", "1");
        let mut expected: f32 = 0.0;
        for c in [1e8_f32, 0.3, 0.2, 0.1] {
            expected += c;
        }
        assert_eq!(user_terms(&t), expected);
        // Starting value is carried through.
        assert_eq!(t.urgency_user_terms(1.0, &mut NullEnv).unwrap(), {
            let mut v: f32 = 1.0;
            for c in [1e8_f32, 0.3, 0.2, 0.1] {
                v += c;
            }
            v
        });
    }

    #[test]
    fn urgency_c_terms_and_coefficients() {
        let mut f = fixture();
        set_all_coefficients(0.0);

        let mut t = Task::new();
        t.set("description", "x");
        assert_eq!(t.urgency_c(&mut f.ctx).unwrap(), 0.0);

        // Each term only contributes its own coefficient.
        type Setter = fn(f32);
        let cases: [(Setter, &str, &str, f32); 6] = [
            (Task::set_urgency_project_coefficient, "project", "p", 1.0),
            (Task::set_urgency_active_coefficient, "start", "1700000000", 4.0),
            (Task::set_urgency_scheduled_coefficient, "scheduled", "1", 5.0),
            (Task::set_urgency_due_coefficient, "due", "1", 12.0),
            (Task::set_urgency_tags_coefficient, "tag_a", "", 1.0 * 0.8_f32),
            (Task::set_urgency_annotations_coefficient, "annotation_1", "a", 1.0 * 0.8_f32),
        ];
        for (setter, attribute, value, expected) in cases {
            let mut t = Task::new();
            t.set(attribute, value);
            set_all_coefficients(0.0);
            // Without its coefficient the term does not count.
            assert_eq!(t.urgency_c(&mut f.ctx).unwrap(), 0.0, "{attribute}");
            setter(if attribute.starts_with("tag") || attribute.starts_with("annotation") {
                1.0
            } else {
                expected
            });
            assert_eq!(t.urgency_c(&mut f.ctx).unwrap(), expected, "{attribute}");
        }

        set_all_coefficients(0.0);
        let mut t = Task::new();
        t.is_blocked = true;
        Task::set_urgency_blocked_coefficient(-5.0);
        assert_eq!(t.urgency_c(&mut f.ctx).unwrap(), -5.0);
        t.is_blocked = false;
        t.is_blocking = true;
        Task::set_urgency_blocking_coefficient(8.0);
        // urgency.inherit defaults to off.
        assert_eq!(t.urgency_c(&mut f.ctx).unwrap(), 8.0);

        set_all_coefficients(0.0);
        let mut t = Task::new();
        t.set("status", "pending");
        t.set_int("wait", localtime::time_now() + DAY);
        Task::set_urgency_waiting_coefficient(-3.0);
        assert_eq!(t.urgency_c(&mut f.ctx).unwrap(), -3.0);

        set_all_coefficients(0.0);
        let t = Task::new();
        Task::set_urgency_age_coefficient(2.0);
        // No entry: age term 1.0.
        assert_eq!(t.urgency_c(&mut f.ctx).unwrap(), 2.0);
    }

    #[test]
    fn full_urgency_hand_computed() {
        let mut f = fixture();
        // The default rc coefficients, as Context::staticInitialization sets them.
        Task::set_urgency_project_coefficient(1.0);
        Task::set_urgency_active_coefficient(4.0);
        Task::set_urgency_scheduled_coefficient(5.0);
        Task::set_urgency_waiting_coefficient(-3.0);
        Task::set_urgency_blocked_coefficient(-5.0);
        Task::set_urgency_annotations_coefficient(1.0);
        Task::set_urgency_tags_coefficient(1.0);
        Task::set_urgency_due_coefficient(12.0);
        Task::set_urgency_blocking_coefficient(8.0);
        Task::set_urgency_age_coefficient(2.0);
        Task::set_urgency_age_max(365.0);
        coefficients(&[
            ("urgency.user.tag.next.coefficient", 15.0),
            ("urgency.uda.priority.H.coefficient", 6.0),
            ("urgency.uda.priority.M.coefficient", 3.9),
            ("urgency.uda.priority.L.coefficient", 1.8),
            ("urgency.user.project.home.coefficient", 2.5),
            ("urgency.user.keyword.fix.coefficient", 0.7),
        ]);

        let mut t = Task::new();
        t.set("description", "fix the fence");
        t.set("status", "pending");
        t.set("project", "home.garden");
        t.set("start", "1700000000");
        t.set("scheduled", "1");
        t.set("annotation_1700000001", "a");
        t.set("annotation_1700000002", "b");
        t.add_tag("a");
        t.add_tag("b");
        t.add_tag("c");
        t.set_int("due", localtime::time_now() - 30 * DAY);
        t.set("priority", "H");

        // project 1 + active 4 + scheduled 5 + annotations 0.9 + tags 1 +
        // due 12 + age (no entry) 2 + uda 6 + keyword 0.7 + project 2.5,
        // summed in float: 35.099998 (0x420c6666).
        let u = t.urgency(&mut f.ctx).unwrap();
        assert_eq!(u.to_bits(), 0x420c_6666);
        assert!(!t.recalc_urgency);
        assert_eq!(t.urgency_value, u);

        // Cached until something changes.
        Task::set_urgency_project_coefficient(100.0);
        assert_eq!(t.urgency(&mut f.ctx).unwrap(), u);
        t.recalc_urgency = true;
        // Project term first: 100 + 4 + 5 + ... = 134.09999 (0x43061999).
        assert_eq!(t.urgency(&mut f.ctx).unwrap().to_bits(), 0x4306_1999);
        Task::set_urgency_project_coefficient(1.0);
        // Setting an attribute flags a recalculation (next tag: +15).
        t.add_tag("next");
        assert!(t.recalc_urgency);
        // ... + 15 (tag next, after the uda/keyword/project terms) = 50.1
        // (0x42486666).
        assert_eq!(t.urgency(&mut f.ctx).unwrap().to_bits(), 0x4248_6666);

        // A minimal task: 0 terms except age (no entry) = 2.0; with an entry
        // 73 days old: 2.0 * (73 / 365) = 0.4.
        let mut t = Task::new();
        t.set("description", "x");
        assert_eq!(t.urgency(&mut f.ctx).unwrap(), 2.0);
        let mut t = Task::new();
        t.set_int("entry", localtime::time_now() - 73 * DAY - 60);
        assert_eq!(t.urgency(&mut f.ctx).unwrap(), (73.0_f64 / 365.0) as f32 * 2.0);
    }

    fn pending(ctx: &mut Context, uuid: &str) -> Task {
        ctx.tdb2.pending_tasks().unwrap().iter().find(|t| t.get_ref("uuid") == uuid).unwrap().clone()
    }

    #[test]
    fn inherit_through_blocked_chain() {
        let mut f = fixture();
        set_all_coefficients(0.0);
        Task::set_urgency_blocked_coefficient(-5.0);
        Task::set_urgency_blocking_coefficient(8.0);
        coefficients(&[("urgency.user.tag.hot.coefficient", 20.0)]);

        // c depends on b depends on a: a blocks b, b blocks c.
        let mut a = Task::new();
        a.set("description", "a");
        tdb2::add(&mut f.ctx, &mut a).unwrap();
        let mut b = Task::new();
        b.set("description", "b");
        tdb2::add(&mut f.ctx, &mut b).unwrap();
        let mut c = Task::new();
        c.set("description", "c");
        c.add_tag("hot");
        tdb2::add(&mut f.ctx, &mut c).unwrap();
        b.add_dependency(&mut f.ctx, &a.get("uuid")).unwrap();
        tdb2::modify(&mut f.ctx, &mut b).unwrap();
        c.add_dependency(&mut f.ctx, &b.get("uuid")).unwrap();
        tdb2::modify(&mut f.ctx, &mut c).unwrap();

        let mut a = pending(&mut f.ctx, &a.get("uuid"));
        let mut b = pending(&mut f.ctx, &b.get("uuid"));
        let mut c = pending(&mut f.ctx, &c.get("uuid"));
        assert!(a.is_blocking && !a.is_blocked);
        assert!(b.is_blocking && b.is_blocked);
        assert!(!c.is_blocking && c.is_blocked);

        // Without inheritance: c = -5 + 20, b = -5 + 8, a = 8.
        assert_eq!(c.clone().urgency(&mut f.ctx).unwrap(), 15.0);
        assert_eq!(b.clone().urgency(&mut f.ctx).unwrap(), 3.0);
        assert_eq!(a.clone().urgency(&mut f.ctx).unwrap(), 8.0);
        // urgency_inherit itself does not depend on rc.urgency.inherit.
        assert_eq!(b.urgency_inherit(&mut f.ctx).unwrap(), 15.0);
        assert_eq!(a.urgency_inherit(&mut f.ctx).unwrap(), 3.0);
        // Nothing blocked: -FLT_MAX.
        assert_eq!(c.urgency_inherit(&mut f.ctx).unwrap(), -f32::MAX);

        f.ctx.config.set("urgency.inherit", "1");
        // c blocks nothing: unchanged.
        assert_eq!(c.urgency(&mut f.ctx).unwrap(), 15.0);
        // b: max (3, 15) + 0.01 = 15.01 (0x417028f6).
        let ub = b.urgency(&mut f.ctx).unwrap();
        assert_eq!(ub, (15.0_f64 + 0.01) as f32);
        assert_eq!(ub.to_bits(), 0x4170_28f6);
        // a: inherits b's (recomputed) urgency: max (8, 15.01) + 0.01
        // = 15.02 (0x417051ec).
        let ua = a.urgency(&mut f.ctx).unwrap();
        assert_eq!(ua, (ub as f64 + 0.01) as f32);
        assert_eq!(ua.to_bits(), 0x4170_51ec);

        // When the task's own urgency is higher it keeps it, plus 0.01.
        coefficients(&[]);
        let mut a = pending(&mut f.ctx, &a.get("uuid"));
        let mut b = pending(&mut f.ctx, &b.get("uuid"));
        // c = -5; b = max (3, -5) + 0.01; a = max (8, b) + 0.01.
        assert_eq!(b.urgency(&mut f.ctx).unwrap(), (3.0_f64 + 0.01) as f32);
        assert_eq!(a.urgency(&mut f.ctx).unwrap(), (8.0_f64 + 0.01) as f32);

        // Inheritance needs is_blocking: a task not flagged as blocking is
        // not looked up.
        let mut a = pending(&mut f.ctx, &a.get("uuid"));
        a.is_blocking = false;
        Task::set_urgency_blocking_coefficient(0.0);
        assert_eq!(a.urgency(&mut f.ctx).unwrap(), 0.0);

        // A completed blocked task no longer contributes.
        coefficients(&[("urgency.user.tag.hot.coefficient", 20.0)]);
        Task::set_urgency_blocking_coefficient(8.0);
        let mut c = pending(&mut f.ctx, &c.get("uuid"));
        c.set_status(Status::Deleted);
        tdb2::modify(&mut f.ctx, &mut c).unwrap();
        let mut b = pending(&mut f.ctx, &b.get("uuid"));
        assert!(!b.is_blocking && b.is_blocked);
        // b is still blocked by a but no longer blocking: -5 only.
        assert_eq!(b.urgency(&mut f.ctx).unwrap(), -5.0);
    }

    #[test]
    fn later_milestone_stubs() {
        let t = Task::new();
        assert!(matches!(t.diff(&Task::new()), Err(TwError::NotImplemented(ref s)) if s == "Task::diff"));
    }
}

#[cfg(test)]
mod json_tests {
    use super::*;
    use crate::test_util::env_lock;
    use tempfile::TempDir;

    const UUID_A: &str = "92a40a34-37f3-4785-8ca1-ff89cfbfd105";
    const UUID_B: &str = "e08e35fa-e42b-4de0-acc4-518fca8f6365";

    /// Restores the `Task::attributes` and urgency thread-local statics when
    /// dropped.
    struct StaticsGuard;

    impl Drop for StaticsGuard {
        fn drop(&mut self) {
            Task::set_attributes(BTreeMap::new());
            Task::set_urgency_tags_coefficient(0.0);
        }
    }

    /// A Context whose replica lives in a temporary data directory. Fields
    /// drop in declaration order: the TempDir is removed and the statics are
    /// restored before the lock is released.
    struct Fixture {
        ctx: Context,
        _dir: TempDir,
        _statics: StaticsGuard,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    fn fixture(attributes: &[(&str, &str)]) -> Fixture {
        // Context::new reads $HOME; serialize with env-mutating tests.
        let lock = env_lock();
        let dir = TempDir::new().unwrap();
        let mut ctx = Context::new();
        ctx.tdb2.open_replica_at(dir.path().to_str().unwrap(), true, true).unwrap();
        Task::set_attributes(attributes.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        Fixture { ctx, _dir: dir, _statics: StaticsGuard, _lock: lock }
    }

    /// The `t_test.cpp` / `tw_2689_test.cpp` attribute types.
    const COLUMNS: &[(&str, &str)] = &[
        ("description", "string"),
        ("entry", "date"),
        ("tags", "string"),
        ("uuid", "string"),
        ("depends", "string"),
    ];

    fn err<T: std::fmt::Debug>(r: Result<T>) -> String {
        r.unwrap_err().to_string()
    }

    #[test]
    fn t_test_parse_and_tags() {
        let mut f = fixture(COLUMNS);
        assert!(Task::from_input(&mut f.ctx, "{}").is_ok());
        let t5 = Task::from_input(
            &mut f.ctx,
            r#"{"uuid":"00000000-0000-0000-000000000001","description":"foo","entry":"1234567890"}"#,
        )
        .unwrap();
        assert_eq!(t5.get("entry"), "1234567890");
        assert_eq!(t5.get("description"), "foo");

        let mut t6 = Task::new();
        t6.set("entry", "20130602T224000Z");
        t6.set("description", "DESC");
        t6.add_tag("tag1");
        assert_eq!(
            t6.compose_json(&mut f.ctx, false).unwrap(),
            r#"{"description":"DESC","entry":"20130602T224000Z","tags":["tag1"]}"#
        );
        t6.add_tag("tag2");
        assert_eq!(
            t6.compose_json_plain().unwrap(),
            r#"{"description":"DESC","entry":"20130602T224000Z","tags":["tag1","tag2"]}"#
        );

        let line = r#"{"description":"DESC","entry":"20130602T224000Z","tags":["tag1","tag2"]}"#;
        let t7 = Task::from_input(&mut f.ctx, line).unwrap();
        assert_eq!(t7.get("entry"), "1370212800");
        assert_eq!(t7.get("tags"), "tag1,tag2");
        assert_eq!(t7.compose_json_plain().unwrap(), line);
    }

    #[test]
    fn tw_2689_depends_forms() {
        let mut f = fixture(COLUMNS);
        let samples = [
            format!(
                r#"{{"depends":"[\"{UUID_A}\",\"{UUID_B}\"]","uuid":"00000000-0000-0000-0000-000000000000"}}"#
            ),
            format!(r#"{{"depends":"{UUID_A},{UUID_B}","uuid":"00000000-0000-0000-0000-000000000000"}}"#),
            format!(r#"{{"depends":["{UUID_A}","{UUID_B}"],"uuid":"00000000-0000-0000-0000-000000000000"}}"#),
        ];
        for sample in &samples {
            let t = Task::from_input(&mut f.ctx, sample).unwrap();
            assert_eq!(t.get("uuid"), "00000000-0000-0000-0000-000000000000", "{sample}");
            assert_eq!(t.get("depends"), format!("{UUID_A},{UUID_B}"), "{sample}");
            assert!(t.has(&format!("dep_{UUID_A}")));
            assert!(t.has(&format!("dep_{UUID_B}")));
            assert_eq!(
                t.compose_json_plain().unwrap(),
                format!(r#"{{"uuid":"00000000-0000-0000-0000-000000000000","depends":["{UUID_A}","{UUID_B}"]}}"#)
            );
        }
    }

    #[test]
    fn untyped_legacy_strings_are_split_by_parse() {
        // With no column types, `tags`/`depends` are orphans stored verbatim;
        // `parse` then mirrors them into `tag_..`/`dep_..`.
        let mut f = fixture(&[]);
        let line = format!(r#"{{"depends":"{UUID_B},{UUID_A}","tags":"b,a"}}"#);
        let t = Task::from_input(&mut f.ctx, &line).unwrap();
        assert_eq!(t.get("tags"), "a,b");
        assert!(t.has("tag_a") && t.has("tag_b"));
        assert_eq!(t.get("depends"), format!("{UUID_A},{UUID_B}"));

        // The object constructor does not apply the parse fix-ups.
        let root = json::parse(&line).unwrap();
        let t = Task::from_json_object(&mut f.ctx, &root).unwrap();
        assert_eq!(t.get("tags"), "b,a");
        assert!(!t.has("tag_a"));
        assert_eq!(t.get("depends"), format!("{UUID_B},{UUID_A}"));
        assert!(t.get_dependency_uuids().is_empty());
    }

    #[test]
    fn id_urgency_skipped_only_when_typed() {
        let mut f = fixture(&[("id", "numeric"), ("urgency", "numeric"), ("description", "string")]);
        let t = Task::from_input(&mut f.ctx, r#"{"id":4,"urgency":3.5,"description":"x"}"#).unwrap();
        assert!(!t.has("id"));
        assert!(!t.has("urgency"));
        assert_eq!(t.get("description"), "x");

        Task::set_attributes(BTreeMap::new());
        f.ctx.debug_messages.clear();
        let t = Task::from_input(&mut f.ctx, r#"{"id":4,"urgency":3.5}"#).unwrap();
        assert_eq!(t.get("id"), "4.000000");
        assert_eq!(t.get("urgency"), "3.500000");
        assert_eq!(f.ctx.debug_messages.len(), 2);
        for (message, name) in f.ctx.debug_messages.iter().zip(["id", "urgency"]) {
            assert!(message.starts_with(&format!("Task::parseJSON found orphan '{name}' with value '0x")), "{message}");
            assert!(message.ends_with("' --> preserved\n"), "{message}");
        }
    }

    #[test]
    fn strings_decoded_and_encoded() {
        let mut f = fixture(&[("description", "string"), ("recur", "duration")]);
        let line = r#"{"description":"say \"hi\"\\now","orphan":"a\tb","recur":"1d"}"#;
        let t = Task::from_input(&mut f.ctx, line).unwrap();
        assert_eq!(t.get("description"), "say \"hi\"\\now");
        assert_eq!(t.get("orphan"), "a\tb");
        assert_eq!(t.get("recur"), "1d");
        // Round trip: string (and untyped) values are re-encoded.
        assert_eq!(t.compose_json_plain().unwrap(), line);

        // Non-string, non-date, non-numeric types are written unencoded.
        let mut t = Task::new();
        t.set("recur", "a\"b");
        assert_eq!(t.compose_json_plain().unwrap(), r#"{"recur":"a"b"}"#);
    }

    #[test]
    fn numeric_values() {
        let mut f = fixture(&[("imask", "numeric")]);
        let t = Task::from_input(&mut f.ctx, r#"{"imask":1}"#).unwrap();
        assert_eq!(t.get("imask"), "1.000000");
        assert_eq!(t.compose_json_plain().unwrap(), r#"{"imask":1.000000}"#);
        let t = Task::from_input(&mut f.ctx, r#"{"imask":"2"}"#).unwrap();
        assert_eq!(t.compose_json_plain().unwrap(), r#"{"imask":2}"#);
    }

    #[test]
    fn dates_iso_epoch_and_modification() {
        let mut f = fixture(&[("entry", "date"), ("due", "date"), ("modification", "date"), ("modified", "date")]);
        let t = Task::from_input(&mut f.ctx, r#"{"due":"1370212800","entry":"20130602T224000Z"}"#).unwrap();
        assert_eq!(t.get("entry"), "1370212800");
        assert_eq!(t.get("due"), "1370212800");
        assert_eq!(t.compose_json_plain().unwrap(), r#"{"due":"20130602T224000Z","entry":"20130602T224000Z"}"#);

        // TW-1274: `modification` is stored as `modified`.
        let t = Task::from_input(&mut f.ctx, r#"{"modification":"20130602T224000Z"}"#).unwrap();
        assert!(!t.has("modification"));
        assert_eq!(t.get("modified"), "1370212800");

        // ..and a stored `modification` is written as `modified`.
        let mut t = Task::new();
        t.set("modification", "1370212800");
        assert_eq!(t.compose_json_plain().unwrap(), r#"{"modified":"20130602T224000Z"}"#);

        // A zero date is not written.
        let mut t = Task::new();
        t.set("due", "0");
        assert_eq!(t.compose_json_plain().unwrap(), "{}");

        // Invalid dates throw the Datetime constructor error.
        assert_eq!(
            err(Task::from_input(&mut f.ctx, r#"{"due":"garbage"}"#)),
            "'garbage' is not a valid date in the '' format."
        );
        assert_eq!(
            err(Task::from_input(&mut f.ctx, r#"{"modification":"nope"}"#)),
            "'nope' is not a valid date in the '' format."
        );
        let mut t = Task::new();
        t.set("due", "12abc");
        assert_eq!(err(t.compose_json_plain()), "'12abc' is not a valid date in the '' format.");
    }

    #[test]
    fn annotations() {
        let mut f = fixture(&[("description", "string")]);
        let line = r#"{"annotations":[{"entry":"20130602T224000Z","description":"one \"q\""},{"entry":"1370212800","description":"two"},{"description":"three"}],"description":"d"}"#;
        let t = Task::from_input(&mut f.ctx, line).unwrap();
        assert_eq!(t.annotation_count, 3);
        assert_eq!(t.get("annotation_1370212800"), "one \"q\"");
        // Conflicting entries are incremented.
        assert_eq!(t.get("annotation_1370212801"), "two");
        // A missing entry uses the current time.
        let anns = t.get_annotations();
        assert_eq!(anns.len(), 3);
        let now_key = anns.iter().find(|(_, v)| *v == "three").unwrap().0.clone();
        let stamp: i64 = now_key["annotation_".len()..].parse().unwrap();
        assert!((stamp - localtime::time_now()).abs() < 5);

        let mut t2 = Task::new();
        t2.set("description", "d");
        t2.set_annotations(&BTreeMap::from([
            ("annotation_1370212800".to_string(), "one \"q\"".to_string()),
            ("annotation_1370212801".to_string(), "two".to_string()),
        ]));
        t2.add_tag("t");
        assert_eq!(
            t2.compose_json_plain().unwrap(),
            r#"{"description":"d","annotations":[{"entry":"20130602T224000Z","description":"one \"q\""},{"entry":"20130602T224001Z","description":"two"}],"tags":["t"]}"#
        );

        // As in C++, the separator is written even with no attributes before.
        let mut t3 = Task::new();
        t3.set("annotation_1370212800", "x");
        assert_eq!(
            t3.compose_json_plain().unwrap(),
            r#"{,"annotations":[{"entry":"20130602T224000Z","description":"x"}]}"#
        );
    }

    #[test]
    fn annotation_errors() {
        let mut f = fixture(&[]);
        assert_eq!(
            err(Task::from_input(&mut f.ctx, r#"{"annotations":"foo"}"#)),
            r#"Annotations is malformed: "foo""#
        );
        assert_eq!(
            err(Task::from_input(&mut f.ctx, r#"{"annotations":{"a":1}}"#)),
            r#"Annotations is malformed: {"a":1.000000}"#
        );
        assert_eq!(
            err(Task::from_input(&mut f.ctx, r#"{"annotations":[{"entry":"20130602T224000Z"}]}"#)),
            r#"Annotation is missing a description: {"entry":"20130602T224000Z"}"#
        );
        assert_eq!(
            err(Task::from_input(&mut f.ctx, r#"{"annotations":[{"entry":"bad","description":"x"}]}"#)),
            "'bad' is not a valid date in the '' format."
        );
    }

    #[test]
    fn json_errors_propagate() {
        let mut f = fixture(&[]);
        for bad in ["{", "x", "{} x", r#"{"a":}"#] {
            let expected = json::parse(bad).unwrap_err().to_string();
            assert_eq!(err(Task::from_input(&mut f.ctx, bad)), expected, "{bad}");
        }
        assert_eq!(err(Task::from_input(&mut f.ctx, "x")), "Error: expected '{' or '[' at position 0");
        // A non-object root is ignored.
        let t = Task::from_input(&mut f.ctx, r#"[{"description":"x"}]"#).unwrap();
        assert!(t.is_empty());
    }

    #[test]
    fn depends_errors_propagate() {
        let mut f = fixture(&[("depends", "string")]);
        let mut t = Task::new();
        t.set("uuid", UUID_A);
        assert_eq!(
            err(t.parse_json(&mut f.ctx, &format!(r#"{{"depends":["{UUID_A}"]}}"#))),
            "A task cannot be dependent on itself."
        );
        let mut t = Task::new();
        t.set("uuid", UUID_A);
        assert_eq!(
            err(t.parse(&mut f.ctx, &format!(r#"{{"depends":"{UUID_B},{UUID_A}"}}"#))),
            "A task cannot be dependent on itself."
        );
    }

    #[test]
    fn decorate_with_id_and_urgency() {
        let mut f = fixture(&[("description", "string")]);
        let mut t = Task::new();
        t.id = 5;
        t.set("description", "x");
        assert_eq!(t.compose_json(&mut f.ctx, true).unwrap(), r#"{"id":5,"description":"x","urgency":0}"#);

        // urgency_tags for one tag is 0.8 (a float): `out << float` prints 0.8.
        Task::set_urgency_tags_coefficient(1.0);
        t.add_tag("a");
        assert_eq!(
            t.compose_json(&mut f.ctx, true).unwrap(),
            r#"{"id":5,"description":"x","tags":["a"],"urgency":0.8}"#
        );
        // Undecorated output omits both.
        assert_eq!(t.compose_json(&mut f.ctx, false).unwrap(), r#"{"description":"x","tags":["a"]}"#);

        // Round trip through parse.
        Task::set_attributes(
            [("description", "string"), ("id", "numeric"), ("urgency", "numeric"), ("tags", "string")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
        let decorated = t.compose_json(&mut f.ctx, true).unwrap();
        let mut back = Task::from_input(&mut f.ctx, &decorated).unwrap();
        assert_eq!(back.id, 0);
        assert_eq!(back.compose_json_plain().unwrap(), r#"{"description":"x","tags":["a"]}"#);
        assert_eq!(back.urgency(&mut f.ctx).unwrap(), 0.8f32);
    }

    #[test]
    fn determine_version_cases() {
        let uuid = "27755d92-c5e9-4c21-bd8e-c3dd9e6d3cf7";
        assert_eq!(Task::determine_version(&format!("{uuid} - [t] [a] desc")), 2);
        assert_eq!(Task::determine_version(&format!("{uuid} + [t] [a] [n] desc")), 3);
        assert_eq!(Task::determine_version(&format!("{uuid} X [] [] [] desc")), 3);
        assert_eq!(Task::determine_version(&format!("{uuid} r desc")), 2);
        // Status character not recognised.
        assert_eq!(Task::determine_version(&format!("{uuid} p [t] [a] desc")), 0);
        assert_eq!(Task::determine_version(r#"[description:"x" uuid:"u"]"#), 4);
        assert_eq!(Task::determine_version(r#"[description:"x"]"#), 0);
        assert_eq!(Task::determine_version("X [a] [b] desc"), 1);
        assert_eq!(Task::determine_version("[a] [b] desc"), 1);
        assert_eq!(Task::determine_version("[ab"), 0);
        assert_eq!(Task::determine_version("[abc"), 1);
        assert_eq!(Task::determine_version("[]"), 0);
        assert_eq!(Task::determine_version(""), 0);
        assert_eq!(Task::determine_version("{}"), 0);
        assert_eq!(Task::determine_version(r#"{"uuid":"x"}"#), 0);
    }

    #[test]
    fn parse_legacy_cases() {
        let mut f = fixture(&[]);
        let uuid = "27755d92-c5e9-4c21-bd8e-c3dd9e6d3cf7";
        let mut t = Task::new();
        assert_eq!(
            err(t.parse_legacy(None, "X [a] [b] desc")),
            "Taskwarrior no longer supports file format 1, originally used between 27 November 2006 and 31 December 2007."
        );
        assert_eq!(
            err(t.parse_legacy(None, &format!("{uuid} - [t] [a] desc"))),
            "Taskwarrior no longer supports file format 2, originally used between 1 January 2008 and 12 April 2009."
        );
        assert_eq!(
            err(t.parse_legacy(Some(&mut f.ctx), &format!("{uuid} - [t] [a] [n] desc"))),
            "Taskwarrior no longer supports file format 3, originally used between 23 March 2009 and 16 May 2009."
        );
        assert!(f.ctx.debug_messages.is_empty());

        t.recalc_urgency = false;
        t.parse_legacy(Some(&mut f.ctx), r#"[description:"x" uuid:"u"]"#).unwrap();
        assert!(t.recalc_urgency);
        assert!(t.is_empty());

        assert_eq!(
            err(t.parse_legacy(Some(&mut f.ctx), "garbage")),
            "Unrecognized Taskwarrior file format or blank line in data."
        );
        assert_eq!(f.ctx.debug_messages, vec!["Invalid fileformat at line 'garbage'".to_string()]);
        assert_eq!(err(t.parse_legacy(None, "")), "Unrecognized Taskwarrior file format or blank line in data.");
    }
}

// Faithful, assertion-count-checked ports of test/t_test.cpp and
// test/tw_2689_test.cpp.
#[cfg(test)]
#[path = "task_tests.rs"]
mod upstream_tests;

// Task::modify / Task::substitute over a real Context and CLI2 parse.
#[cfg(test)]
#[path = "task_modify_tests.rs"]
mod modify_tests;
