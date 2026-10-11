//! Rust translation of `src/sort.cpp`.
//!
//! | C++                                                              | Rust                      |
//! |------------------------------------------------------------------|---------------------------|
//! | `sort_tasks (std::vector<Task>&, std::vector<int>& order, keys)` | [`sort_tasks`]            |
//! | `sort_projects (list<pair<string,int>>&, map<string,int>&)`      | [`sort_projects`]         |
//! | `sort_projects (list<pair<string,int>>&, map<string,bool>&)`     | [`sort_projects_bool`]    |
//! | `static bool sort_compare (int, int)`                            | `sort_compare` (private)  |
//! | `static global_data`, `global_keys`                              | [`SortData`] (explicit)   |
//! | `static unsigned int sort_random_seed`                           | thread-local, [`sort_random_seed`] / [`set_sort_random_seed`] |
//!
//! The C++ statics `global_data`/`global_keys` become the explicit
//! [`SortData`] comparator context; `Context::getContext ()` becomes the
//! `ctx` argument (urgency, `tdb2.id`, the column registry, config).
//!
//! **Algorithm.** C++ uses `std::stable_sort` with a `bool (int, int)`
//! "less" predicate. Some branches of `sort_compare` are not a strict weak
//! ordering (e.g. two different `recur` strings with the same duration are
//! equivalent and stop the key scan, while identical strings continue to the
//! next key), so different stable algorithms can disagree. To reproduce the
//! C++ result exactly, [`stable_sort`] is a line-by-line port of libstdc++'s
//! `std::stable_sort` (`__stable_sort_adaptive` with a `(n + 1) / 2`
//! temporary buffer, chunked insertion sort of 7, buffered merge loops and
//! `__merge_adaptive`), calling the predicate in the same order with the
//! same arguments.
//!
//! **`random`.** The per-task key is `std::to_string (std::hash<std::string>
//! {} (uuid + std::to_string (seed)))`, compared as strings; [`std_hash`] is
//! a port of libstdc++'s 64-bit `_Hash_bytes` (MurmurHash64A variant, seed
//! `0xc70f6907`), so `rc.debug.random.seed` reproduces the C++ order.
//!
//! **Deviation.** For a field that is neither built in nor a column, C++
//! `Context::getContext ().columns[field]` inserts a null entry into the
//! column map before throwing. The Rust registry cannot hold null columns, so
//! nothing is inserted; the error (and thus the aborted command) is the same.

use std::cell::Cell;
use std::collections::BTreeMap;

use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::duration::Duration;
use crate::shared::format::format;
use crate::shared::shared::split;
use crate::shared::timer::Timer;
use crate::task::Task;
use crate::util::extract_parents_default;

thread_local! {
    /// C++ `static unsigned int sort_random_seed = 0;`.
    static SORT_RANDOM_SEED: Cell<u32> = const { Cell::new(0) };
}

/// C++ `sort_random_seed` (read). 0 means "not yet chosen".
pub fn sort_random_seed() -> u32 {
    SORT_RANDOM_SEED.with(Cell::get)
}

/// C++ `sort_random_seed = value`.
pub fn set_sort_random_seed(value: u32) {
    SORT_RANDOM_SEED.with(|s| s.set(value));
}

/// The C++ statics `global_data` and `global_keys`, as an explicit
/// comparator context.
pub struct SortData<'a> {
    /// C++ `global_data`: the tasks the `order` indices refer to. Mutable
    /// because `Task::urgency` caches its value.
    pub data: &'a mut [Task],
    /// C++ `global_keys`: the comma-split sort keys (`project+`, `due-/`, ...).
    pub keys: Vec<String>,
    /// C++ `sort_random_seed` as chosen for this sort.
    pub seed: u32,
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void sort_tasks (std::vector<Task>& data, std::vector<int>& order,
/// const std::string& keys)`: stable-sorts the indices in `order` (into
/// `data`) by the comma-separated `keys`, each optionally suffixed `+`/`-`
/// (direction) and `/` (break indicator, ignored here). Adds the elapsed time
/// to `ctx.time_sort_us`.
///
/// The `random` seed is chosen once per thread (C++: per process): from
/// `rc.debug.random.seed` if set, otherwise randomly.
///
/// # Errors
/// - `The '<field>' column is not a valid sort field.` for an unknown key.
/// - `rc.debug.random.seed` that `std::stoul` rejects (C++
///   `std::invalid_argument`/`std::out_of_range`, reaching `catch (...)`):
///   [`TwError::Runtime`].
/// - Errors from `Task::urgency` and `TDB2::id` (C++ throws out of the
///   comparator). As in C++, `order` may then be partially permuted.
pub fn sort_tasks(ctx: &mut Context, data: &mut [Task], order: &mut [usize], keys: &str) -> Result<()> {
    let timer = Timer::new();

    // Split the key defs.
    let keys = split(keys, ',');

    // Generate a random seend for sorting by "random".
    if sort_random_seed() == 0 {
        // For testing purposes, allow the seed to be specified in an undocumented configuration
        // setting.
        let seed_str = ctx.config.get("debug.random.seed");
        if seed_str.is_empty() {
            set_sort_random_seed(random_device());
        } else {
            // `unsigned int = unsigned long`: truncation.
            set_sort_random_seed(cpp_stoul(&seed_str)? as u32);
        }
    }

    let mut sd = SortData { data, keys, seed: sort_random_seed() };

    // Only sort if necessary.
    if !order.is_empty() {
        let mut error: Option<TwError> = None;
        stable_sort(order, &mut |left, right| {
            if error.is_some() {
                return false;
            }
            match sort_compare(ctx, &mut sd, left, right) {
                Ok(less) => less,
                Err(e) => {
                    error = Some(e);
                    false
                }
            }
        });
        if let Some(e) = error {
            return Err(e);
        }
    }

    ctx.time_sort_us += timer.total_us() as i64;
    Ok(())
}

/// C++ `std::random_device rd; rd ()`: a random 32-bit value (here from the
/// standard library's randomly keyed hasher).
fn random_device() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(std::process::id().into());
    hasher.finish() as u32
}

