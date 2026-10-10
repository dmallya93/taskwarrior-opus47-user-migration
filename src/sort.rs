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
