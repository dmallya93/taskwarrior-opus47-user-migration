//! Rust translation of `src/TDB2.h` / `src/TDB2.cpp`, calling
//! [`taskchampion::Replica`] directly instead of the C++ cxx bridge
//! (`src/taskchampion-cpp/src/lib.rs`).
//!
//! Every bridge call is replaced by the taskchampion call the bridge made:
//! * `tc::new_replica_on_disk` -> [`SqliteStorage::new`] (`<location>`, the
//!   data directory; taskchampion keeps `taskchampion.sqlite3` inside it)
//!   with [`AccessMode`] and `create_if_missing`, then [`Replica::new`].
//! * `replica->create_task` / `get_task` return a [`taskchampion::Task`],
//!   whose `set_value` / `set_status` stamp `modified` once per task and
//!   (for `set_status`) maintain `end`, exactly as the bridge's
//!   `tc::Task::set_value` / `set_status`.
//! * `get_task_data` / `delete_task` use [`TaskData`].
//! * The bridge's async calls ran inside `rt().block_on (..)` on one
//!   current-thread tokio runtime; [`rt`] is that runtime (DD3).
//!
//! C++ `TDB2::add` / `TDB2::modify` call `task.validate`, `Hooks::onAdd` /
//! `onModify` through `Context::getContext ()`. Here they are the free
//! functions [`add`] and [`modify`], which take the [`Context`] and call the
//! storage halves on [`Tdb2`] in the C++ order. [`Tdb2::add_task`] /
//! [`Tdb2::modify_task`] are the same storage operations without validation
//! and hooks (used by tests and helpers).
//!
//! API mapping:
//! * `bool get (int, Task&)` / `bool get (const std::string&, Task&)` ->
//!   [`Tdb2::get_by_id`] / [`Tdb2::get_by_uuid`] returning `Option<Task>`.
//! * `Task* find_pending (uuid)` -> [`Tdb2::find_pending`] returning
//!   `Option<&mut Task>`.
//! * `const std::vector<Task>& pending_tasks ()` -> `Result<&[Task]>`.
//! * `tc::uuid_from_string` panicked on an invalid UUID (aborting the C++
//!   process); here [`uuid_from_string`] returns the panic text as a
//!   [`TwError::Message`].
//! * The `Timer`s in `all_tasks` / `pending_tasks` add to
//!   [`Tdb2::time_load_us`]; Context folds that into its `time_load_us`
//!   for the Perf line. `gc` returns its elapsed time, which Context adds to
//!   `time_gc_us`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use taskchampion::storage::AccessMode;
use taskchampion::{Operation as TcOperation, Operations, Replica, SqliteStorage, Status as TcStatus, TaskData, Uuid, WorkingSet};

use crate::context::Context;
use crate::shared::format::format;
use crate::shared::fs::Path;
use crate::shared::shared::close_enough;
use crate::shared::timer::Timer;
use crate::task::{Status, Task};
use crate::{Result, TwError};

/// The concrete replica type (`tc::Replica` in the bridge).
pub type TcReplica = Replica<SqliteStorage>;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// The bridge's `rt ()`: one lazily created current-thread tokio runtime on
/// which every async taskchampion call is `block_on`-ed.
pub fn rt() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build the tokio runtime")
    })
}

/// The bridge's `tc::uuid_from_string`: parse a UUID string.
///
/// # Errors
/// `<uuid> is not a valid UUID` (the bridge's panic message).
pub fn uuid_from_string(uuid: &str) -> Result<Uuid> {
    Uuid::parse_str(uuid).map_err(|_| TwError::Message(format!("{uuid} is not a valid UUID")))
}

/// The bridge's `Uuid::to_string`: lowercase hyphenated.
fn uuid_to_string(uuid: &Uuid) -> String {
    uuid.as_hyphenated().to_string()
}

/// The bridge's `tc::add_undo_point (ops)`.
pub fn add_undo_point(ops: &mut Operations) {
    ops.push(TcOperation::UndoPoint);
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `statusFromString`: map the C++ representation of status string to
/// the typed taskchampion status.
fn status_from_string(s: &str) -> Result<TcStatus> {
    match s {
        "pending" => Ok(TcStatus::Pending),
        "completed" => Ok(TcStatus::Completed),
        "deleted" => Ok(TcStatus::Deleted),
        "recurring" => Ok(TcStatus::Recurring),
        _ => Err(TwError::Message(format("Unknown task status value '{1}'.", &[&s]))),
    }
}

/// The bridge's `WorkingSet::by_uuid`: 0 when absent.
fn ws_by_uuid(ws: &WorkingSet, uuid: Uuid) -> i32 {
    ws.by_uuid(uuid).unwrap_or(0) as i32
}

/// The bridge's `WorkingSet::by_index`: nil when absent (an `int` index that
/// is negative never matches).
fn ws_by_index(ws: &WorkingSet, index: i32) -> Uuid {
    if index < 0 {
        return Uuid::nil();
    }
    ws.by_index(index as usize).unwrap_or_else(Uuid::nil)
}

/// C++ `Task (rust::Box<tc::TaskData>)` including the `parseTC` tail
/// `id = tdb2.id (uuid)`.
fn task_from_data(data: TaskData, ws: &WorkingSet) -> Task {
    let uuid = data.get_uuid();
    let mut task = Task::from_task_data(data);
    task.id = ws_by_uuid(ws, uuid);
    task
}

/// Microseconds from a `Timer`, as C++ adds `timer.total_us ()` to an
/// `long long` counter.
fn timer_us(timer: &Timer) -> i64 {
    timer.total_us() as i64
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `struct DependencyGraph`: adjacency maps for the dependency graph.
/// Index refers to the `pending_tasks ()` vector.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DependencyGraph {
    /// UUID -> indices of the pending tasks that depend on it.
    pub dependents: HashMap<String, Vec<usize>>,
    /// UUID -> indices of the pending tasks it depends on.
    pub dependencies: HashMap<String, Vec<usize>>,
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `class TDB2`: represents all the files in the task database.
///
/// We adopt a caching strategy which lazily builds `pending_tasks`,
/// `completed_tasks`, `working_set`, `dependency_graph` from the replica
/// whenever its state may have changed.
#[derive(Default)]
pub struct Tdb2 {
    /// C++ `static bool TDB2::debug_mode`, set by
    /// `Context::staticInitialization` from `rc.debug`.
    pub debug_mode: bool,

    /// Microseconds spent loading tasks (`all_tasks`, `pending_tasks`); C++
    /// adds these directly to `Context::time_load_us`. Context folds this
    /// accumulator into its own counter before printing the Perf line.
    pub time_load_us: i64,

    replica: Option<TcReplica>,

    // Cached information from the replica. All caches are invalidated when
    // state may have changed in the replica.
    working_set: Option<WorkingSet>,
    pending_tasks: Option<Vec<Task>>,
    completed_tasks: Option<Vec<Task>>,
    // Dependency cache.
    dependency_graph: Option<DependencyGraph>,
    // Lazily cache UUIDs within the pending set. Avoids scans of the vectors
    // with get/modify.
    pending_index: Option<HashMap<String, usize>>,

    /// UUID -> Task containing all tasks modified in this invocation.
    changes: BTreeMap<String, Task>,
}

impl std::fmt::Debug for Tdb2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tdb2")
            .field("debug_mode", &self.debug_mode)
            .field("time_load_us", &self.time_load_us)
            .field("replica_open", &self.replica.is_some())
            .field("changes", &self.changes.len())
            .finish_non_exhaustive()
    }
}