/// C++ `std::stoul (str)` (base 10): leading whitespace, optional sign, then
/// digits, trailing text ignored; a `-` negates in unsigned arithmetic.
///
/// # Errors
/// [`TwError::Runtime`] with the libstdc++ `what ()` (`stoul`) when there
/// are no digits (`std::invalid_argument`) or the value overflows
/// (`std::out_of_range`).
fn cpp_stoul(text: &str) -> Result<u64> {
    let rest = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, digits) = match rest.as_bytes().first() {
        Some(b'-') => (true, &rest[1..]),
        Some(b'+') => (false, &rest[1..]),
        _ => (false, rest),
    };
    let digits: &str = &digits[..digits.bytes().take_while(u8::is_ascii_digit).count()];
    if digits.is_empty() {
        return Err(TwError::Runtime("stoul".to_string()));
    }
    let value: u64 = digits.parse().map_err(|_| TwError::Runtime("stoul".to_string()))?;
    Ok(if negative { value.wrapping_neg() } else { value })
}

/// libstdc++ `std::hash<std::string>{} (s)` on LP64:
/// `std::_Hash_bytes (s.data (), s.length (), 0xc70f6907)`.
pub fn std_hash(s: &str) -> u64 {
    hash_bytes(s.as_bytes(), 0xc70f_6907)
}

