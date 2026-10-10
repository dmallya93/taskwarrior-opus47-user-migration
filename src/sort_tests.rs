//! Unit tests for [`super`]: `sort_tasks` over every `sort_compare` branch
//! (id, urgency, the string attributes, the built-in dates, depends, recur,
//! numeric/string/date/duration UDAs with and without `uda.<name>.values`,
//! `random`), mixed and decorated keys, the error paths, the libstdc++
//! `std::stable_sort` and `std::hash` ports (known answers produced by
//! g++ 11.4), and both `sort_projects` overloads.

use super::*;
use crate::cli2;
use crate::context::set_stdout_tty_override;
use crate::test_util::{env_lock, EnvGuard};
use std::sync::MutexGuard;
use tempfile::TempDir;

/// Restores the process-wide CLI2 abbreviation length that
/// `Context::initialize` sets.
struct MinimumMatchLength(i32);

impl Drop for MinimumMatchLength {
    fn drop(&mut self) {
        cli2::set_minimum_match_length(self.0);
    }
}

/// Fields drop in declaration order: the Context and data directory first,
/// then the statics and environment are restored, the lock goes last.
struct Fixture {
    ctx: Context,
    _dir: TempDir,
    _mml: MinimumMatchLength,
    _env: EnvGuard,
    _lock: MutexGuard<'static, ()>,
}

const UDAS: &[&str] = &[
    "rc.uda.size.type:string",
    "rc.uda.size.values:S,M,L",
    "rc.uda.plain.type:string",
    "rc.uda.est.type:numeric",
    "rc.uda.when.type:date",
    "rc.uda.len.type:duration",
    "rc.uda.ref.type:uuid",
];

fn initialized(list: &[&str]) -> Context {
    let args: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    let mut ctx = Context::new();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let rc = ctx.initialize_with(&args, &mut out, &mut err).unwrap();
    assert_eq!(rc, 0, "{}", String::from_utf8_lossy(&err));
    ctx
}

impl Fixture {
    /// A Context for `task <UDAS> <list...> count` on a data directory
    /// seeded with the `seed` JSON tasks.
    fn new(list: &[&str], seed: &[&str]) -> Self {
        let lock = env_lock();
        let env = EnvGuard::save(&["HOME", "TASKRC", "TASKDATA", "XDG_CONFIG_HOME"]);
        let mml = MinimumMatchLength(cli2::minimum_match_length());
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let home = format!("{base}/home");
        std::fs::create_dir(&home).unwrap();
        let rc = format!("{base}/taskrc");
        std::fs::write(&rc, "").unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("TASKRC", &rc);
        std::env::set_var("TASKDATA", format!("{base}/data"));
        std::env::remove_var("XDG_CONFIG_HOME");
        set_stdout_tty_override(Some(false));

        let mut full = vec!["task"];
        full.extend_from_slice(UDAS);
        full.extend_from_slice(list);
        full.push("count");
        if !seed.is_empty() {
            let mut seeder = initialized(&full);
            for json in seed {
                let mut task = Task::from_input(&mut seeder, json).unwrap();
                crate::tdb2::add(&mut seeder, &mut task).unwrap();
            }
        }

        let ctx = initialized(&full);
        Fixture { ctx, _dir: dir, _mml: mml, _env: env, _lock: lock }
    }

    /// `sort_tasks` over all of `data`, returning the descriptions in order.
    fn sorted(&mut self, data: &mut [Task], keys: &str) -> Vec<String> {
        let mut order: Vec<usize> = (0..data.len()).collect();
        sort_tasks(&mut self.ctx, data, &mut order, keys).unwrap();
        order.iter().map(|&i| data[i].get("description")).collect()
    }
}

/// A task with `description` plus the given attributes.
fn task(description: &str, attributes: &[(&str, &str)]) -> Task {
    let mut t = Task::new();
    t.set("description", description);
    for (name, value) in attributes {
        t.set(name, value);
    }
    t
}