impl Tdb2 {
    /// C++ `TDB2::TDB2`.
    pub fn new() -> Self {
        Tdb2::default()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `TDB2::open_replica (location, create_if_missing, read_write)`:
    /// the bridge's `new_replica_on_disk`.
    ///
    /// # Errors
    /// The taskchampion error (e.g. `unable to open database file: ...` when
    /// the directory is missing and `create_if_missing` is false).
    pub fn open_replica(&mut self, location: &Path, create_if_missing: bool, read_write: bool) -> Result<()> {
        self.open_replica_at(&location.data, create_if_missing, read_write)
    }

    /// [`open_replica`](Self::open_replica) on a plain directory name.
    pub fn open_replica_at(&mut self, location: &str, create_if_missing: bool, read_write: bool) -> Result<()> {
        let access_mode = if read_write { AccessMode::ReadWrite } else { AccessMode::ReadOnly };
        let dir = PathBuf::from(location);
        let storage = rt().block_on(SqliteStorage::new(dir, access_mode, create_if_missing))?;
        self.replica = Some(Replica::new(storage));
        // A freshly opened replica has nothing cached yet.
        self.invalidate_cached_info();
        Ok(())
    }

    /// Whether `open_replica` has been called successfully.
    pub fn is_open(&self) -> bool {
        self.replica.is_some()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `rust::Box<tc::Replica>& TDB2::replica ()`.
    ///
    /// # Errors
    /// `replica not open` when `open_replica` was never called (C++
    /// asserts).
    pub fn replica(&mut self) -> Result<&mut TcReplica> {
        // One of the open_replica_ methods must be called before this one.
        self.replica.as_mut().ok_or_else(|| TwError::Message("replica not open".to_string()))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const rust::Box<tc::WorkingSet>& TDB2::working_set ()`.
    pub fn working_set(&mut self) -> Result<&WorkingSet> {
        if self.working_set.is_none() {
            let replica = self.replica()?;
            let ws = rt().block_on(replica.working_set())?;
            self.working_set = Some(ws);
        }
        Ok(self.working_set.as_ref().expect("working set just loaded"))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void TDB2::maybe_add_undo_point (rust::Vec<tc::Operation>&)`:
    /// only add an UndoPoint if there are not yet any changes.
    pub fn maybe_add_undo_point(&self, ops: &mut Operations) {
        if self.changes.is_empty() {
            add_undo_point(ops);
        }
    }

    /// The common head of C++ `add` / `modify` after validation: a new
    /// operation list (with an undo point for the first change of this
    /// invocation), then `changes[uuid] = task`.
    pub fn begin_change(&mut self, task: &Task) -> Operations {
        let mut ops = Operations::new();
        self.maybe_add_undo_point(&mut ops);

        let uuid = task.get("uuid");
        self.changes.insert(uuid, task.clone());
        ops
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `TDB2::add` without `validate` and `hooks.onAdd`: add the new
    /// task to the replica.
    pub fn add_task(&mut self, task: &mut Task) -> Result<()> {
        let ops = self.begin_change(task);
        let tcuuid = uuid_from_string(task.get_ref("uuid"))?;
        self.commit_add(task, tcuuid, ops)
    }

    /// The storage tail of C++ `TDB2::add`, after `hooks.onAdd`: create the
    /// task `tcuuid` (the uuid read before the hook ran), set its
    /// attributes, commit, and assign the working-set ID.
    pub fn commit_add(&mut self, task: &mut Task, tcuuid: Uuid, mut ops: Operations) -> Result<()> {
        let replica = self.replica()?;
        let mut tctask = rt().block_on(replica.create_task(tcuuid, &mut ops))?;

        // Add the task attributes, but defer `status` until after every other
        // attribute so `tc::Task::set_status` sees complete input.
        let mut deferred_status = String::new();
        for attr in task.all() {
            // TaskChampion does not store uuid or id in the task data
            if attr == "uuid" || attr == "id" {
                continue;
            }
            if attr == "status" {
                deferred_status = task.get(&attr);
                continue;
            }
            tctask.set_value(attr.clone(), Some(task.get(&attr)), &mut ops)?;
        }
        if !deferred_status.is_empty() {
            tctask.set_status(status_from_string(&deferred_status)?, &mut ops)?;
        }
        let replica = self.replica()?;
        rt().block_on(replica.commit_operations(ops))?;

        self.invalidate_cached_info();

        // get the ID that was assigned to this task
        let id = ws_by_uuid(self.working_set()?, tcuuid);
        if id > 0 {
            task.id = id;
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `TDB2::modify` without `validate` and `hooks.onModify`: modify
    /// the task in storage to match the given task.
    pub fn modify_task(&mut self, task: &mut Task) -> Result<()> {
        // All locally modified tasks are timestamped, implicitly overwriting any
        // changes the user or hooks tried to apply to the "modified" attribute.
        task.set_as_now("modified");
        let uuid = task.get("uuid");

        let ops = self.begin_change(task);

        let original = self.get_by_uuid(&uuid)?;
        let found_original = original.is_some();
        let original = original.unwrap_or_default();
        self.commit_modify(task, &uuid, &original, found_original, ops)
    }

    /// The storage tail of C++ `TDB2::modify`, after `hooks.onModify`.
    ///
    /// Note that there are a few race conditions to consider here.
    /// Taskwarrior loads the entire task into memory and this method then
    /// essentially writes the entire task back to the database. So, if the
    /// task in the database changes between loading the task and this method
    /// being called, this method will "revert" those changes. In practice
    /// this would only occur when multiple `task` invocations run at the
    /// same time and try to modify the same task.
    ///
    /// There is also the possibility that another task process has deleted
    /// the task from the database between the time this process loaded the
    /// task and called this method. In this case, this method throws an error
    /// that will make sense to the user. This is especially unlikely since
    /// tasks are only deleted when they have been unmodified for a long time.
    ///
    /// # Errors
    /// `task no longer exists`; `Unknown task status value '{1}'.`
    pub fn commit_modify(
        &mut self,
        task: &Task,
        uuid: &str,
        original: &Task,
        found_original: bool,
        mut ops: Operations,
    ) -> Result<()> {
        let tcuuid = uuid_from_string(uuid)?;
        let replica = self.replica()?;
        let Some(mut tctask) = rt().block_on(replica.get_task(tcuuid))? else {
            return Err(TwError::Message("task no longer exists".to_string()));
        };

        let mut deferred_status: Option<String> = None;
        let mut seen: HashSet<String> = HashSet::new();
        for k in task.all() {
            // ignore task keys that aren't stored
            if k == "uuid" {
                continue;
            }
            seen.insert(k.clone());
            let v_new = task.get(&k);
            let v_old = original.get(&k); // "" if missing
            if k == "status" {
                if v_new != v_old {
                    deferred_status = Some(v_new);
                }
                continue;
            }
            if v_new == v_old {
                continue;
            }
            // An empty string -> removed.
            if v_new.is_empty() {
                tctask.set_value(k, None, &mut ops)?;
            } else {
                tctask.set_value(k, Some(v_new), &mut ops)?;
            }
        }

        // remove any deleted properties
        for k in original.all() {
            if k == "uuid" || k == "status" {
                continue;
            }
            if !seen.contains(&k) {
                tctask.set_value(k, None, &mut ops)?;
            }
        }

        if let Some(status) = deferred_status {
            tctask.set_status(status_from_string(&status)?, &mut ops)?;
        }

        let replica = self.replica()?;
        rt().block_on(replica.commit_operations(ops))?;

        // If the task entered or left the pending set, we must invalidate the cache.
        let was_pending = found_original && original.get_status()? == Status::Pending;
        let now_pending = task.get_status()? == Status::Pending;
        if was_pending != now_pending || !found_original {
            self.invalidate_cached_info();
            return Ok(());
        }

        // If the task stayed in the set, we can edit the vector in-place.
        // This speeds up modifications a lot relative to reloading and parsing from rust.
        let mut deps_changed = false;
        if self.pending_tasks.is_some() {
            if let Some(pt) = self.find_pending(uuid)? {
                let old_deps = pt.get_dependency_uuids();
                *pt = task.clone();
                let new_deps = task.get_dependency_uuids();
                if old_deps != new_deps {
                    deps_changed = true;
                    self.pending_index()?;
                    let tasks = self.pending_tasks.as_mut().expect("pending tasks loaded");
                    let index = self.pending_index.as_ref().expect("pending index built");
                    dependency_scan(tasks, index)?;
                }
            }
        }
        // We have to drop the dependency map, in case modifications were made to those.
        // We only drop it if they actually changed.
        if deps_changed {
            self.dependency_graph = None;
        }
        // We also have to drop _completed_tasks. This probably isn't strictly necessary
        // but it seems sensible for correctness reasons.
        self.completed_tasks = None;
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void TDB2::purge (Task&)`: delete the task's data from the
    /// replica.
    pub fn purge(&mut self, task: &Task) -> Result<()> {
        let uuid = uuid_from_string(task.get_ref("uuid"))?;
        let mut ops = Operations::new();
        let replica = self.replica()?;
        if let Some(mut tctask) = rt().block_on(replica.get_task_data(uuid))? {
            tctask.delete(&mut ops);
            rt().block_on(replica.commit_operations(ops))?;
        }

        self.invalidate_cached_info();
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void TDB2::get_changes (std::vector<Task>&)`: the tasks modified
    /// in this invocation, in UUID order.
    pub fn get_changes(&self) -> Vec<Task> {
        self.changes.values().cloned().collect()
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void TDB2::gc ()`: rebuild (and renumber) the working set when
    /// `gc_enabled` (C++ `config.getBoolean ("gc")`; allowed as an override,
    /// but not recommended). Returns the elapsed microseconds, which the
    /// caller adds to `Context::time_gc_us`.
    pub fn gc(&mut self, gc_enabled: bool) -> Result<i64> {
        let timer = Timer::new();

        // Allowed as an override, but not recommended.
        if gc_enabled {
            let replica = self.replica()?;
            rt().block_on(replica.rebuild_working_set(true))?;
        }

        Ok(timer_us(&timer))
    }

    /// The bridge's `Replica::rebuild_working_set (renumber)`.
    pub fn rebuild_working_set(&mut self, renumber: bool) -> Result<()> {
        let replica = self.replica()?;
        rt().block_on(replica.rebuild_working_set(renumber))?;
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void TDB2::expire_tasks ()`.
    pub fn expire_tasks(&mut self) -> Result<()> {
        let replica = self.replica()?;
        rt().block_on(replica.expire_tasks())?;
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `int TDB2::latest_id ()`: latest ID is that of the last pending
    /// task.
    pub fn latest_id(&mut self) -> Result<i32> {
        Ok(self.working_set()?.largest_index() as i32)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const std::vector<Task> TDB2::all_tasks ()`.
    ///
    /// Not cached: callers read this once per process, and it can be very
    /// large.
    pub fn all_tasks(&mut self) -> Result<Vec<Task>> {
        let timer = Timer::new();
        let replica = self.replica()?;
        let all_tctasks = rt().block_on(replica.all_task_data())?;
        let ws = self.working_set()?;
        let mut all: Vec<Task> = Vec::with_capacity(all_tctasks.len());
        for (_, tctask) in all_tctasks {
            all.push(task_from_data(tctask, ws));
        }

        // Build a temporary map so that dependency_scan can resolve references
        // inside all_tasks.
        let mut all_index: HashMap<String, usize> = HashMap::with_capacity(all.len());
        for (i, task) in all.iter().enumerate() {
            all_index.insert(task.get("uuid"), i);
        }

        dependency_scan(&mut all, &all_index)?;

        self.time_load_us += timer_us(&timer);
        Ok(all)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const std::vector<Task>& TDB2::pending_tasks ()`: load and cache
    /// pending tasks. The first call will build the UUID index, after which
    /// it is reused.
    pub fn pending_tasks(&mut self) -> Result<&[Task]> {
        self.load_pending_tasks()?;
        Ok(self.pending_tasks.as_deref().expect("pending tasks loaded"))
    }

    /// The body of [`pending_tasks`](Self::pending_tasks).
    fn load_pending_tasks(&mut self) -> Result<()> {
        if self.pending_tasks.is_none() {
            let timer = Timer::new();

            let replica = self.replica()?;
            let pending_tctasks = rt().block_on(replica.pending_task_data())?;
            let ws = self.working_set()?;
            let mut result: Vec<Task> = Vec::with_capacity(pending_tctasks.len());

            for tctask in pending_tctasks {
                result.push(task_from_data(tctask, ws));
            }

            // Build a UUID map for use with get()/modify() and dependency_scan()
            // while the pending vector is already in memory.
            let mut index: HashMap<String, usize> = HashMap::with_capacity(result.len());
            for (i, task) in result.iter().enumerate() {
                index.entry(task.get("uuid")).or_insert(i);
            }

            dependency_scan(&mut result, &index)?;
            self.pending_index = Some(index);

            self.time_load_us += timer_us(&timer);
            self.pending_tasks = Some(result);
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const std::vector<Task>& TDB2::completed_tasks ()`: load and
    /// cache all completed tasks by scanning all tasks, and excluding those
    /// in the working set. We cache it to speed up those reports which
    /// involve completed tasks.
    pub fn completed_tasks(&mut self) -> Result<&[Task]> {
        if self.completed_tasks.is_none() {
            let replica = self.replica()?;
            let all_tctasks = rt().block_on(replica.all_task_data())?;
            let ws = self.working_set()?;

            let mut result: Vec<Task> = Vec::with_capacity(all_tctasks.len());

            for (_, tctask) in all_tctasks {
                // if this task is _not_ in the working set, return it.
                if ws_by_uuid(ws, tctask.get_uuid()) == 0 {
                    result.push(task_from_data(tctask, ws));
                }
            }
            self.completed_tasks = Some(result);
        }
        Ok(self.completed_tasks.as_deref().expect("completed tasks loaded"))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const DependencyGraph& TDB2::dependency_graph ()`: build and
    /// return the dependency map for pending tasks. We invalidate it
    /// whenever pending_tasks may have changed.
    pub fn dependency_graph(&mut self) -> Result<&DependencyGraph> {
        if self.dependency_graph.is_none() {
            self.load_pending_tasks()?;
            // reuse the UUID index
            self.pending_index()?;
            let tasks = self.pending_tasks.as_ref().expect("pending tasks loaded");
            let index = self.pending_index.as_ref().expect("pending index built");
            self.dependency_graph = Some(build_dependency_graph(tasks, index));
        }

        Ok(self.dependency_graph.as_ref().expect("dependency graph built"))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const std::unordered_map<std::string, size_t>& TDB2::pending_index ()`:
    /// builds and returns the UUID map if it is missing.
    ///
    /// C++ requires `pending_tasks` to be loaded; here they are loaded on
    /// demand.
    pub fn pending_index(&mut self) -> Result<&HashMap<String, usize>> {
        if self.pending_index.is_none() {
            self.load_pending_tasks()?;
        }
        if self.pending_index.is_none() {
            let tasks = self.pending_tasks.as_ref().expect("pending tasks loaded");
            let mut index: HashMap<String, usize> = HashMap::with_capacity(tasks.len());
            for (i, task) in tasks.iter().enumerate() {
                index.entry(task.get("uuid")).or_insert(i);
            }
            self.pending_index = Some(index);
        }

        Ok(self.pending_index.as_ref().expect("pending index built"))
    }

    /// C++ `Task* TDB2::find_pending (const std::string& uuid)`: finds the
    /// UUID in the index. Returns `None` if the task is not in the pending
    /// set.
    pub fn find_pending(&mut self, uuid: &str) -> Result<Option<&mut Task>> {
        let position = self.find_pending_position(uuid)?;
        Ok(position.map(|i| &mut self.pending_tasks.as_mut().expect("pending tasks loaded")[i]))
    }

    /// The index of `uuid` in [`pending_tasks`](Self::pending_tasks).
    pub fn find_pending_position(&mut self, uuid: &str) -> Result<Option<usize>> {
        self.load_pending_tasks()?;
        Ok(self.pending_index()?.get(uuid).copied())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `void TDB2::invalidate_cached_info ()`.
    pub fn invalidate_cached_info(&mut self) {
        self.pending_tasks = None;
        self.completed_tasks = None;
        self.working_set = None;
        self.dependency_graph = None;
        self.pending_index = None;
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `bool TDB2::get (int, Task&)`: locate task by ID, wherever it is.
    pub fn get_by_id(&mut self, id: i32) -> Result<Option<Task>> {
        let tcuuid = ws_by_index(self.working_set()?, id);
        if !tcuuid.is_nil() {
            let uuid = uuid_to_string(&tcuuid);
            // Load index of pending tasks.
            self.load_pending_tasks()?;
            // Lookup the UUID in the index instead of scanning the vector.
            if let Some(pt) = self.find_pending(&uuid)? {
                return Ok(Some(pt.clone()));
            }
        }

        Ok(None)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `bool TDB2::get (const std::string&, Task&)`: locate task by UUID,
    /// including by partial ID, wherever it is.
    pub fn get_by_uuid(&mut self, uuid: &str) -> Result<Option<Task>> {
        self.load_pending_tasks()?;

        // Try to match exact UUID within the index.
        if let Some(pt) = self.find_pending(uuid)? {
            return Ok(Some(pt.clone()));
        }

        // try a partial match
        if uuid.len() < 36 {
            for pending_task in self.pending_tasks.as_ref().expect("pending tasks loaded") {
                if close_enough(pending_task.get_ref("uuid"), uuid, uuid.len() as u32) {
                    return Ok(Some(pending_task.clone()));
                }
            }
        }

        // Nothing to do but iterate over all tasks and check whether it's closeEnough.
        let replica = self.replica()?;
        let all_tctasks = rt().block_on(replica.all_task_data())?;
        for (_, tctask) in all_tctasks {
            let tctask_uuid = uuid_to_string(&tctask.get_uuid());
            if close_enough(&tctask_uuid, uuid, uuid.len() as u32) {
                let ws = self.working_set()?;
                return Ok(Some(task_from_data(tctask, ws)));
            }
        }

        Ok(None)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `bool TDB2::has (const std::string&)`: locate task by UUID,
    /// wherever it is.
    pub fn has(&mut self, uuid: &str) -> Result<bool> {
        let tcuuid = uuid_from_string(uuid)?;
        let replica = self.replica()?;
        Ok(rt().block_on(replica.get_task_data(tcuuid))?.is_some())
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const std::vector<Task> TDB2::siblings (Task&)`.
    pub fn siblings(&mut self, task: &Task) -> Result<Vec<Task>> {
        let mut results = Vec::new();
        if task.has("parent") {
            let parent = task.get("parent");

            for i in self.pending_tasks()? {
                // Do not include self in results.
                if i.id != task.id {
                    // Do not include completed or deleted tasks.
                    let status = i.get_status()?;
                    if status != Status::Completed && status != Status::Deleted {
                        // If task has the same parent, it is a sibling.
                        if i.has("parent") && i.get_ref("parent") == parent {
                            results.push(i.clone());
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `const std::vector<Task> TDB2::children (Task&)`: return the child
    /// tasks of a parent. Uses the pending tasks cache, to avoid having to
    /// fetch this info from the replica.
    pub fn children(&mut self, parent: &Task) -> Result<Vec<Task>> {
        let mut results = Vec::new();
        let this_uuid = parent.get("uuid");

        for i in self.pending_tasks()? {
            if i.get_ref("uuid") == this_uuid {
                continue;
            }
            if i.has("parent") && i.get_ref("parent") == this_uuid {
                results.push(i.clone());
            }
        }
        Ok(results)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `std::string TDB2::uuid (int)`: `""` when the ID is not in the
    /// working set.
    pub fn uuid(&mut self, id: i32) -> Result<String> {
        let uuid = ws_by_index(self.working_set()?, id);
        if uuid.is_nil() {
            return Ok(String::new());
        }
        Ok(uuid_to_string(&uuid))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `int TDB2::id (const std::string&)`: 0 when the UUID is not in
    /// the working set.
    pub fn id(&mut self, uuid: &str) -> Result<i32> {
        let tcuuid = uuid_from_string(uuid)?;
        Ok(ws_by_uuid(self.working_set()?, tcuuid))
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `int TDB2::num_local_changes ()`.
    pub fn num_local_changes(&mut self) -> Result<i32> {
        let replica = self.replica()?;
        Ok(rt().block_on(replica.num_local_operations())? as i32)
    }

    ////////////////////////////////////////////////////////////////////////////
    /// C++ `int TDB2::num_reverts_possible ()`.
    pub fn num_reverts_possible(&mut self) -> Result<i32> {
        let replica = self.replica()?;
        Ok(rt().block_on(replica.num_undo_points())? as i32)
    }

    ////////////////////////////////////////////////////////////////////////////
    // Replica accessors used by the undo, info, import and sync commands
    // (milestone 7). These are the bridge's `Replica` methods; like the C++
    // callers (which use `tdb2.replica ()` directly), they do not invalidate
    // the caches.

    /// The bridge's `Replica::commit_operations`.
    pub fn commit_operations(&mut self, ops: Operations) -> Result<()> {
        let replica = self.replica()?;
        rt().block_on(replica.commit_operations(ops))?;
        Ok(())
    }

    /// The bridge's `Replica::get_task_data`.
    pub fn get_task_data(&mut self, uuid: Uuid) -> Result<Option<TaskData>> {
        let replica = self.replica()?;
        Ok(rt().block_on(replica.get_task_data(uuid))?)
    }

    /// The bridge's `Replica::get_undo_operations`.
    pub fn get_undo_operations(&mut self) -> Result<Operations> {
        let replica = self.replica()?;
        Ok(rt().block_on(replica.get_undo_operations())?)
    }

    /// The bridge's `Replica::commit_reversed_operations`: `false` when other
    /// operations have occurred. It rebuilds the working set itself.
    pub fn commit_reversed_operations(&mut self, ops: Operations) -> Result<bool> {
        let replica = self.replica()?;
        Ok(rt().block_on(replica.commit_reversed_operations(ops))?)
    }

    /// The bridge's `Replica::get_task_operations`.
    pub fn get_task_operations(&mut self, uuid: Uuid) -> Result<Operations> {
        let replica = self.replica()?;
        Ok(rt().block_on(replica.get_task_operations(uuid))?)
    }

    /// The bridge's `Replica::num_local_operations`.
    pub fn num_local_operations(&mut self) -> Result<usize> {
        let replica = self.replica()?;
        Ok(rt().block_on(replica.num_local_operations())?)
    }

    /// Build the server from `config` and sync the replica with it (the
    /// shared body of the bridge's `sync_to_*`).
    fn sync_with(&mut self, config: taskchampion::ServerConfig, avoid_snapshots: bool) -> Result<()> {
        let replica = self.replica.as_mut().ok_or_else(|| TwError::Message("replica not open".to_string()))?;
        rt().block_on(async {
            let mut server = config.into_server().await?;
            replica.sync(&mut server, avoid_snapshots).await
        })?;
        Ok(())
    }

    /// The bridge's `Replica::sync_to_local`.
    pub fn sync_to_local(&mut self, server_dir: &str, avoid_snapshots: bool) -> Result<()> {
        let config = taskchampion::ServerConfig::Local {
            server_dir: server_dir.into(),
        };
        self.sync_with(config, avoid_snapshots)
    }

    /// The bridge's `Replica::sync_to_remote`.
    pub fn sync_to_remote(
        &mut self,
        url: &str,
        client_id: Uuid,
        encryption_secret: &str,
        avoid_snapshots: bool,
    ) -> Result<()> {
        let config = taskchampion::ServerConfig::Remote {
            url: url.to_string(),
            client_id,
            encryption_secret: encryption_secret.as_bytes().to_vec(),
        };
        self.sync_with(config, avoid_snapshots)
    }

    /// The bridge's `Replica::sync_to_aws_with_profile`.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_to_aws_with_profile(
        &mut self,
        region: &str,
        bucket: &str,
        profile_name: &str,
        endpoint_url: &str,
        force_path_style: bool,
        encryption_secret: &str,
        avoid_snapshots: bool,
    ) -> Result<()> {
        let config = taskchampion::ServerConfig::Aws {
            region: Some(region.to_string()),
            bucket: bucket.to_string(),
            credentials: taskchampion::server::AwsCredentials::Profile {
                profile_name: profile_name.to_string(),
            },
            encryption_secret: encryption_secret.as_bytes().to_vec(),
            endpoint_url: (!endpoint_url.is_empty()).then(|| endpoint_url.to_string()),
            force_path_style,
        };
        self.sync_with(config, avoid_snapshots)
    }

    /// The bridge's `Replica::sync_to_aws_with_access_key`.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_to_aws_with_access_key(
        &mut self,
        region: &str,
        bucket: &str,
        access_key_id: &str,
        secret_access_key: &str,
        endpoint_url: &str,
        force_path_style: bool,
        encryption_secret: &str,
        avoid_snapshots: bool,
    ) -> Result<()> {
        let config = taskchampion::ServerConfig::Aws {
            region: Some(region.to_string()),
            bucket: bucket.to_string(),
            credentials: taskchampion::server::AwsCredentials::AccessKey {
                access_key_id: access_key_id.to_string(),
                secret_access_key: secret_access_key.to_string(),
            },
            encryption_secret: encryption_secret.as_bytes().to_vec(),
            endpoint_url: (!endpoint_url.is_empty()).then(|| endpoint_url.to_string()),
            force_path_style,
        };
        self.sync_with(config, avoid_snapshots)
    }

    /// The bridge's `Replica::sync_to_aws_with_default_creds`.
    pub fn sync_to_aws_with_default_creds(
        &mut self,
        region: &str,
        bucket: &str,
        endpoint_url: &str,
        force_path_style: bool,
        encryption_secret: &str,
        avoid_snapshots: bool,
    ) -> Result<()> {
        let config = taskchampion::ServerConfig::Aws {
            region: Some(region.to_string()),
            bucket: bucket.to_string(),
            credentials: taskchampion::server::AwsCredentials::Default,
            encryption_secret: encryption_secret.as_bytes().to_vec(),
            endpoint_url: (!endpoint_url.is_empty()).then(|| endpoint_url.to_string()),
            force_path_style,
        };
        self.sync_with(config, avoid_snapshots)
    }

    /// The bridge's `Replica::sync_to_gcp`: an empty `credential_path` means
    /// application default credentials.
    pub fn sync_to_gcp(
        &mut self,
        bucket: &str,
        credential_path: &str,
        encryption_secret: &str,
        avoid_snapshots: bool,
    ) -> Result<()> {
        let config = taskchampion::ServerConfig::Gcp {
            bucket: bucket.to_string(),
            credential_path: if credential_path.is_empty() {
                None
            } else {
                Some(credential_path.to_string())
            },
            encryption_secret: encryption_secret.as_bytes().to_vec(),
        };
        self.sync_with(config, avoid_snapshots)
    }

    /// The bridge's `Replica::sync_to_git`: empty `remote` / `git_path` mean
    /// none.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_to_git(
        &mut self,
        local_path: &str,
        branch: &str,
        remote: &str,
        local_only: bool,
        encryption_secret: &str,
        git_path: &str,
        avoid_snapshots: bool,
    ) -> Result<()> {
        let config = taskchampion::ServerConfig::Git {
            local_path: local_path.into(),
            branch: branch.to_string(),
            remote: if remote.is_empty() { None } else { Some(remote.to_string()) },
            local_only,
            encryption_secret: encryption_secret.as_bytes().to_vec(),
            git_path: if git_path.is_empty() { None } else { Some(git_path.into()) },
        };
        self.sync_with(config, avoid_snapshots)
    }
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void TDB2::add (Task&)`: add the new task to the replica.
pub fn add(ctx: &mut Context, task: &mut Task) -> Result<()> {
    // Ensure the task is consistent, and provide defaults if necessary.
    // bool argument to validate() is "applyDefault", to apply default values for
    // properties not otherwise given.
    // VALIDATE-HOOK (wired by the Task::validate task): task.validate(ctx, true)?;

    let ops = ctx.tdb2.begin_change(task);
    let tcuuid = uuid_from_string(task.get_ref("uuid"))?;

    // run hooks for this new task
    ctx.hooks.on_add(task)?;

    ctx.tdb2.commit_add(task, tcuuid, ops)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void TDB2::modify (Task&)`: modify the task in storage to match the
/// given task (see [`Tdb2::commit_modify`]).
pub fn modify(ctx: &mut Context, task: &mut Task) -> Result<()> {
    // All locally modified tasks are timestamped, implicitly overwriting any
    // changes the user or hooks tried to apply to the "modified" attribute.
    task.set_as_now("modified");
    // VALIDATE-HOOK (wired by the Task::validate task): task.validate(ctx, false)?;
    let uuid = task.get("uuid");

    let ops = ctx.tdb2.begin_change(task);

    // invoke the hook and allow it to modify the task before updating
    let original = ctx.tdb2.get_by_uuid(&uuid)?;
    let found_original = original.is_some();
    let original = original.unwrap_or_default();
    ctx.hooks.on_modify(&original, task)?;

    ctx.tdb2.commit_modify(task, &uuid, &original, found_original, ops)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `static void dependency_scan (...)`: set `Task::is_blocked` /
/// `Task::is_blocking` flags using the pre-built UUID map.
fn dependency_scan(tasks: &mut [Task], uuid_index: &HashMap<String, usize>) -> Result<()> {
    // Reset all flags first. This is for safety reasons - if we don't do this
    // dependency_scan() only sets them to true, so it can stay true (within the cache)
    // even after we have changed a task's dependencies after a modify when
    // dependency_scan() runs the second time - the flag will still be set to true!
    // In many cases, this isn't user-visible - it's only visible in reports
    // or filters that explicitly filter for +BLOCKING.
    for task in tasks.iter_mut() {
        task.is_blocking = false;
        task.is_blocked = false;
    }
    for i in 0..tasks.len() {
        let lstatus = tasks[i].get_status()?;
        for dep in tasks[i].get_dependency_uuids() {
            let Some(&j) = uuid_index.get(&dep) else {
                continue;
            };

            let rstatus = tasks[j].get_status()?;
            if lstatus != Status::Completed
                && lstatus != Status::Deleted
                && rstatus != Status::Completed
                && rstatus != Status::Deleted
            {
                tasks[i].is_blocked = true;
                tasks[j].is_blocking = true;
            }
        }
    }
    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `static DependencyGraph build_dependency_graph (...)`: build the full
/// dependency map from the task vector.
fn build_dependency_graph(tasks: &[Task], uuid_index: &HashMap<String, usize>) -> DependencyGraph {
    let mut graph = DependencyGraph::default();

    for (i, task) in tasks.iter().enumerate() {
        let deps = task.get_dependency_uuids();
        if deps.is_empty() {
            continue;
        }

        let uuid = task.get("uuid");

        for dep in deps {
            let Some(&index) = uuid_index.get(&dep) else {
                continue;
            };

            graph.dependencies.entry(uuid.clone()).or_default().push(index);
            graph.dependents.entry(dep).or_default().push(i);
        }
    }

    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const U1: &str = "00000000-0000-0000-0000-000000000001";
    const U2: &str = "00000000-0000-0000-0000-000000000002";
    const U3: &str = "00000000-0000-0000-0000-000000000003";

    fn open(dir: &TempDir) -> Tdb2 {
        let mut tdb2 = Tdb2::new();
        tdb2.open_replica_at(dir.path().to_str().unwrap(), true, true).unwrap();
        tdb2
    }

    fn task(uuid: &str, description: &str, status: &str) -> Task {
        let mut t = Task::new();
        t.set("uuid", uuid);
        t.set("description", description);
        t.set("status", status);
        t.set("entry", "1700000000");
        t
    }

    #[test]
    fn replica_not_open() {
        let mut tdb2 = Tdb2::new();
        assert!(!tdb2.is_open());
        assert_eq!(tdb2.latest_id().unwrap_err().to_string(), "replica not open");
        assert_eq!(tdb2.pending_tasks().unwrap_err().to_string(), "replica not open");
    }

    #[test]
    fn open_creates_database_in_directory() {
        let dir = TempDir::new().unwrap();
        let data = dir.path().join("data");
        let mut tdb2 = Tdb2::new();
        tdb2.open_replica_at(data.to_str().unwrap(), true, true).unwrap();
        assert!(data.join("taskchampion.sqlite3").exists());
        assert_eq!(tdb2.latest_id().unwrap(), 0);
    }

    #[test]
    fn open_missing_without_create_fails() {
        let dir = TempDir::new().unwrap();
        let data = dir.path().join("missing");
        let mut tdb2 = Tdb2::new();
        let err = tdb2.open_replica_at(data.to_str().unwrap(), false, true).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("unable to open database file"), "{text}");
        assert!(text.contains("Error code 14"), "{text}");
        assert!(!data.exists());
    }

    #[test]
    fn add_get_roundtrip_and_ids() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut t1 = task(U1, "one", "pending");
        tdb2.add_task(&mut t1).unwrap();
        assert_eq!(t1.id, 1);
        let mut t2 = task(U2, "two", "pending");
        tdb2.add_task(&mut t2).unwrap();
        assert_eq!(t2.id, 2);
        assert_eq!(tdb2.latest_id().unwrap(), 2);

        let got = tdb2.get_by_id(1).unwrap().unwrap();
        assert_eq!(got.get("description"), "one");
        assert_eq!(got.id, 1);
        assert_eq!(got.get("uuid"), U1);
        assert!(got.has("modified"), "set_value stamps modified");
        assert!(tdb2.get_by_id(3).unwrap().is_none());
        assert!(tdb2.get_by_id(-1).unwrap().is_none());

        assert_eq!(tdb2.uuid(2).unwrap(), U2);
        assert_eq!(tdb2.uuid(7).unwrap(), "");
        assert_eq!(tdb2.id(U2).unwrap(), 2);
        assert_eq!(tdb2.id(U3).unwrap(), 0);
        assert!(tdb2.has(U1).unwrap());
        assert!(!tdb2.has(U3).unwrap());
        assert_eq!(tdb2.id("bogus").unwrap_err().to_string(), "bogus is not a valid UUID");

        let pending = tdb2.pending_tasks().unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|t| t.id > 0));
        assert_eq!(tdb2.all_tasks().unwrap().len(), 2);
        assert!(tdb2.completed_tasks().unwrap().is_empty());

        let changes = tdb2.get_changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].get("uuid"), U1);
    }

    #[test]
    fn deferred_status_sets_end() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut t = task(U1, "done already", "completed");
        tdb2.add_task(&mut t).unwrap();
        assert_eq!(t.id, 0, "completed tasks are not in the working set");
        let got = tdb2.get_by_uuid(U1).unwrap().unwrap();
        assert_eq!(got.get("status"), "completed");
        assert!(got.has("end"), "tc set_status stamps end");
        assert_eq!(tdb2.completed_tasks().unwrap().len(), 1);
        assert!(tdb2.pending_tasks().unwrap().is_empty());
    }

    #[test]
    fn unknown_status_is_error() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut t = task(U1, "bad", "waiting");
        let err = tdb2.add_task(&mut t).unwrap_err();
        assert_eq!(err.to_string(), "Unknown task status value 'waiting'.");
    }

    #[test]
    fn modify_and_remove_attributes() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut t = task(U1, "one", "pending");
        t.set("project", "home");
        tdb2.add_task(&mut t).unwrap();

        let mut m = tdb2.get_by_id(1).unwrap().unwrap();
        m.set("description", "changed");
        m.remove("project");
        m.set("priority", "H");
        tdb2.modify_task(&mut m).unwrap();

        let got = tdb2.get_by_uuid(U1).unwrap().unwrap();
        assert_eq!(got.get("description"), "changed");
        assert!(!got.has("project"));
        assert_eq!(got.get("priority"), "H");
        assert_eq!(got.id, 1);

        // An empty value removes the attribute.
        let mut m = got.clone();
        m.set("priority", "");
        tdb2.modify_task(&mut m).unwrap();
        tdb2.invalidate_cached_info();
        let got = tdb2.get_by_uuid(U1).unwrap().unwrap();
        assert!(!got.has("priority"));

        // Completing leaves the pending set: the cache is invalidated, but
        // the task stays in the working set until the next gc.
        let mut m = got.clone();
        m.set("status", "completed");
        tdb2.modify_task(&mut m).unwrap();
        assert!(tdb2.pending_index.is_none(), "cache invalidated");
        assert_eq!(tdb2.pending_tasks().unwrap()[0].get("status"), "completed");
        tdb2.gc(true).unwrap();
        tdb2.invalidate_cached_info();
        assert!(tdb2.pending_tasks().unwrap().is_empty());
        let got = tdb2.get_by_uuid(U1).unwrap().unwrap();
        assert_eq!(got.get("status"), "completed");
        assert!(got.has("end"));
    }

    #[test]
    fn modify_missing_task_errors() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut t = task(U1, "ghost", "pending");
        assert_eq!(tdb2.modify_task(&mut t).unwrap_err().to_string(), "task no longer exists");
    }

    #[test]
    fn purge_removes_task() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut t = task(U1, "one", "deleted");
        tdb2.add_task(&mut t).unwrap();
        assert!(tdb2.has(U1).unwrap());
        tdb2.purge(&t).unwrap();
        assert!(!tdb2.has(U1).unwrap());
        assert!(tdb2.get_by_uuid(U1).unwrap().is_none());
        // Purging a missing task is a no-op.
        tdb2.purge(&t).unwrap();
    }

    #[test]
    fn undo_points_and_local_changes() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        assert_eq!(tdb2.num_reverts_possible().unwrap(), 0);
        assert_eq!(tdb2.num_local_changes().unwrap(), 0);
        let mut t1 = task(U1, "one", "pending");
        tdb2.add_task(&mut t1).unwrap();
        let mut t2 = task(U2, "two", "pending");
        tdb2.add_task(&mut t2).unwrap();
        // Only the first change of an invocation adds an undo point.
        assert_eq!(tdb2.num_reverts_possible().unwrap(), 1);
        assert!(tdb2.num_local_changes().unwrap() > 2);

        let undo = tdb2.get_undo_operations().unwrap();
        assert!(matches!(undo.first(), Some(TcOperation::UndoPoint)));
        let ops = tdb2.get_task_operations(uuid_from_string(U1).unwrap()).unwrap();
        assert!(ops.iter().any(|op| matches!(op, TcOperation::Create { .. })));

        // A new invocation adds a fresh undo point.
        drop(tdb2);
        let mut tdb2 = open(&dir);
        let mut t3 = task(U3, "three", "pending");
        tdb2.add_task(&mut t3).unwrap();
        assert_eq!(tdb2.num_reverts_possible().unwrap(), 2);

        let undo = tdb2.get_undo_operations().unwrap();
        assert!(tdb2.commit_reversed_operations(undo).unwrap());
        assert!(!tdb2.has(U3).unwrap());
        assert_eq!(tdb2.num_reverts_possible().unwrap(), 1);
    }

    #[test]
    fn siblings_and_children() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut parent = task(U1, "parent", "recurring");
        tdb2.add_task(&mut parent).unwrap();
        let mut c1 = task(U2, "child1", "pending");
        c1.set("parent", U1);
        tdb2.add_task(&mut c1).unwrap();
        let mut c2 = task(U3, "child2", "pending");
        c2.set("parent", U1);
        tdb2.add_task(&mut c2).unwrap();

        let children = tdb2.children(&parent).unwrap();
        assert_eq!(children.len(), 2);
        let siblings = tdb2.siblings(&c1).unwrap();
        assert_eq!(siblings.len(), 1);
        assert_eq!(siblings[0].get("uuid"), U3);
        assert!(tdb2.siblings(&parent).unwrap().is_empty());
    }

    #[test]
    fn dependency_flags_and_graph() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut t1 = task(U1, "blocker", "pending");
        tdb2.add_task(&mut t1).unwrap();
        let mut t2 = task(U2, "blocked", "pending");
        t2.set(&Task::dep2attr(U1), "");
        tdb2.add_task(&mut t2).unwrap();
        let mut t3 = task(U3, "free", "pending");
        tdb2.add_task(&mut t3).unwrap();

        let blocker = tdb2.get_by_uuid(U1).unwrap().unwrap();
        assert!(blocker.is_blocking && !blocker.is_blocked);
        let blocked = tdb2.get_by_uuid(U2).unwrap().unwrap();
        assert!(blocked.is_blocked && !blocked.is_blocking);
        let free = tdb2.get_by_uuid(U3).unwrap().unwrap();
        assert!(!free.is_blocked && !free.is_blocking);

        let all = tdb2.all_tasks().unwrap();
        let b = all.iter().find(|t| t.get_ref("uuid") == U2).unwrap();
        assert!(b.is_blocked);

        let graph = tdb2.dependency_graph().unwrap().clone();
        let i1 = tdb2.find_pending_position(U1).unwrap().unwrap();
        let i2 = tdb2.find_pending_position(U2).unwrap().unwrap();
        assert_eq!(graph.dependencies[U2], vec![i1]);
        assert_eq!(graph.dependents[U1], vec![i2]);

        // Removing the dependency in place rescans the flags.
        let mut m = blocked.clone();
        m.remove(&Task::dep2attr(U1));
        tdb2.modify_task(&mut m).unwrap();
        let blocker = tdb2.get_by_uuid(U1).unwrap().unwrap();
        assert!(!blocker.is_blocking);
        assert!(tdb2.dependency_graph().unwrap().dependencies.is_empty());
    }

    #[test]
    fn partial_uuid_get() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let u = "a1b2c3d4-0000-0000-0000-000000000001";
        let mut t = task(u, "partial", "pending");
        tdb2.add_task(&mut t).unwrap();
        let mut d = task("ffffffff-0000-0000-0000-000000000001", "gone", "deleted");
        tdb2.add_task(&mut d).unwrap();

        assert_eq!(tdb2.get_by_uuid("a1b2c3d4").unwrap().unwrap().get("uuid"), u);
        assert_eq!(tdb2.get_by_uuid("A1B2C3").unwrap().unwrap().get("uuid"), u);
        // Not pending: found by scanning all tasks.
        let got = tdb2.get_by_uuid("ffffffff").unwrap().unwrap();
        assert_eq!(got.get("description"), "gone");
        assert_eq!(got.id, 0);
        assert!(tdb2.get_by_uuid("12345678").unwrap().is_none());
    }

    #[test]
    fn read_only_refuses_writes() {
        let dir = TempDir::new().unwrap();
        {
            let mut tdb2 = open(&dir);
            let mut t = task(U1, "one", "pending");
            tdb2.add_task(&mut t).unwrap();
        }
        let mut tdb2 = Tdb2::new();
        tdb2.open_replica_at(dir.path().to_str().unwrap(), true, false).unwrap();
        assert_eq!(tdb2.pending_tasks().unwrap().len(), 1);
        let mut t = task(U2, "two", "pending");
        assert!(tdb2.add_task(&mut t).is_err());
    }

    #[test]
    fn gc_rebuilds_working_set() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut t1 = task(U1, "one", "pending");
        tdb2.add_task(&mut t1).unwrap();
        let mut t2 = task(U2, "two", "pending");
        tdb2.add_task(&mut t2).unwrap();
        let mut m = tdb2.get_by_id(1).unwrap().unwrap();
        m.set("status", "completed");
        tdb2.modify_task(&mut m).unwrap();
        assert_eq!(tdb2.id(U2).unwrap(), 2);

        assert!(tdb2.gc(false).unwrap() >= 0);
        tdb2.invalidate_cached_info();
        assert_eq!(tdb2.id(U2).unwrap(), 2, "gc disabled leaves IDs");

        tdb2.gc(true).unwrap();
        tdb2.invalidate_cached_info();
        assert_eq!(tdb2.id(U2).unwrap(), 1, "gc renumbers");
        assert_eq!(tdb2.latest_id().unwrap(), 1);
        tdb2.expire_tasks().unwrap();
    }

    #[test]
    fn time_load_accumulates() {
        let dir = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        assert_eq!(tdb2.time_load_us, 0);
        tdb2.pending_tasks().unwrap();
        tdb2.all_tasks().unwrap();
        assert!(tdb2.time_load_us >= 0);
    }

    #[test]
    fn sync_to_local_roundtrip() {
        let dir = TempDir::new().unwrap();
        let server = TempDir::new().unwrap();
        let mut tdb2 = open(&dir);
        let mut t = task(U1, "synced", "pending");
        tdb2.add_task(&mut t).unwrap();
        tdb2.sync_to_local(server.path().to_str().unwrap(), false).unwrap();
        assert_eq!(tdb2.num_local_changes().unwrap(), 0);

        let other = TempDir::new().unwrap();
        let mut tdb2b = open(&other);
        tdb2b.sync_to_local(server.path().to_str().unwrap(), false).unwrap();
        assert!(tdb2b.has(U1).unwrap());
    }

    #[test]
    fn status_from_string_values() {
        assert_eq!(status_from_string("pending").unwrap(), TcStatus::Pending);
        assert_eq!(status_from_string("completed").unwrap(), TcStatus::Completed);
        assert_eq!(status_from_string("deleted").unwrap(), TcStatus::Deleted);
        assert_eq!(status_from_string("recurring").unwrap(), TcStatus::Recurring);
        assert_eq!(status_from_string("x").unwrap_err().to_string(), "Unknown task status value 'x'.");
    }
}