/// libstdc++ `std::_Hash_bytes` (`libsupc++/hash_bytes.cc`, 64-bit
/// `size_t`), little-endian unaligned loads.
fn hash_bytes(buf: &[u8], seed: u64) -> u64 {
    fn shift_mix(v: u64) -> u64 {
        v ^ (v >> 47)
    }
    fn load_bytes(p: &[u8]) -> u64 {
        let mut result: u64 = 0;
        for &b in p.iter().rev() {
            result = (result << 8).wrapping_add(u64::from(b));
        }
        result
    }

    let mul: u64 = (0xc6a4_a793_u64 << 32) + 0x5bd1_e995;
    let len = buf.len();
    let len_aligned = len & !0x7;
    let mut hash = seed ^ (len as u64).wrapping_mul(mul);
    for chunk in buf[..len_aligned].chunks_exact(8) {
        let word = u64::from_le_bytes(chunk.try_into().unwrap_or([0; 8]));
        let data = shift_mix(word.wrapping_mul(mul)).wrapping_mul(mul);
        hash ^= data;
        hash = hash.wrapping_mul(mul);
    }
    if len & 0x7 != 0 {
        let data = load_bytes(&buf[len_aligned..]);
        hash ^= data;
        hash = hash.wrapping_mul(mul);
    }
    hash = shift_mix(hash).wrapping_mul(mul);
    shift_mix(hash)
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void sort_projects (std::list<std::pair<std::string, int>>& sorted,
/// std::map<std::string, int>& allProjects)`: appends the projects in map
/// order to `sorted`, each subproject right after its latest parent; missing
/// parents are appended first with a count of 1.
pub fn sort_projects(sorted: &mut Vec<(String, i32)>, all_projects: &BTreeMap<String, i32>) {
    for (name, count) in all_projects {
        let parents = extract_parents_default(name);
        if !parents.is_empty() {
            // if parents exist: store iterator position of last parent
            // (`None` is the C++ `sorted.end ()`).
            let mut parent_pos: Option<usize> = None;
            for parent in &parents {
                parent_pos = sorted.iter().position(|item| item.0 == *parent);

                // if parent does not exist yet: insert into sorted view
                if parent_pos.is_none() {
                    sorted.push((parent.clone(), 1));
                }
            }

            // insert new element below latest parent
            match parent_pos {
                None => sorted.push((name.clone(), *count)),
                Some(pos) => sorted.insert(pos + 1, (name.clone(), *count)),
            }
        } else {
            // if has no parents: simply push to end of list
            sorted.push((name.clone(), *count));
        }
    }
}

/// C++ `void sort_projects (std::list<std::pair<std::string, int>>& sorted,
/// std::map<std::string, bool>& allProjects)`: as [`sort_projects`] with
/// the flags as 0/1 counts.
pub fn sort_projects_bool(sorted: &mut Vec<(String, i32)>, all_projects: &BTreeMap<String, bool>) {
    let all_projects_int: BTreeMap<String, i32> =
        all_projects.iter().map(|(name, flag)| (name.clone(), i32::from(*flag))).collect();

    sort_projects(sorted, &all_projects_int);
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `static bool sort_compare (int left, int right)`: "is `left` before
/// `right`" over the sort keys; a key on which the two tasks are equal moves
/// on to the next key, and `false` when all keys are equal.
fn sort_compare(ctx: &mut Context, sd: &mut SortData<'_>, left: usize, right: usize) -> Result<bool> {
    for k in &sd.keys {
        let (field, ascending, _break_indicator) = Context::decompose_sort_field(k);
        let field = field.as_str();

        // Random.
        if field == "random" {
            // For "random" sort, we produce a stable number for each task based on a hash of its
            // UUID plus the random seed.
            let seed = sd.seed.to_string();
            let left_scrambled = std_hash(&(sd.data[left].get("uuid") + &seed)).to_string();
            let right_scrambled = std_hash(&(sd.data[right].get("uuid") + &seed)).to_string();

            if left_scrambled == right_scrambled {
                continue;
            }

            return Ok(if ascending { left_scrambled < right_scrambled } else { left_scrambled > right_scrambled });
        }
        // Urgency.
        else if field == "urgency" {
            let left_real = sd.data[left].urgency(ctx)?;
            let right_real = sd.data[right].urgency(ctx)?;

            if left_real == right_real {
                continue;
            }

            return Ok(if ascending { left_real < right_real } else { left_real > right_real });
        }
        // Number.
        else if field == "id" {
            let left_number = sd.data[left].id;
            let right_number = sd.data[right].id;

            if left_number == right_number {
                continue;
            }

            return Ok(if ascending { left_number < right_number } else { left_number > right_number });
        }
        // String.
        else if matches!(field, "description" | "project" | "status" | "tags" | "uuid" | "parent" | "imask" | "mask")
        {
            let left_string = sd.data[left].get_ref(field);
            let right_string = sd.data[right].get_ref(field);

            if left_string == right_string {
                continue;
            }

            return Ok(if ascending { left_string < right_string } else { left_string > right_string });
        }
        // Due Date.
        else if matches!(field, "due" | "end" | "entry" | "start" | "until" | "wait" | "modified" | "scheduled") {
            match compare_dates(sd.data[left].get_ref(field), sd.data[right].get_ref(field), ascending) {
                Some(less) => return Ok(less),
                None => continue,
            }
        }
        // Depends string.
        else if field == "depends" {
            // Raw data is an un-sorted list of UUIDs.  We just need a stable
            // sort, so we sort them lexically.
            let mut left_deps = sd.data[left].get_dependency_uuids();
            left_deps.sort();
            let mut right_deps = sd.data[right].get_dependency_uuids();
            right_deps.sort();

            if left_deps == right_deps {
                continue;
            }

            if left_deps.is_empty() && !right_deps.is_empty() {
                return Ok(ascending);
            }

            if !left_deps.is_empty() && right_deps.is_empty() {
                return Ok(!ascending);
            }

            // Sort on the first dependency.
            let left_number = ctx.tdb2.id(&left_deps[0])?;
            let right_number = ctx.tdb2.id(&right_deps[0])?;

            if left_number == right_number {
                continue;
            }

            return Ok(if ascending { left_number < right_number } else { left_number > right_number });
        }
        // Duration.
        else if field == "recur" {
            match compare_durations(sd.data[left].get_ref(field), sd.data[right].get_ref(field), ascending) {
                Some(less) => return Ok(less),
                None => continue,
            }
        }
        // UDAs.
        else if let Some(column) = ctx.columns.get(field).cloned() {
            let type_ = column.type_();
            if type_ == "numeric" {
                let left_real = c_strtof(sd.data[left].get_ref(field));
                let right_real = c_strtof(sd.data[right].get_ref(field));

                if left_real == right_real {
                    continue;
                }

                return Ok(if ascending { left_real < right_real } else { left_real > right_real });
            } else if type_ == "string" {
                let left_string = sd.data[left].get_ref(field);
                let right_string = sd.data[right].get_ref(field);

                if left_string == right_string {
                    continue;
                }

                // UDAs of the type string can have custom sort orders, which need to be considered.
                let custom = Task::with_custom_order(|orders| {
                    orders.get(field).map(|order| {
                        // Guaranteed to be found, because of ColUDA::validate ().
                        // (`std::find` returns `end ()` when not found.)
                        let pos_left = order.iter().position(|v| v == left_string).unwrap_or(order.len());
                        let pos_right = order.iter().position(|v| v == right_string).unwrap_or(order.len());
                        if ascending {
                            pos_left < pos_right
                        } else {
                            pos_left > pos_right
                        }
                    })
                });
                if let Some(less) = custom {
                    return Ok(less);
                }

                // Empty values are unconditionally last, if no custom order was specified.
                if left_string.is_empty() {
                    return Ok(false);
                } else if right_string.is_empty() {
                    return Ok(true);
                }

                return Ok(if ascending { left_string < right_string } else { left_string > right_string });
            } else if type_ == "date" {
                match compare_dates(sd.data[left].get_ref(field), sd.data[right].get_ref(field), ascending) {
                    Some(less) => return Ok(less),
                    None => continue,
                }
            } else if type_ == "duration" {
                match compare_durations(sd.data[left].get_ref(field), sd.data[right].get_ref(field), ascending) {
                    Some(less) => return Ok(less),
                    None => continue,
                }
            }
            // Any other column type: C++ falls out of the if-chain and moves
            // on to the next key.
        } else {
            return Err(TwError::Message(format("The '{1}' column is not a valid sort field.", &[&field])));
        }
    }

    Ok(false)
}

/// The C++ date branches (built-in dates and `date` UDAs): set values sort
/// before unset ones regardless of direction, then the epoch strings compare
/// lexically. `None` is the C++ `continue` (equal).
fn compare_dates(left_string: &str, right_string: &str, ascending: bool) -> Option<bool> {
    if !left_string.is_empty() && right_string.is_empty() {
        return Some(true);
    }

    if left_string.is_empty() && !right_string.is_empty() {
        return Some(false);
    }

    if left_string == right_string {
        return None;
    }

    Some(if ascending { left_string < right_string } else { left_string > right_string })
}

/// The C++ duration branches (`recur` and `duration` UDAs): equal strings
/// `continue` (`None`); otherwise the parsed durations compare, so two
/// different strings with the same duration are equivalent without looking
/// at later keys.
fn compare_durations(left_string: &str, right_string: &str, ascending: bool) -> Option<bool> {
    if left_string == right_string {
        return None;
    }

    let left_duration = Duration::from_string(left_string);
    let right_duration = Duration::from_string(right_string);
    Some(if ascending { left_duration < right_duration } else { left_duration > right_duration })
}

/// C `strtof (s.c_str (), nullptr)` (the process never calls `setlocale`, so
/// the radix is `.`).
fn c_strtof(text: &str) -> f32 {
    let prefix = text.split('\0').next().unwrap_or("");
    let Ok(text) = std::ffi::CString::new(prefix) else {
        return 0.0;
    };
    // SAFETY: `text` is a valid NUL-terminated C string for the duration of
    // the call, and a null `endptr` is allowed.
    unsafe { libc::strtof(text.as_ptr(), std::ptr::null_mut()) }
}

////////////////////////////////////////////////////////////////////////////////
// libstdc++ `std::stable_sort`, ported for exact tie-break parity.

/// libstdc++ `std::_S_chunk_size`.
const CHUNK_SIZE: usize = 7;

/// libstdc++ `std::stable_sort (first, last, comp)` on `usize` values with
/// the "less" predicate `less`: `__stable_sort` allocating a `(len + 1) / 2`
/// temporary buffer, then `__stable_sort_adaptive`.
pub fn stable_sort(v: &mut [usize], less: &mut dyn FnMut(usize, usize) -> bool) {
    let len = v.len();
    if len == 0 {
        return;
    }
    let half = len.div_ceil(2);
    let mut buffer = vec![0usize; half];

    // __stable_sort_adaptive (first, first + buf.size (), last, buffer, comp)
    merge_sort_with_buffer(&mut v[..half], &mut buffer, less);
    merge_sort_with_buffer(&mut v[half..], &mut buffer, less);
    merge_adaptive(v, half, &mut buffer, less);
}

/// libstdc++ `__insertion_sort` (with `__unguarded_linear_insert`).
fn insertion_sort(v: &mut [usize], less: &mut dyn FnMut(usize, usize) -> bool) {
    for i in 1..v.len() {
        let val = v[i];
        if less(val, v[0]) {
            v.copy_within(0..i, 1);
            v[0] = val;
        } else {
            let mut last = i;
            while less(val, v[last - 1]) {
                v[last] = v[last - 1];
                last -= 1;
            }
            v[last] = val;
        }
    }
}

/// libstdc++ `__chunk_insertion_sort`.
fn chunk_insertion_sort(v: &mut [usize], chunk_size: usize, less: &mut dyn FnMut(usize, usize) -> bool) {
    let mut first = 0;
    while v.len() - first >= chunk_size {
        insertion_sort(&mut v[first..first + chunk_size], less);
        first += chunk_size;
    }
    insertion_sort(&mut v[first..], less);
}

/// libstdc++ `__move_merge`: merges `a` and `b` into `out`, taking from `b`
/// only when `less (b, a)`.
fn move_merge(a: &[usize], b: &[usize], out: &mut [usize], less: &mut dyn FnMut(usize, usize) -> bool) {
    let (mut i, mut j, mut o) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if less(b[j], a[i]) {
            out[o] = b[j];
            j += 1;
        } else {
            out[o] = a[i];
            i += 1;
        }
        o += 1;
    }
    let rest_a = a.len() - i;
    out[o..o + rest_a].copy_from_slice(&a[i..]);
    o += rest_a;
    out[o..o + (b.len() - j)].copy_from_slice(&b[j..]);
}

/// libstdc++ `__merge_sort_loop`: merges runs of `step` from `src` into
/// `dst`.
fn merge_sort_loop(src: &[usize], dst: &mut [usize], step: usize, less: &mut dyn FnMut(usize, usize) -> bool) {
    let len = src.len();
    let two_step = 2 * step;
    let mut first = 0;
    while len - first >= two_step {
        move_merge(
            &src[first..first + step],
            &src[first + step..first + two_step],
            &mut dst[first..first + two_step],
            less,
        );
        first += two_step;
    }

    let step = (len - first).min(step);
    move_merge(&src[first..first + step], &src[first + step..], &mut dst[first..], less);
}

/// libstdc++ `__merge_sort_with_buffer`.
fn merge_sort_with_buffer(v: &mut [usize], buffer: &mut [usize], less: &mut dyn FnMut(usize, usize) -> bool) {
    let len = v.len();
    let buffer = &mut buffer[..len];
    let mut step = CHUNK_SIZE;
    chunk_insertion_sort(v, step, less);

    while step < len {
        merge_sort_loop(v, buffer, step, less);
        step *= 2;
        merge_sort_loop(buffer, v, step, less);
        step *= 2;
    }
}

/// libstdc++ `__merge_adaptive` when the buffer holds the shorter run:
/// `__move_merge_adaptive` (first run buffered) or
/// `__move_merge_adaptive_backward` (second run buffered).
fn merge_adaptive(v: &mut [usize], middle: usize, buffer: &mut [usize], less: &mut dyn FnMut(usize, usize) -> bool) {
    let len = v.len();
    let len1 = middle;
    let len2 = len - middle;
    if len1 <= len2 {
        buffer[..len1].copy_from_slice(&v[..middle]);

        // __move_merge_adaptive (buffer, buffer_end, middle, last, first)
        let (mut i, mut j, mut o) = (0, middle, 0);
        while i < len1 && j < len {
            if less(v[j], buffer[i]) {
                v[o] = v[j];
                j += 1;
            } else {
                v[o] = buffer[i];
                i += 1;
            }
            o += 1;
        }
        if i < len1 {
            v[o..o + (len1 - i)].copy_from_slice(&buffer[i..len1]);
        }
    } else {
        buffer[..len2].copy_from_slice(&v[middle..]);

        // __move_merge_adaptive_backward (first, middle, buffer, buffer_end, last)
        if len1 == 0 {
            v[len - len2..].copy_from_slice(&buffer[..len2]);
            return;
        } else if len2 == 0 {
            return;
        }

        let mut last1 = middle - 1;
        let mut last2 = len2 - 1;
        let mut result = len;
        loop {
            if less(buffer[last2], v[last1]) {
                result -= 1;
                v[result] = v[last1];
                if last1 == 0 {
                    v[result - (last2 + 1)..result].copy_from_slice(&buffer[..=last2]);
                    return;
                }
                last1 -= 1;
            } else {
                result -= 1;
                v[result] = buffer[last2];
                if last2 == 0 {
                    return;
                }
                last2 -= 1;
            }
        }
    }
}

#[cfg(test)]
#[path = "sort_tests.rs"]
mod tests;

#[cfg(test)]
mod report_sort_tests {
    //! `sort_tasks` driven the way `CmdCustom::execute` drives it, checked
    //! against the semantics of C++ `sort.cpp` (`sort_compare`):
    //!
    //! 1. tasks are stored through `tdb2::add` in a real data directory;
    //! 2. `report.r.sort` is read from config and split, and every key goes
    //!    through `legacySortColumnMap`. As in C++, the *unvalidated* string
    //!    is still what `sort_tasks` receives;
    //! 3. `Filter::subset` (no filter: every task) selects the data;
    //! 4. the sequence `0..n` is sorted by `sort_tasks`.
    //!
    //! Expectations come from the C++ comparator: date epoch strings compare
    //! lexically, and unset dates are last in both directions. Strings compare
    //! bytewise. Durations compare parsed. `uda.X.values` is reversed into
    //! `Task::customOrder`, with values missing from it at `end ()`. A string
    //! UDA without values puts empties last in both directions. Depends sorts
    //! on the ID of the lexically first dependency, and `random` hashes uuid
    //! plus seed.
    use super::*;
    use crate::cli2;
    use crate::context::set_stdout_tty_override;
    use crate::filter::Filter;
    use crate::legacy::legacy_sort_column_map;
    use crate::test_util::{env_lock, EnvGuard};
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    /// Restores the CLI2 abbreviation length and the `random` seed that the
    /// tests (and `Context::initialize`) change.
    struct Statics(i32);

    impl Drop for Statics {
        fn drop(&mut self) {
            cli2::set_minimum_match_length(self.0);
            set_sort_random_seed(0);
            set_stdout_tty_override(None);
        }
    }

    /// Fields drop in declaration order: Context and data directory, then
    /// statics and environment, the lock last.
    struct Report {
        ctx: Context,
        _dir: TempDir,
        _statics: Statics,
        _env: EnvGuard,
        _lock: MutexGuard<'static, ()>,
    }

    fn initialized(list: &[&str]) -> Context {
        let args: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        let mut ctx = Context::new();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let rc = ctx.initialize_with(&args, &mut out, &mut err).unwrap();
        assert_eq!(rc, 0, "{}", String::from_utf8_lossy(&err));
        ctx
    }

    impl Report {
        /// `task <rc...> count` on a data directory holding the `seed` JSON
        /// tasks (added in order, so pending tasks get IDs 1, 2, ...).
        fn new(rc: &[&str], seed: &[&str]) -> Self {
            let lock = env_lock();
            let env = EnvGuard::save(&["HOME", "TASKRC", "TASKDATA", "XDG_CONFIG_HOME"]);
            let statics = Statics(cli2::minimum_match_length());
            set_sort_random_seed(0);
            let dir = TempDir::new().unwrap();
            let base = dir.path().to_str().unwrap().to_string();
            let home = format!("{base}/home");
            std::fs::create_dir(&home).unwrap();
            let taskrc = format!("{base}/taskrc");
            std::fs::write(&taskrc, "").unwrap();
            std::env::set_var("HOME", &home);
            std::env::set_var("TASKRC", &taskrc);
            std::env::set_var("TASKDATA", format!("{base}/data"));
            std::env::remove_var("XDG_CONFIG_HOME");
            set_stdout_tty_override(Some(false));

            let mut full = vec!["task"];
            full.extend_from_slice(rc);
            full.push("count");
            if !seed.is_empty() {
                let mut seeder = initialized(&full);
                for json in seed {
                    let mut task = Task::from_input(&mut seeder, json).unwrap();
                    crate::tdb2::add(&mut seeder, &mut task).unwrap();
                }
            }

            let ctx = initialized(&full);
            Report { ctx, _dir: dir, _statics: statics, _env: env, _lock: lock }
        }

        /// CmdCustom steps 3: `Filter::subset` with no filter.
        fn load(&mut self) -> Vec<Task> {
            let mut filtered = Vec::new();
            Filter::new().subset(&mut self.ctx, &mut filtered).unwrap();
            filtered
        }

        /// CmdCustom steps 2 and 4 on `filtered` with `rc.report.r.sort` =
        /// `sort`: the resulting sequence.
        fn sequence(&mut self, filtered: &mut [Task], sort: &str) -> Result<Vec<usize>> {
            self.ctx.config.set("report.r.sort", sort);
            let report_sort = self.ctx.config.get("report.r.sort");
            let mut sort_order = split(&report_sort, ',');
            if !sort_order.is_empty() && sort_order[0] != "none" {
                for column in sort_order.iter_mut() {
                    legacy_sort_column_map(&mut self.ctx, column);
                }
            }
            let mut sequence: Vec<usize> = (0..filtered.len()).collect();
            if !sort_order.is_empty() {
                sort_tasks(&mut self.ctx, filtered, &mut sequence, &report_sort)?;
            }
            Ok(sequence)
        }

        /// Descriptions of the sorted `filtered` tasks.
        fn sorted(&mut self, filtered: &mut [Task], sort: &str) -> Vec<String> {
            let sequence = self.sequence(filtered, sort).unwrap();
            sequence.iter().map(|&i| filtered[i].get("description")).collect()
        }

        /// The full CmdCustom path: load, then sort.
        fn report(&mut self, sort: &str) -> Vec<String> {
            let mut filtered = self.load();
            self.sorted(&mut filtered, sort)
        }

        fn report_err(&mut self, sort: &str) -> String {
            let mut filtered = self.load();
            match self.sequence(&mut filtered, sort) {
                Ok(s) => panic!("expected an error, got {s:?}"),
                Err(e) => e.to_string(),
            }
        }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn reversed(list: &[&str]) -> Vec<String> {
        list.iter().rev().map(|s| s.to_string()).collect()
    }

    const ENTRY: &str = r#""entry":"20210101T000000Z""#;

    #[test]
    fn natural_id_order() {
        let mut seed: Vec<String> = (1..=12)
            .map(|i| format!(r#"{{"description":"t{i}","status":"pending",{ENTRY}}}"#))
            .collect();
        seed.push(format!(r#"{{"description":"done","status":"completed","end":"20210102T000000Z",{ENTRY}}}"#));
        let seed: Vec<&str> = seed.iter().map(String::as_str).collect();
        let mut r = Report::new(&[], &seed);
        let mut filtered = r.load();
        let ids = |r: &mut Report, filtered: &mut [Task], sort: &str| -> Vec<i32> {
            let sequence = r.sequence(filtered, sort).unwrap();
            sequence.iter().map(|&i| filtered[i].id).collect()
        };
        // Numeric, not lexical ("10" after "9"); completed tasks have ID 0.
        let ascending: Vec<i32> = (0..=12).collect();
        assert_eq!(ids(&mut r, &mut filtered, "id"), ascending);
        assert_eq!(ids(&mut r, &mut filtered, "id+"), ascending);
        assert_eq!(ids(&mut r, &mut filtered, "id+/"), ascending);
        let descending: Vec<i32> = (0..=12).rev().collect();
        assert_eq!(ids(&mut r, &mut filtered, "id-"), descending);
        assert_eq!(ids(&mut r, &mut filtered, "id-/"), descending);
        // IDs follow the task descriptions t1..t12.
        let sorted = r.sorted(&mut filtered, "id+");
        assert_eq!(sorted[0], "done");
        assert_eq!(sorted[9], "t9");
        assert_eq!(sorted[10], "t10");
        assert!(r.ctx.time_sort_us >= 0);
    }

    #[test]
    fn uuid_and_string_attributes() {
        let mut r = Report::new(
            &[],
            &[
                r#"{"uuid":"eeeeeeee-0000-4000-8000-000000000003","description":"a","status":"pending","project":"work","tags":["x"],"entry":"20210101T000000Z"}"#,
                r#"{"uuid":"eeeeeeee-0000-4000-8000-000000000001","description":"B","status":"pending","project":"Home","tags":["b","a"],"entry":"20210101T000000Z"}"#,
                r#"{"uuid":"eeeeeeee-0000-4000-8000-000000000002","description":"c","status":"completed","end":"20210102T000000Z","entry":"20210101T000000Z"}"#,
                r#"{"uuid":"eeeeeeee-0000-4000-8000-000000000004","description":"d","status":"deleted","project":"home","end":"20210102T000000Z","entry":"20210101T000000Z"}"#,
            ],
        );
        assert_eq!(r.report("uuid+"), names(&["B", "c", "a", "d"]));
        assert_eq!(r.report("uuid-"), names(&["d", "a", "c", "B"]));
        // Bytewise: "" < "Home" < "home" < "work".
        assert_eq!(r.report("project+"), names(&["c", "B", "d", "a"]));
        assert_eq!(r.report("project-"), names(&["a", "d", "B", "c"]));
        assert_eq!(r.report("description"), names(&["B", "a", "c", "d"]));
        assert_eq!(r.report("description-"), names(&["d", "c", "a", "B"]));
        // completed < deleted < pending, ties on the next key.
        assert_eq!(r.report("status+,description+"), names(&["c", "d", "B", "a"]));
        assert_eq!(r.report("status-,description-"), names(&["a", "B", "d", "c"]));
        // The "tags" attribute is rebuilt in key order ("a,b").
        assert_eq!(r.report("tags+,description+"), names(&["c", "d", "B", "a"]));
        assert_eq!(r.report("tags-,description+"), names(&["a", "B", "c", "d"]));
    }

    #[test]
    fn recurrence_attributes_parent_imask_mask_recur() {
        const A: &str = "aaaaaaaa-0000-4000-8000-000000000000";
        const B: &str = "bbbbbbbb-0000-4000-8000-000000000000";
        let child = |d: &str, parent: &str, imask: &str, recur: &str| {
            format!(
                r#"{{"description":"{d}","status":"pending","parent":"{parent}","imask":{imask},"recur":"{recur}","due":"20210105T000000Z",{ENTRY}}}"#
            )
        };
        let seed = [
            format!(
                r#"{{"uuid":"{A}","description":"tmplA","status":"recurring","recur":"weekly","mask":"-","due":"20210105T000000Z",{ENTRY}}}"#
            ),
            format!(
                r#"{{"uuid":"{B}","description":"tmplB","status":"recurring","recur":"2d","mask":"+--","due":"20210105T000000Z",{ENTRY}}}"#
            ),
            child("c1", A, "1", "weekly"),
            child("c10", B, "10", "2d"),
            child("c2", A, "2", "weekly"),
            format!(r#"{{"description":"plain","status":"pending",{ENTRY}}}"#),
        ];
        let seed: Vec<&str> = seed.iter().map(String::as_str).collect();
        let mut r = Report::new(&[], &seed);
        // imask is in the string branch: "1" < "10" < "2"; "" first.
        assert_eq!(r.report("imask+,description+"), names(&["plain", "tmplA", "tmplB", "c1", "c10", "c2"]));
        assert_eq!(r.report("imask-,description+"), names(&["c2", "c10", "c1", "plain", "tmplA", "tmplB"]));
        assert_eq!(r.report("parent+,description+"), names(&["plain", "tmplA", "tmplB", "c1", "c2", "c10"]));
        assert_eq!(r.report("parent-,description+"), names(&["c10", "c1", "c2", "plain", "tmplA", "tmplB"]));
        // '+' (43) < '-' (45).
        assert_eq!(r.report("mask+,description+"), names(&["c1", "c10", "c2", "plain", "tmplB", "tmplA"]));
        // recur: parsed durations (unset = 0); identical strings go on to the
        // next key.
        assert_eq!(r.report("recur+,description+"), names(&["plain", "c10", "tmplB", "c1", "c2", "tmplA"]));
        assert_eq!(r.report("recur-,description+"), names(&["c1", "c2", "tmplA", "c10", "tmplB", "plain"]));
    }

    /// Every built-in date attribute plus a date UDA, as stored epochs.
    /// `early`/`late` are completed: C++ `Task::validate` removes `end` from
    /// pending tasks.
    #[test]
    fn date_attributes_lexical_epochs_and_missing_last() {
        const FIELDS: [&str; 7] = ["due", "end", "start", "until", "wait", "scheduled", "when"];
        let all_dates = |d: &str, iso: &str, entry: &str| {
            let dates: Vec<String> = FIELDS.iter().map(|f| format!(r#""{f}":"{iso}""#)).collect();
            format!(r#"{{"description":"{d}","status":"completed",{},"entry":"{entry}"}}"#, dates.join(","))
        };
        let seed = [
            all_dates("late", "20210102T000000Z", "20210102T000000Z"),
            all_dates("early", "20210101T000000Z", "20210101T000000Z"),
            // 999999999 and 1000000000: lexically reversed.
            r#"{"description":"lex9","status":"pending","due":"20010909T014639Z","when":"20010909T014639Z","entry":"20210103T000000Z"}"#.to_string(),
            r#"{"description":"lex10","status":"pending","due":"20010909T014640Z","when":"20010909T014640Z","entry":"20210103T000000Z"}"#.to_string(),
            // No dates; entry defaults to now.
            r#"{"description":"none","status":"pending"}"#.to_string(),
        ];
        let seed: Vec<&str> = seed.iter().map(String::as_str).collect();
        let mut r = Report::new(&["rc.uda.when.type:date"], &seed);

        for field in ["due", "when"] {
            assert_eq!(
                r.report(&format!("{field}+,description+")),
                names(&["lex10", "early", "late", "lex9", "none"]),
                "{field}"
            );
            assert_eq!(
                r.report(&format!("{field}-,description+")),
                names(&["lex9", "late", "early", "lex10", "none"]),
                "{field}"
            );
        }
        for field in ["end", "start", "until", "wait", "scheduled"] {
            assert_eq!(
                r.report(&format!("{field}+,description+")),
                names(&["early", "late", "lex10", "lex9", "none"]),
                "{field}"
            );
            assert_eq!(
                r.report(&format!("{field}-,description+")),
                names(&["late", "early", "lex10", "lex9", "none"]),
                "{field}"
            );
            // Two unset values are equal: the next key decides.
            assert_eq!(
                r.report(&format!("{field}-,description-")),
                names(&["late", "early", "none", "lex9", "lex10"]),
                "{field}"
            );
        }
        assert_eq!(r.report("entry+,description+"), names(&["early", "late", "lex10", "lex9", "none"]));
        assert_eq!(r.report("entry-,description+"), names(&["none", "lex10", "lex9", "late", "early"]));

        // `modified` is stamped by tdb2::add; set it on the loaded tasks
        // (CmdCustom sorts exactly what Filter::subset returned).
        let mut filtered = r.load();
        for task in filtered.iter_mut() {
            match task.get("description").as_str() {
                "early" => task.set("modified", "1609459200"),
                "late" => task.set("modified", "1609545600"),
                "lex9" => task.set("modified", "999999999"),
                "lex10" => task.set("modified", "1000000000"),
                _ => task.remove("modified"),
            }
        }
        assert_eq!(r.sorted(&mut filtered, "modified+,description+"), names(&["lex10", "early", "late", "lex9", "none"]));
        assert_eq!(r.sorted(&mut filtered, "modified-,description+"), names(&["lex9", "late", "early", "lex10", "none"]));
    }

    #[test]
    fn depends_first_dependency_id() {
        const U1: &str = "cccccccc-0000-4000-8000-000000000001";
        const U2: &str = "cccccccc-0000-4000-8000-000000000002";
        const U3: &str = "cccccccc-0000-4000-8000-000000000003";
        let plain = |d: &str, u: &str| format!(r#"{{"uuid":"{u}","description":"{d}","status":"pending",{ENTRY}}}"#);
        let dep = |d: &str, uuids: &[&str]| {
            let list: Vec<String> = uuids.iter().map(|u| format!("\"{u}\"")).collect();
            format!(r#"{{"description":"{d}","status":"pending","depends":[{}],{ENTRY}}}"#, list.join(","))
        };
        let seed = [
            plain("u1", U1),
            plain("u2", U2),
            plain("u3", U3),
            dep("on3", &[U3]),
            // Lexically first dependency: U1.
            dep("on2+1", &[U2, U1]),
            plain("none", "cccccccc-0000-4000-8000-0000000000ff"),
            dep("on1", &[U1]),
        ];
        let seed: Vec<&str> = seed.iter().map(String::as_str).collect();
        let mut r = Report::new(&[], &seed);
        assert_eq!(r.ctx.tdb2.id(U1).unwrap(), 1);
        assert_eq!(r.ctx.tdb2.id(U3).unwrap(), 3);
        // Ascending: no dependencies first; on1 and on2+1 tie on ID 1.
        assert_eq!(
            r.report("depends+,description+"),
            names(&["none", "u1", "u2", "u3", "on1", "on2+1", "on3"])
        );
        assert_eq!(
            r.report("depends-,description-"),
            names(&["on3", "on2+1", "on1", "u3", "u2", "u1", "none"])
        );
        // Legacy break marker on depends.
        assert_eq!(
            r.report("depends-/,description-"),
            names(&["on3", "on2+1", "on1", "u3", "u2", "u1", "none"])
        );
    }

    #[test]
    fn urgency_computed_per_task() {
        let mut r = Report::new(
            &[],
            &[
                r#"{"description":"plain","status":"pending"}"#,
                r#"{"description":"next","status":"pending","tags":["next"]}"#,
                r#"{"description":"proj","status":"pending","project":"p"}"#,
            ],
        );
        assert_eq!(r.report("urgency-"), names(&["next", "proj", "plain"]));
        assert_eq!(r.report("urgency+"), names(&["plain", "proj", "next"]));
        assert_eq!(r.report("urgency"), names(&["plain", "proj", "next"]));
    }

    #[test]
    fn break_suffix_is_stripped_and_quirks() {
        let mut r = Report::new(
            &[],
            &[
                r#"{"description":"w2","status":"pending","project":"work"}"#,
                r#"{"description":"h1","status":"pending","project":"home"}"#,
                r#"{"description":"w1","status":"pending","project":"work"}"#,
                r#"{"description":"n","status":"pending"}"#,
            ],
        );
        let plain = r.report("project+,description-");
        assert_eq!(plain, names(&["n", "h1", "w2", "w1"]));
        assert_eq!(r.report("project+/,description-"), plain);
        assert_eq!(r.report("project+/,description-/"), plain);
        assert_eq!(r.report("project-/,description+"), names(&["w1", "w2", "h1", "n"]));

        // C++ decomposeSortField: '/' without '+'/'-' stays part of the key.
        assert_eq!(r.report_err("project/"), "The 'project/' column is not a valid sort field.");
        assert_eq!(r.report_err("bogus+"), "The 'bogus' column is not a valid sort field.");
        // Reached only when earlier keys tie.
        assert_eq!(r.report_err("project,bogus-"), "The 'bogus' column is not a valid sort field.");
        // The legacy map does not rewrite the string passed to sort_tasks
        // (and removed names are not mapped at all).
        assert_eq!(r.report_err("entry_time"), "The 'entry_time' column is not a valid sort field.");
        assert!(!r.ctx.columns.contains_key("bogus"));
    }

    const UDA_RC: &[&str] = &[
        "rc.uda.size.type:string",
        "rc.uda.size.values:H,M,L",
        "rc.uda.sizee.type:string",
        "rc.uda.sizee.values:H,M,L,",
        "rc.uda.plain.type:string",
        "rc.uda.est.type:numeric",
        "rc.uda.len.type:duration",
    ];

    #[test]
    fn string_uda_with_values() {
        let mut r = Report::new(
            UDA_RC,
            &[
                r#"{"description":"m","status":"pending","size":"M","sizee":"M"}"#,
                r#"{"description":"none","status":"pending"}"#,
                r#"{"description":"h","status":"pending","size":"H","sizee":"H"}"#,
                r#"{"description":"l","status":"pending","size":"L","sizee":"L"}"#,
            ],
        );
        // `Task::customOrder` is appended to by every Context initialization
        // on the thread (C++: once per process; the seeding Context ran it
        // too). `std::find` takes the first match, so only the prefix counts.
        assert!(Task::custom_order().get("size").unwrap().starts_with(&names(&["L", "M", "H"])));
        assert!(Task::custom_order().get("sizee").unwrap().starts_with(&names(&["", "L", "M", "H"])));
        // customOrder is the reversed value list; "" is not in it (end ()).
        assert_eq!(r.report("size+"), names(&["l", "m", "h", "none"]));
        assert_eq!(r.report("size-"), names(&["none", "h", "m", "l"]));
        // A trailing comma puts "" first in customOrder.
        assert_eq!(r.report("sizee+"), names(&["none", "l", "m", "h"]));
        assert_eq!(r.report("sizee-"), names(&["h", "m", "l", "none"]));
    }

    #[test]
    fn string_uda_without_values() {
        let mut r = Report::new(
            UDA_RC,
            &[
                r#"{"description":"none1","status":"pending"}"#,
                r#"{"description":"b","status":"pending","plain":"b"}"#,
                r#"{"description":"none2","status":"pending"}"#,
                r#"{"description":"a","status":"pending","plain":"a"}"#,
            ],
        );
        assert!(!Task::custom_order().contains_key("plain"));
        // Empty values are last in both directions; two empties tie.
        assert_eq!(r.report("plain+,description-"), names(&["a", "b", "none2", "none1"]));
        assert_eq!(r.report("plain-,description+"), names(&["b", "a", "none1", "none2"]));
    }

    #[test]
    fn numeric_and_duration_udas() {
        let mut r = Report::new(
            UDA_RC,
            &[
                r#"{"description":"ten","status":"pending","est":10,"len":"P7D"}"#,
                r#"{"description":"ninehalf","status":"pending","est":9.5,"len":"P2D"}"#,
                r#"{"description":"none","status":"pending"}"#,
                r#"{"description":"neg","status":"pending","est":-1,"len":"PT36H"}"#,
            ],
        );
        // strtof: unset is 0.
        assert_eq!(r.report("est+"), names(&["neg", "none", "ninehalf", "ten"]));
        assert_eq!(r.report("est-"), names(&["ten", "ninehalf", "none", "neg"]));
        // Parsed durations, not strings ("P2D" < "P7D" < "PT36H").
        assert_eq!(r.report("len+"), names(&["none", "neg", "ninehalf", "ten"]));
        assert_eq!(r.report("len-"), names(&["ten", "ninehalf", "neg", "none"]));
    }

    #[test]
    fn random_is_deterministic_with_seed() {
        let uuids: Vec<String> = (0..10).map(|i| format!("dddddddd-0000-4000-8000-{i:012}")).collect();
        let seed: Vec<String> = uuids
            .iter()
            .map(|u| format!(r#"{{"uuid":"{u}","description":"{u}","status":"pending",{ENTRY}}}"#))
            .collect();
        let seed: Vec<&str> = seed.iter().map(String::as_str).collect();
        let mut r = Report::new(&["rc.debug.random.seed:1234"], &seed);
        let mut expected = uuids.clone();
        expected.sort_by_key(|u| std_hash(&format!("{u}1234")).to_string());
        let first = r.report("random");
        assert_eq!(first, expected);
        assert_eq!(sort_random_seed(), 1234);
        assert_eq!(r.report("random+"), first);
        let desc: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_eq!(r.report("random-"), reversed(&desc));
        // Not the uuid order.
        assert_ne!(first, uuids);
    }

    #[test]
    fn sort_projects_from_report_data() {
        let mut r = Report::new(
            &[],
            &[
                r#"{"description":"1","status":"pending","project":"home.garden"}"#,
                r#"{"description":"2","status":"pending","project":"home.garden"}"#,
                r#"{"description":"3","status":"pending","project":"home.kitchen"}"#,
                r#"{"description":"4","status":"pending","project":"work.alpha.x"}"#,
                r#"{"description":"5","status":"pending","project":"home"}"#,
                r#"{"description":"6","status":"pending"}"#,
            ],
        );
        let filtered = r.load();
        // CmdProjects: unique[project] += 1.
        let mut unique: BTreeMap<String, i32> = BTreeMap::new();
        for task in &filtered {
            *unique.entry(task.get("project")).or_insert(0) += 1;
        }
        let mut sorted = Vec::new();
        sort_projects(&mut sorted, &unique);
        let pairs = |l: &[(&str, i32)]| -> Vec<(String, i32)> { l.iter().map(|(n, c)| (n.to_string(), *c)).collect() };
        assert_eq!(
            sorted,
            pairs(&[
                ("", 1),
                ("home", 1),
                ("home.kitchen", 1),
                ("home.garden", 2),
                ("work", 1),
                ("work.alpha", 1),
                ("work.alpha.x", 1)
            ])
        );

        // CmdSummary: allProjects[project] = false; missing parents get 1.
        let mut all: BTreeMap<String, bool> = BTreeMap::new();
        for task in &filtered {
            if !task.get("project").is_empty() && task.get("project") != "home" {
                all.insert(task.get("project"), false);
            }
        }
        let mut sorted = Vec::new();
        sort_projects_bool(&mut sorted, &all);
        assert_eq!(
            sorted,
            pairs(&[
                ("home", 1),
                ("home.kitchen", 0),
                ("home.garden", 0),
                ("work", 1),
                ("work.alpha", 1),
                ("work.alpha.x", 0)
            ])
        );
    }
}