fn with_id(mut t: Task, id: i32) -> Task {
    t.id = id;
    t
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

////////////////////////////////////////////////////////////////////////////////
// Built-in fields.

#[test]
fn id_both_directions_and_default_ascending() {
    let mut f = Fixture::new(&[], &[]);
    let mut data = vec![with_id(task("c", &[]), 3), with_id(task("a", &[]), 1), with_id(task("b", &[]), 2)];
    assert_eq!(f.sorted(&mut data, "id"), names(&["a", "b", "c"]));
    assert_eq!(f.sorted(&mut data, "id+"), names(&["a", "b", "c"]));
    assert_eq!(f.sorted(&mut data, "id-"), names(&["c", "b", "a"]));
    // The break indicator is accepted and ignored.
    assert_eq!(f.sorted(&mut data, "id-/"), names(&["c", "b", "a"]));
    assert!(f.ctx.time_sort_us >= 0);
}

#[test]
fn string_fields_compare_bytewise() {
    let mut f = Fixture::new(&[], &[]);
    for field in ["project", "status", "tags", "uuid", "parent", "imask", "mask"] {
        let mut data = vec![
            task("1", &[(field, "beta")]),
            task("2", &[(field, "Zed")]),
            task("3", &[(field, "alpha")]),
            task("4", &[(field, "")]),
        ];
        // Empty first ascending: plain string comparison.
        assert_eq!(f.sorted(&mut data, &format!("{field}+")), names(&["4", "2", "3", "1"]), "{field}");
        assert_eq!(f.sorted(&mut data, &format!("{field}-")), names(&["1", "3", "2", "4"]), "{field}");
    }

    let mut data = vec![task("beta", &[]), task("Zed", &[]), task("alpha", &[]), task("", &[])];
    assert_eq!(f.sorted(&mut data, "description+"), names(&["", "Zed", "alpha", "beta"]));
    assert_eq!(f.sorted(&mut data, "description-"), names(&["beta", "alpha", "Zed", ""]));
}

#[test]
fn mixed_keys_break_ties_in_order() {
    let mut f = Fixture::new(&[], &[]);
    let mut data = vec![
        with_id(task("w2", &[("project", "work")]), 2),
        with_id(task("h1", &[("project", "home")]), 1),
        with_id(task("w5", &[("project", "work")]), 5),
        with_id(task("h4", &[("project", "home")]), 4),
        with_id(task("n3", &[]), 3),
    ];
    assert_eq!(f.sorted(&mut data, "project+,id-"), names(&["n3", "h4", "h1", "w5", "w2"]));
    assert_eq!(f.sorted(&mut data, "project-,id+"), names(&["w2", "w5", "h1", "h4", "n3"]));
}

#[test]
fn all_equal_keys_keep_input_order() {
    let mut f = Fixture::new(&[], &[]);
    let mut data: Vec<Task> = (0..20).map(|i| task(&format!("t{i:02}"), &[("project", "p")])).collect();
    let expected: Vec<String> = (0..20).map(|i| format!("t{i:02}")).collect();
    assert_eq!(f.sorted(&mut data, "project,id"), expected);
    // An unsorted order vector is a permutation that stays as given.
    let mut order: Vec<usize> = (0..20).rev().collect();
    sort_tasks(&mut f.ctx, &mut data, &mut order, "project").unwrap();
    assert_eq!(order, (0..20).rev().collect::<Vec<_>>());
}

#[test]
fn dates_put_set_values_first_and_compare_epoch_strings() {
    let mut f = Fixture::new(&[], &[]);
    for field in ["due", "end", "entry", "start", "until", "wait", "modified", "scheduled", "when"] {
        let mut data = vec![
            task("none", &[]),
            task("200", &[(field, "200")]),
            task("1000", &[(field, "1000")]),
            task("300", &[(field, "300")]),
        ];
        // Lexical epoch comparison ("1000" < "200"), unset last both ways.
        assert_eq!(f.sorted(&mut data, &format!("{field}+")), names(&["1000", "200", "300", "none"]), "{field}");
        assert_eq!(f.sorted(&mut data, &format!("{field}-")), names(&["300", "200", "1000", "none"]), "{field}");
    }
}

#[test]
fn recur_and_duration_udas_compare_parsed_durations() {
    let mut f = Fixture::new(&[], &[]);
    for field in ["recur", "len"] {
        let mut data = vec![
            task("weekly", &[(field, "weekly")]),
            task("2d", &[(field, "2d")]),
            task("none", &[]),
            task("P1D", &[(field, "P1D")]),
        ];
        assert_eq!(f.sorted(&mut data, &format!("{field}+")), names(&["none", "P1D", "2d", "weekly"]), "{field}");
        assert_eq!(f.sorted(&mut data, &format!("{field}-")), names(&["weekly", "2d", "P1D", "none"]), "{field}");

        // Different strings with the same duration are equivalent and stop
        // the key scan (later keys are not consulted); identical strings
        // continue to the next key.
        let mut data = vec![
            with_id(task("daily-3", &[(field, "daily")]), 3),
            with_id(task("1d-1", &[(field, "1d")]), 1),
            with_id(task("daily-2", &[(field, "daily")]), 2),
        ];
        let mut order = vec![0, 1];
        sort_tasks(&mut f.ctx, &mut data, &mut order, &format!("{field},id")).unwrap();
        assert_eq!(order, [0, 1], "{field}");
        let mut order = vec![0, 2];
        sort_tasks(&mut f.ctx, &mut data, &mut order, &format!("{field},id")).unwrap();
        assert_eq!(order, [2, 0], "{field}");
    }
}

#[test]
fn depends_sorts_on_first_dependency_id() {
    const U1: &str = "cccccccc-0000-4000-8000-000000000001";
    const U2: &str = "cccccccc-0000-4000-8000-000000000002";
    const U3: &str = "cccccccc-0000-4000-8000-000000000003";
    let mut f = Fixture::new(
        &[],
        &[
            r#"{"uuid":"cccccccc-0000-4000-8000-000000000001","description":"u1","status":"pending","entry":"20260101T000000Z"}"#,
            r#"{"uuid":"cccccccc-0000-4000-8000-000000000002","description":"u2","status":"pending","entry":"20260101T000000Z"}"#,
            r#"{"uuid":"cccccccc-0000-4000-8000-000000000003","description":"u3","status":"pending","entry":"20260101T000000Z"}"#,
        ],
    );
    let id1 = f.ctx.tdb2.id(U1).unwrap();
    let id3 = f.ctx.tdb2.id(U3).unwrap();
    assert!(id1 > 0 && id3 > 0 && id1 != id3);
    let dep = |d: &str, uuids: &[&str]| {
        let mut t = task(d, &[]);
        for u in uuids {
            t.set(&format!("dep_{u}"), "x");
        }
        t
    };
    let mut data = vec![
        dep("on3", &[U3]),
        dep("on2+1", &[U2, U1]), // first after lexical sort: U1
        dep("none", &[]),
        dep("on1", &[U1]),
    ];
    // on1 and on2+1 tie on the first dependency's ID; description breaks it.
    let (asc, desc) = if id1 < id3 {
        (["none", "on1", "on2+1", "on3"], ["on3", "on2+1", "on1", "none"])
    } else {
        (["none", "on3", "on1", "on2+1"], ["on2+1", "on1", "on3", "none"])
    };
    assert_eq!(f.sorted(&mut data, "depends+,description+"), names(&asc));
    assert_eq!(f.sorted(&mut data, "depends-,description-"), names(&desc));
}

#[test]
fn urgency_compares_computed_values() {
    let mut f = Fixture::new(&[], &[]);
    let mut data = vec![
        task("plain", &[("status", "pending")]),
        task("project", &[("status", "pending"), ("project", "p")]),
        task("next", &[("status", "pending"), ("tags", "next"), ("tag_next", "x")]),
    ];
    assert_eq!(f.sorted(&mut data, "urgency-"), names(&["next", "project", "plain"]));
    assert_eq!(f.sorted(&mut data, "urgency+"), names(&["plain", "project", "next"]));
}

////////////////////////////////////////////////////////////////////////////////
// UDAs.

#[test]
fn numeric_uda_compares_strtof() {
    let mut f = Fixture::new(&[], &[]);
    let mut data = vec![
        task("10", &[("est", "10")]),
        task("9.5", &[("est", "9.5")]),
        task("none", &[]),
        task("-1", &[("est", "-1")]),
        task("2e1", &[("est", "2e1")]),
    ];
    // Unset is strtof ("") == 0.
    assert_eq!(f.sorted(&mut data, "est"), names(&["-1", "none", "9.5", "10", "2e1"]));
    assert_eq!(f.sorted(&mut data, "est-"), names(&["2e1", "10", "9.5", "none", "-1"]));
}

#[test]
fn string_uda_with_values_uses_reversed_value_order() {
    let mut f = Fixture::new(&[], &[]);
    assert_eq!(Task::custom_order().get("size").unwrap(), &names(&["L", "M", "S"]));
    let mut data = vec![
        with_id(task("S", &[("size", "S")]), 1),
        with_id(task("none", &[]), 2),
        with_id(task("L", &[("size", "L")]), 3),
        with_id(task("M", &[("size", "M")]), 4),
    ];
    // Values not in the list (including "") take position end ().
    assert_eq!(f.sorted(&mut data, "size+"), names(&["L", "M", "S", "none"]));
    assert_eq!(f.sorted(&mut data, "size-"), names(&["none", "S", "M", "L"]));

    // Two different values both missing from the list are equivalent, and
    // later keys are not consulted.
    let mut data = vec![
        with_id(task("X", &[("size", "X")]), 2),
        with_id(task("Y", &[("size", "Y")]), 1),
        with_id(task("Y2", &[("size", "Y")]), 0),
    ];
    let mut order = vec![0, 1];
    sort_tasks(&mut f.ctx, &mut data, &mut order, "size,id").unwrap();
    assert_eq!(order, [0, 1]);
    let mut order = vec![1, 2];
    sort_tasks(&mut f.ctx, &mut data, &mut order, "size,id").unwrap();
    assert_eq!(order, [2, 1]);
}

#[test]
fn string_uda_without_values_puts_empty_last() {
    let mut f = Fixture::new(&[], &[]);
    let mut data =
        vec![task("none", &[]), task("b", &[("plain", "b")]), task("a", &[("plain", "a")]), task("none2", &[])];
    assert_eq!(f.sorted(&mut data, "plain+"), names(&["a", "b", "none", "none2"]));
    assert_eq!(f.sorted(&mut data, "plain-"), names(&["b", "a", "none", "none2"]));
}

#[test]
fn other_column_types_fall_through_to_the_next_key() {
    let mut f = Fixture::new(&[], &[]);
    let mut data = vec![
        with_id(task("a", &[("ref", "ffffffff-0000-4000-8000-000000000000")]), 2),
        with_id(task("b", &[("ref", "00000000-0000-4000-8000-000000000000")]), 1),
    ];
    assert_eq!(f.sorted(&mut data, "ref,id"), names(&["b", "a"]));
    assert_eq!(f.sorted(&mut data, "ref"), names(&["a", "b"]));
}

////////////////////////////////////////////////////////////////////////////////
// Errors.

#[test]
fn unknown_field_is_an_error_only_when_reached() {
    let mut f = Fixture::new(&[], &[]);
    let mut data = vec![with_id(task("a", &[]), 1), with_id(task("b", &[]), 2)];
    let mut order = vec![1, 0];
    match sort_tasks(&mut f.ctx, &mut data, &mut order, "bogus") {
        Err(TwError::Message(m)) => assert_eq!(m, "The 'bogus' column is not a valid sort field."),
        other => panic!("{other:?}"),
    }
    // Not reached: the IDs differ.
    let mut order = vec![1, 0];
    sort_tasks(&mut f.ctx, &mut data, &mut order, "id,bogus-").unwrap();
    assert_eq!(order, [0, 1]);
    // Nothing to sort.
    sort_tasks(&mut f.ctx, &mut data, &mut [], "bogus").unwrap();
    // The registry is unchanged (C++ inserts a null entry; see module docs).
    assert!(!f.ctx.columns.contains_key("bogus"));
}

////////////////////////////////////////////////////////////////////////////////
// Random.

#[test]
fn random_uses_configured_seed_and_libstdcxx_hash() {
    set_sort_random_seed(0);
    let mut f = Fixture::new(&["rc.debug.random.seed:42"], &[]);
    let uuids: Vec<String> = (0..12).map(|i| format!("dddddddd-0000-4000-8000-{i:012}")).collect();
    let mut data: Vec<Task> = uuids.iter().map(|u| task(u, &[("uuid", u)])).collect();
    let mut expected = uuids.clone();
    expected.sort_by_key(|u| std_hash(&format!("{u}42")).to_string());
    assert_eq!(f.sorted(&mut data, "random"), expected);
    assert_eq!(sort_random_seed(), 42);
    expected.reverse();
    assert_eq!(f.sorted(&mut data, "random-"), expected);

    // Once chosen, the seed is kept (C++ static).
    f.ctx.config.set("debug.random.seed", "7");
    let mut order: Vec<usize> = vec![0, 1];
    sort_tasks(&mut f.ctx, &mut data, &mut order, "random").unwrap();
    assert_eq!(sort_random_seed(), 42);
    set_sort_random_seed(0);
}

#[test]
fn random_seed_parsing_follows_stoul() {
    set_sort_random_seed(0);
    let mut f = Fixture::new(&[], &[]);
    for (text, seed) in [(" 7x", 7u32), ("+12", 12), ("-1", u32::MAX), ("4294967297", 1)] {
        set_sort_random_seed(0);
        f.ctx.config.set("debug.random.seed", text);
        sort_tasks(&mut f.ctx, &mut [], &mut [], "random").unwrap();
        assert_eq!(sort_random_seed(), seed, "{text}");
    }
    for text in ["abc", "-", "99999999999999999999999"] {
        set_sort_random_seed(0);
        f.ctx.config.set("debug.random.seed", text);
        assert!(matches!(sort_tasks(&mut f.ctx, &mut [], &mut [], "id"), Err(TwError::Runtime(_))), "{text}");
    }
    // Unset: a random seed is drawn.
    set_sort_random_seed(0);
    f.ctx.config.set("debug.random.seed", "");
    sort_tasks(&mut f.ctx, &mut [], &mut [], "id").unwrap();
    set_sort_random_seed(0);
}

/// Inputs for [`HASH_EXPECTED`].
const HASH_INPUTS: [&str; 7] = [
    "",
    "a",
    "abcdefg",
    "abcdefgh",
    "abcdefghi",
    "aaaaaaaa-0000-4000-8000-0000000000011234567",
    "hello world, this is a longer string!",
];

#[test]
fn std_hash_matches_libstdcxx() {
    for (input, expected) in HASH_INPUTS.iter().zip(HASH_EXPECTED) {
        assert_eq!(std_hash(input), expected, "{input:?}");
    }
}

////////////////////////////////////////////////////////////////////////////////
// std::stable_sort port.

fn fnv(h: &mut u64, v: u64) {
    *h ^= v;
    *h = h.wrapping_mul(1_099_511_628_211);
}

/// For n = 0..=60 and two predicates (a valid `key[l] < key[r]` and the
/// non-strict-weak `key[l] + 3 <= key[r]`), the exact sequence of predicate
/// calls and the final order equal those of libstdc++ `std::stable_sort`.
#[test]
fn stable_sort_matches_libstdcxx() {
    for &(n, mode, calls_expected, order_expected) in STABLE_SORT_EXPECTED {
        let key: Vec<i64> = (0..n as i64).map(|i| (i * 7919 + 3) % 13).collect();
        let mut order: Vec<usize> = (0..n).collect();
        let mut calls: u64 = 1_469_598_103_934_665_603;
        stable_sort(&mut order, &mut |l, r| {
            fnv(&mut calls, l as u64);
            fnv(&mut calls, r as u64);
            if mode == 0 {
                key[l] < key[r]
            } else {
                key[l] + 3 <= key[r]
            }
        });
        let mut order_hash: u64 = 1_469_598_103_934_665_603;
        for &o in &order {
            fnv(&mut order_hash, o as u64);
        }
        assert_eq!((calls, order_hash), (calls_expected, order_expected), "n={n} mode={mode}");
    }
}

////////////////////////////////////////////////////////////////////////////////
// sort_projects.

fn pairs(list: &[(&str, i32)]) -> Vec<(String, i32)> {
    list.iter().map(|(n, c)| (n.to_string(), *c)).collect()
}

#[test]
fn sort_projects_inserts_children_after_latest_parent() {
    let all: BTreeMap<String, i32> =
        [("a", 5), ("a.b", 2), ("a.c", 1), ("x.y.z", 3), ("m", 4)].iter().map(|(n, c)| (n.to_string(), *c)).collect();
    let mut sorted = Vec::new();
    sort_projects(&mut sorted, &all);
    // Each child goes directly below its parent, so later siblings come
    // first (C++ list insert); missing parents are appended with count 1.
    assert_eq!(sorted, pairs(&[("a", 5), ("a.c", 1), ("a.b", 2), ("m", 4), ("x", 1), ("x.y", 1), ("x.y.z", 3)]));

    // Existing content is kept and searched for parents.
    let mut sorted = pairs(&[("p", 9), ("q", 8)]);
    let all: BTreeMap<String, i32> = [("p.r".to_string(), 1)].into_iter().collect();
    sort_projects(&mut sorted, &all);
    assert_eq!(sorted, pairs(&[("p", 9), ("p.r", 1), ("q", 8)]));
}

#[test]
fn sort_projects_bool_counts_flags() {
    let all: BTreeMap<String, bool> = [("h.w".to_string(), true), ("k".to_string(), false)].into_iter().collect();
    let mut sorted = Vec::new();
    sort_projects_bool(&mut sorted, &all);
    assert_eq!(sorted, pairs(&[("h", 1), ("h.w", 1), ("k", 0)]));
}

/// `std::hash<std::string>` of [`HASH_INPUTS`] (g++ 11.4, LP64).
const HASH_EXPECTED: [u64; 7] = [
    6142509188972423790,
    4993892634952068459,
    16063891478991045295,
    8664279048047335611,
    13036955925923793583,
    5104058867394235249,
    11449839181402114201,
];

/// `(n, mode, fnv(comparator call log), fnv(final order))` from libstdc++
/// `std::stable_sort` (g++ 11.4); see [`stable_sort_matches_libstdcxx`].
const STABLE_SORT_EXPECTED: &[(usize, u8, u64, u64)] = &[
    (0, 0, 1469598103934665603, 1469598103934665603),
    (0, 1, 1469598103934665603, 1469598103934665603),
    (1, 0, 1469598103934665603, 4953163356653287321),
    (1, 1, 1469598103934665603, 4953163356653287321),
    (2, 0, 11125488673309971922, 11126444148914698056),
    (2, 1, 11125488673309971922, 11126444148914698056),
    (3, 0, 2662050902948333694, 12195995521320448702),
    (3, 1, 2662050902948333694, 12195995521320448702),
    (4, 0, 16022523808319764220, 2932290448510678823),
    (4, 1, 16022523808319764220, 2932290448510678823),
    (5, 0, 14817135424196725387, 16553567279584964729),
    (5, 1, 14817135424196725387, 16553567279584964729),
    (6, 0, 17556679753795151039, 12153032850502717618),
    (6, 1, 17556679753795151039, 12153032850502717618),
    (7, 0, 7030850812984076152, 7377657030907562760),
    (7, 1, 6843423839107852805, 6997644488712162780),
    (8, 0, 286803971161086535, 10680663080423899753),
    (8, 1, 14004374887052083523, 3990519789494981341),
    (9, 0, 17755558463762898205, 246646636193945969),
    (9, 1, 4909195728362548316, 16230490977332682817),
    (10, 0, 6736254809560900353, 9962096341505614922),
    (10, 1, 4131372698538722398, 15395064625082312050),
    (11, 0, 11920248932808551849, 10701611435047273548),
    (11, 1, 7889012362014974636, 4622698544839207656),
    (12, 0, 2334283802720078794, 11252372052926268581),
    (12, 1, 8442176670075618501, 15971447603181772729),
    (13, 0, 4523812322509232931, 10947003745555849661),
    (13, 1, 14142111809554169007, 11458280360401892389),
    (14, 0, 11051969749597368536, 16326215074048980104),
    (14, 1, 12207235523376361541, 16027984762372969972),
    (15, 0, 15258639294463260859, 12039287635224301660),
    (15, 1, 1784335521147795459, 5209993101700276176),
    (16, 0, 2822783209029235315, 14339050315055163641),
    (16, 1, 7914604379824482626, 4158629481157891973),
    (17, 0, 4175673218199493125, 9201869936155389861),
    (17, 1, 262118516576460624, 17534641664300854285),
    (18, 0, 18152610481097177811, 16279507371182932836),
    (18, 1, 14798910946988295541, 16750370979166974868),
    (19, 0, 8034878434461702691, 17165124768091771438),
    (19, 1, 2461822393589740314, 3221151516578597174),
    (20, 0, 9028865759422389851, 8464000763773599233),
    (20, 1, 10195024463053344569, 3440400255936412111),
    (21, 0, 16891983051424501732, 6065680895914679687),
    (21, 1, 17354719023008314649, 11050459860191438639),
    (22, 0, 3179610892279016781, 4666179166016373650),
    (22, 1, 453224195243779765, 1885276094411046664),
    (23, 0, 15342812408355203439, 9516273009731050662),
    (23, 1, 13723162117405811579, 3564904078400616582),
    (24, 0, 3096862688934778698, 13478436449486298273),
    (24, 1, 5414286191666541594, 8770726916020657763),
    (25, 0, 4551635963629824562, 2737860326388207195),
    (25, 1, 9556675028589934416, 3590192938415196929),
    (26, 0, 8642279581912075760, 3993261195314469420),
    (26, 1, 3154854129312556398, 5616212227312572970),
    (27, 0, 780160035497336502, 12726730914026634860),
    (27, 1, 16831714313617486614, 687644852223249306),
    (28, 0, 14081030258070681508, 1209580436660174227),
    (28, 1, 11897253831425379048, 2662594041300849701),
    (29, 0, 10545866992725627621, 15539132496746591471),
    (29, 1, 2358964191044759806, 9250222583359123657),
    (30, 0, 9499825641255830894, 16767340650243932964),
    (30, 1, 1782819227110212725, 17448528008289465410),
    (31, 0, 9223030516462877808, 9429333251744214728),
    (31, 1, 7111740700661793024, 8111953145283908080),
    (32, 0, 2259851049486453909, 13277709091910101731),
    (32, 1, 7111008953205797472, 13461829944159350061),
    (33, 0, 18006215734005522142, 4382672417711529585),
    (33, 1, 7889207049637160784, 4089984357774560061),
    (34, 0, 15335726839318516561, 16582276143743441044),
    (34, 1, 9839318882609027325, 5274008606202644374),
    (35, 0, 4625544836341271939, 11034397334795854788),
    (35, 1, 5963913971067774127, 1459506172400261552),
    (36, 0, 15025862588351484423, 7582000474743141535),
    (36, 1, 15667704030749535256, 15058296100285309637),
    (37, 0, 17696220674603952103, 2695130133600754633),
    (37, 1, 558054434488743191, 14431704699382492383),
    (38, 0, 4994103116321436762, 5146516003558470148),
    (38, 1, 5722638943813247452, 3025336355410613378),
    (39, 0, 3570425973388287430, 12388409998924370794),
    (39, 1, 2464435060196628083, 13951737457200709144),
    (40, 0, 13071778363120789534, 12683717466102877785),
    (40, 1, 11436310416605137136, 5219656015385377867),
    (41, 0, 14132730596630990503, 11542764728017901427),
    (41, 1, 909757348891106547, 13985939483833619521),
    (42, 0, 14460026894338579022, 220514701956016868),
    (42, 1, 16134264569512269606, 1645231400643216032),
    (43, 0, 8123168504978687265, 2619155173173589790),
    (43, 1, 14699032364398848002, 1980293990285851902),
    (44, 0, 4778066662736469349, 16416110638318629057),
    (44, 1, 15138825980959479324, 14135350463440169433),
    (45, 0, 12233165715928451602, 7864588566074962739),
    (45, 1, 18048140211601184886, 17282792541561266623),
    (46, 0, 14166731676877443918, 2881129643988000784),
    (46, 1, 4156099678704206127, 4610262451852692812),
    (47, 0, 13365531633364865708, 11224921290989528514),
    (47, 1, 11019019303954192442, 4485837201671627014),
    (48, 0, 14275174005643576931, 14248364225412590587),
    (48, 1, 1216413868194515272, 15261325865789084673),
    (49, 0, 198089955364520235, 15323222006785258303),
    (49, 1, 6261557399237625394, 1719334495718844831),
    (50, 0, 12572364079533436694, 10074712664554067332),
    (50, 1, 11369678304936396766, 12619491794448549258),
    (51, 0, 13604001903942675231, 8303917289759581250),
    (51, 1, 1020747566971145688, 939293939196323512),
    (52, 0, 4800990905238987586, 2809211256135515503),
    (52, 1, 9484547555087121491, 9434031039617617603),
    (53, 0, 6867012068209056260, 11841457730180104537),
    (53, 1, 10629490975396579370, 10659618031289460775),
    (54, 0, 17249612052415450646, 2758262307550793920),
    (54, 1, 2521675501125402739, 18161708979504573808),
    (55, 0, 2920679131886074800, 474456137456569042),
    (55, 1, 7399050597355371093, 12345470996728976252),
    (56, 0, 6145053357811127330, 7039629386788954155),
    (56, 1, 17448909871514574829, 7561178730850867431),
    (57, 0, 4078052879297588495, 2751488049805887089),
    (57, 1, 6581850098439088109, 7970908457370640559),
    (58, 0, 9455651376236389608, 9354192087257490222),
    (58, 1, 9156165702153041465, 6898711163084886888),
    (59, 0, 3686097928019886400, 13278515927972151688),
    (59, 1, 1176220976034064001, 10427030652411605794),
    (60, 0, 16224708804284366367, 8251762856232969077),
    (60, 1, 11687667529209890382, 17256231576332390331),
];
