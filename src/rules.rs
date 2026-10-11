//! Rust translation of `src/rules.cpp`.
//!
//! Everything in the C++ file is ported:
//!
//! - the file statics `gsColor` / `gsPrecedence` (read through [`gs_color`]
//!   and [`gs_precedence`]) and `Datetime now` (see [`rules_now`]);
//! - [`initialize_color_rules`] (called by `Context::initialize`);
//! - the per-task rule engine [`auto_colorize`] with every static helper
//!   (`applyColor`, `colorizeBlocked`, `colorizeBlocking`, `colorizeTagged`,
//!   `colorizeActive`, `colorizeScheduled`, `colorizeUntil`, `colorizeTag`,
//!   `colorizeProject`, `colorizeProjectNone`, `colorizeTagNone`,
//!   `colorizeKeyword`, `colorizeUDA`, `colorizeDue`, `colorizeDueToday`,
//!   `colorizeOverdue`, `colorizeRecurring`, `colorizeCompleted`,
//!   `colorizeDeleted`) as private functions of the same snake-case names;
//! - the message colourers [`colorize_header`], [`colorize_footnote`],
//!   [`colorize_error`] and [`colorize_debug`] used by `Context` when it dumps
//!   headers, footnotes, errors and debug messages.
//!
//! The C++ file statics become thread-local statics (the binary is
//! single-threaded; unit tests each run on their own thread).
//!
//! Design decision DD3 (milestone 6): the rule data is built once per
//! invocation, not once per task. `initialize_color_rules` (run once by
//! `Context::initialize`) parses every `color.*` setting into `gsColor` and
//! expands `rule.precedence.color` into `gsPrecedence`; [`auto_colorize`]
//! only reads those thread-locals (it borrows them, it does not copy them),
//! exactly as the C++ reads its file statics. Nothing is re-parsed per task.
//!
//! Context access: [`auto_colorize`] takes `&mut Context` because it needs
//! `Context::color` (which caches its decision and may rewrite `rc.color`),
//! `rc.rule.color.merge`, `rc.search.case.sensitive`, and the [`TaskEnv`]
//! view of the context for `hasTag` virtual tags and the `is_due*`
//! predicates (`rc.due`, `latest_id`). `is_blocked` / `is_blocking` are the
//! flags TDB2 already computed from the dependency graph when it loaded the
//! task, as in C++; the rules do not query TDB2 themselves. C++ throws from
//! `hasTag` / `getStatus` (`The status '...' is not valid.`, replica errors
//! from `+LATEST`) propagate as `Err`.
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::color::Color;
use crate::shared::datetime::Datetime;
use crate::shared::shared::{auto_complete, find, split};
use crate::task::{Status, Task, TaskEnv};

thread_local! {
    /// C++ `static std::map<std::string, Color> gsColor`.
    static GS_COLOR: RefCell<BTreeMap<String, Color>> = const { RefCell::new(BTreeMap::new()) };
    /// C++ `static std::vector<std::string> gsPrecedence`.
    static GS_PRECEDENCE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// C++ `static Datetime now`, as an epoch; `None` until first captured.
    static NOW: Cell<Option<i64>> = const { Cell::new(None) };
}

/// C++ `static Datetime now`: the time `colorizeScheduled` compares against.
///
/// Deviation: the C++ static is constructed during static initialisation,
/// before `main`. Rust has no life-before-main, so the value is captured on
/// first use; [`initialize_color_rules`] reads it first thing, so in the
/// binary it is fixed during `Context::initialize`, before any task is
/// coloured, and stays fixed for the rest of the run like the C++ static.
pub fn rules_now() -> Datetime {
    let epoch = NOW.with(|n| match n.get() {
        Some(t) => t,
        None => {
            let t = Datetime::new().to_epoch();
            n.set(Some(t));
            t
        }
    });
    Datetime::from_epoch(epoch)
}

/// Test hook: pins (or with `None`, resets) this thread's `now`.
#[cfg(test)]
fn set_rules_now(epoch: Option<i64>) {
    NOW.with(|n| n.set(epoch));
}

/// The loaded `gsColor` rule for `name`, or the default (trivial) colour:
/// C++ `gsColor[name]` (whose `operator[]` default-constructs missing keys).
pub fn gs_color(name: &str) -> Color {
    GS_COLOR.with(|c| c.borrow().get(name).cloned().unwrap_or_default())
}

/// A copy of `gsPrecedence`: the `color.*` rule names in
/// `rule.precedence.color` order.
pub fn gs_precedence() -> Vec<String> {
    GS_PRECEDENCE.with(|p| p.borrow().clone())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `initializeColorRules ()`.
///
/// Calls `Context::color ()` first, which decides colour use once and, when
/// stdout is not a TTY, turns `detection` and `color` off in the
/// configuration (so `getWidth` falls back to `defaultwidth`); without colour
/// it stops there. Otherwise every `color.*` setting is parsed into
/// `gsColor`, and `rule.precedence.color` is expanded (each entry prefixed
/// with `color.` and autocompleted, minimum 3, against the loaded rule names)
/// into `gsPrecedence`.
///
/// An unrecognised colour (`The color '<word>' is not recognized.`, a C++
/// `throw std::string`) is queued with `Context::error` and stops loading,
/// leaving the rules loaded so far, as the C++ `try` / `catch` does; any other
/// error propagates.
pub fn initialize_color_rules(context: &mut Context) -> Result<()> {
    // Fix the file-static `now` at startup (see `rules_now`).
    let _ = rules_now();

    // If color is not enable/supported, short circuit.
    if !context.color() {
        return Ok(());
    }

    match load_color_rules(context) {
        Ok(()) => Ok(()),
        Err(TwError::Message(e)) => {
            context.error(&e);
            Ok(())
        }
        Err(other) => Err(other),
    }
}

/// The body of the C++ `try` block in `initializeColorRules`.
fn load_color_rules(context: &mut Context) -> Result<()> {
    GS_COLOR.with(|c| c.borrow_mut().clear());
    GS_PRECEDENCE.with(|p| p.borrow_mut().clear());

    // Load all the configuration values, filter to only the ones that begin with
    // "color.", then store name/value in gsColor, and name in rules.
    let mut rules: Vec<String> = Vec::new();
    for (key, value) in &context.config {
        if key.starts_with("color.") {
            let c = Color::parse(value)?;
            GS_COLOR.with(|g| g.borrow_mut().insert(key.clone(), c));

            rules.push(key.clone());
        }
    }

    // Load the rule.precedence.color list, split it, then autocomplete against
    // the 'rules' vector loaded above.
    let precedence = split(&context.config.get("rule.precedence.color"), ',');

    for p in &precedence {
        // Add the leading "color." string.
        let rule = format!("color.{p}");
        let results = auto_complete(&rule, &rules, 3); // Hard-coded 3.

        GS_PRECEDENCE.with(|g| g.borrow_mut().extend(results));
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `static void applyColor (const Color& base, Color& c, bool merge)`:
/// blend `base` into `c`, or replace `c` with it.
fn apply_color(base: &Color, c: &mut Color, merge: bool) {
    if merge {
        c.blend(base);
    } else {
        *c = *base;
    }
}

/// C++ `colorizeBlocked`.
fn colorize_blocked(task: &Task, base: &Color, c: &mut Color, merge: bool) {
    if task.is_blocked {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeBlocking`.
fn colorize_blocking(task: &Task, base: &Color, c: &mut Color, merge: bool) {
    if task.is_blocking {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeTagged`.
fn colorize_tagged(task: &Task, base: &Color, c: &mut Color, merge: bool) {
    if task.get_tag_count() != 0 {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeActive`: `start` set and no `end` (not the `+ACTIVE`
/// virtual tag, which ignores `end`; the C++ TODO notes the inconsistency).
fn colorize_active(task: &Task, base: &Color, c: &mut Color, merge: bool) {
    // TODO: Not consistent with the implementation of the +ACTIVE virtual tag
    if task.has("start") && !task.has("end") {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeScheduled`: `scheduled` at or before the file-static `now`
/// (not the `+SCHEDULED` virtual tag, which only checks presence).
fn colorize_scheduled(task: &Task, base: &Color, c: &mut Color, merge: bool) {
    // TODO: Not consistent with the implementation of the +SCHEDULED virtual tag
    if task.has("scheduled") && Datetime::from_epoch(task.get_date("scheduled")) <= rules_now() {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeUntil`.
fn colorize_until(task: &Task, base: &Color, c: &mut Color, merge: bool) {
    if task.has("until") {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeTag`: `color.tag.<tag>`, where `<tag>` may be a virtual tag.
fn colorize_tag(
    task: &Task,
    env: &mut dyn TaskEnv,
    rule: &str,
    base: &Color,
    c: &mut Color,
    merge: bool,
) -> Result<()> {
    if task.has_tag(&rule[10..], env)? {
        apply_color(base, c, merge);
    }
    Ok(())
}

/// C++ `compare (a, b, sensitive)` on byte strings: the C++ compares
/// `project.substr (0, n)`, a byte cut that need not fall on a UTF-8
/// boundary. Case-insensitive is `strcasecmp` (ASCII folding, stops at NUL).
fn compare_bytes(left: &[u8], right: &[u8], sensitive: bool) -> bool {
    if !sensitive {
        let cut = |b: &[u8]| -> usize { b.iter().position(|&ch| ch == 0).unwrap_or(b.len()) };
        return left[..cut(left)].eq_ignore_ascii_case(&right[..cut(right)]);
    }
    left == right
}

/// C++ `colorizeProject`: `color.project.<name>` matches projects that start
/// with `<name>` (so `color.project.home` also colours `home.garden`),
/// observing `rc.search.case.sensitive`.
fn colorize_project(
    sensitive: bool,
    task: &Task,
    rule: &str,
    base: &Color,
    c: &mut Color,
    merge: bool,
) {
    let project = task.get_ref("project").as_bytes();
    let rule_trunc = &rule.as_bytes()[14..];

    // Match project names leftmost.
    if rule_trunc.len() <= project.len()
        && compare_bytes(rule_trunc, &project[..rule_trunc.len()], sensitive)
    {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeProjectNone`.
fn colorize_project_none(task: &Task, base: &Color, c: &mut Color, merge: bool) {
    if !task.has("project") {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeTagNone`.
fn colorize_tag_none(task: &Task, base: &Color, c: &mut Color, merge: bool) {
    if task.get_tag_count() == 0 {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeKeyword`: `color.keyword.<word>` matches a substring of the
/// description, or failing that of any annotation (applied at most once),
/// observing `rc.search.case.sensitive`.
fn colorize_keyword(
    sensitive: bool,
    task: &Task,
    rule: &str,
    base: &Color,
    c: &mut Color,
    merge: bool,
) {
    let keyword = &rule[14..];

    // The easiest thing to check is the description, because it is just one
    // attribute.
    if find(task.get_ref("description"), keyword, sensitive).is_some() {
        apply_color(base, c, merge);
    }
    // Failing the description check, look at all annotations, returning on the
    // first match.
    else {
        for text in task.get_annotations().values() {
            if find(text, keyword, sensitive).is_some() {
                apply_color(base, c, merge);
                return;
            }
        }
    }
}

/// C++ `colorizeUDA`: `color.uda.<name>` matches a task that has the UDA;
/// `color.uda.<name>.<value>` matches that value, and the value `none`
/// also matches a task without the UDA.
fn colorize_uda(task: &Task, rule: &str, base: &Color, c: &mut Color, merge: bool) {
    // Is the rule color.uda.name.value or color.uda.name?
    match rule[10..].find('.') {
        None => {
            if task.has(&rule[10..]) {
                apply_color(base, c, merge);
            }
        }
        Some(off) => {
            let pos = 10 + off;
            let uda = &rule[10..pos];
            let val = &rule[pos + 1..];
            if (val == "none" && !task.has(uda)) || task.get_ref(uda) == val {
                apply_color(base, c, merge);
            }
        }
    }
}

/// C++ `colorizeDue`.
fn colorize_due(
    task: &Task,
    env: &mut dyn TaskEnv,
    base: &Color,
    c: &mut Color,
    merge: bool,
) -> Result<()> {
    if task.is_due(env)? {
        apply_color(base, c, merge);
    }
    Ok(())
}

/// C++ `colorizeDueToday`.
fn colorize_due_today(
    task: &Task,
    env: &mut dyn TaskEnv,
    base: &Color,
    c: &mut Color,
    merge: bool,
) -> Result<()> {
    if task.is_duetoday(env)? {
        apply_color(base, c, merge);
    }
    Ok(())
}

/// C++ `colorizeOverdue`.
fn colorize_overdue(
    task: &Task,
    env: &mut dyn TaskEnv,
    base: &Color,
    c: &mut Color,
    merge: bool,
) -> Result<()> {
    if task.is_overdue(env)? {
        apply_color(base, c, merge);
    }
    Ok(())
}

/// C++ `colorizeRecurring`.
fn colorize_recurring(task: &Task, base: &Color, c: &mut Color, merge: bool) {
    if task.has("recur") {
        apply_color(base, c, merge);
    }
}

/// C++ `colorizeCompleted`.
fn colorize_completed(task: &Task, base: &Color, c: &mut Color, merge: bool) -> Result<()> {
    if task.get_status()? == Status::Completed {
        apply_color(base, c, merge);
    }
    Ok(())
}

/// C++ `colorizeDeleted`.
fn colorize_deleted(task: &Task, base: &Color, c: &mut Color, merge: bool) -> Result<()> {
    if task.get_status()? == Status::Deleted {
        apply_color(base, c, merge);
    }
    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `void autoColorize (Task& task, Color& c)`.
///
/// `c` arrives holding any colour the caller assigned specifically (e.g. a
/// report's alternating-row colour). Without colour (`Context::color` false,
/// i.e. `rc.color` off or no TTY and no `_forcecolor`) or for a task tagged
/// `nocolor`, `c` is reset to no colour and nothing else happens; `color()`
/// is evaluated first, so `hasTag` is not reached when colour is off.
///
/// Otherwise `gsPrecedence` is walked in reverse (the first entry of
/// `rule.precedence.color` is applied last, so it is "King"); each
/// nontrivial rule whose condition holds is blended into `c` when
/// `rc.rule.color.merge` is on, or replaces `c` when it is off. Exact rule
/// names are tested before the `color.tag.` / `color.project.` /
/// `color.keyword.` / `color.uda.` wildcards, in the C++ order, so
/// `color.tag.none` and `color.project.none` are never treated as a tag or
/// project named `none`. Rule names that match nothing (e.g. `color.header`)
/// are ignored.
///
/// # Errors
/// What `Task::has_tag` / `get_status` / `is_due*` return where the C++
/// throws: an invalid stored status, or a `+LATEST` replica error from a
/// `color.tag.LATEST` rule.
pub fn auto_colorize(ctx: &mut Context, task: &Task, c: &mut Color) -> Result<()> {
    // The special tag 'nocolor' overrides all auto and specific colorization.
    if !ctx.color() || task.has_tag("nocolor", ctx)? {
        *c = Color::new();
        return Ok(());
    }

    let merge = ctx.config.get_boolean("rule.color.merge");
    // Observe the case sensitivity setting (read per rule in C++; the
    // configuration cannot change while one task is coloured).
    let sensitive = ctx.config.get_boolean("search.case.sensitive");

    // Note: c already contains colors specifically assigned via command.
    // Note: These rules form a hierarchy - the last rule is King, hence the
    //       reverse iterator.
    GS_PRECEDENCE.with(|precedence| {
        GS_COLOR.with(|colors| {
            let precedence = precedence.borrow();
            let colors = colors.borrow();
            for r in precedence.iter().rev() {
                let base = colors.get(r).copied().unwrap_or_default();
                if !base.nontrivial() {
                    continue;
                }
                let r = r.as_str();
                match r {
                    "color.blocked" => colorize_blocked(task, &base, c, merge),
                    "color.blocking" => colorize_blocking(task, &base, c, merge),
                    "color.tagged" => colorize_tagged(task, &base, c, merge),
                    "color.active" => colorize_active(task, &base, c, merge),
                    "color.scheduled" => colorize_scheduled(task, &base, c, merge),
                    "color.until" => colorize_until(task, &base, c, merge),
                    "color.project.none" => colorize_project_none(task, &base, c, merge),
                    "color.tag.none" => colorize_tag_none(task, &base, c, merge),
                    "color.due" => colorize_due(task, ctx, &base, c, merge)?,
                    "color.due.today" => colorize_due_today(task, ctx, &base, c, merge)?,
                    "color.overdue" => colorize_overdue(task, ctx, &base, c, merge)?,
                    "color.recurring" => colorize_recurring(task, &base, c, merge),
                    "color.completed" => colorize_completed(task, &base, c, merge)?,
                    "color.deleted" => colorize_deleted(task, &base, c, merge)?,

                    // Wildcards
                    _ if r.starts_with("color.tag.") => {
                        colorize_tag(task, ctx, r, &base, c, merge)?
                    }
                    _ if r.starts_with("color.project.") => {
                        colorize_project(sensitive, task, r, &base, c, merge)
                    }
                    _ if r.starts_with("color.keyword.") => {
                        colorize_keyword(sensitive, task, r, &base, c, merge)
                    }
                    _ if r.starts_with("color.uda.") => colorize_uda(task, r, &base, c, merge),
                    _ => {}
                }
            }
            Ok(())
        })
    })
}

/// Colours `input` with the `gsColor` rule `name` if it is nontrivial.
fn colorize_with_rule(name: &str, input: &str) -> String {
    let color = gs_color(name);
    if color.nontrivial() {
        return color.colorize(input);
    }

    input.to_string()
}

////////////////////////////////////////////////////////////////////////////////
/// C++ `colorizeHeader`: `input` in `color.header`, if set.
pub fn colorize_header(input: &str) -> String {
    colorize_with_rule("color.header", input)
}

/// C++ `colorizeFootnote`: `input` in `color.footnote`, if set.
pub fn colorize_footnote(input: &str) -> String {
    colorize_with_rule("color.footnote", input)
}

/// C++ `colorizeError`: `input` in `color.error`, if set.
pub fn colorize_error(input: &str) -> String {
    colorize_with_rule("color.error", input)
}

/// C++ `colorizeDebug`: `input` in `color.debug`, if set.
pub fn colorize_debug(input: &str) -> String {
    colorize_with_rule("color.debug", input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::set_stdout_tty_override;

    /// Restores the thread-local TTY override and clears the rules.
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            set_stdout_tty_override(None);
            set_rules_now(None);
            GS_COLOR.with(|c| c.borrow_mut().clear());
            GS_PRECEDENCE.with(|p| p.borrow_mut().clear());
        }
    }

    fn context(settings: &[(&str, &str)]) -> Context {
        let mut context = Context::new();
        for (k, v) in settings {
            context.config.set(k, *v);
        }
        context
    }

    #[test]
    fn no_color_short_circuits_and_turns_detection_off() {
        let _reset = Reset;
        set_stdout_tty_override(Some(false));
        let mut ctx = context(&[
            ("color", "on"),
            ("detection", "on"),
            ("color.header", "red"),
        ]);
        initialize_color_rules(&mut ctx).unwrap();
        assert_eq!(ctx.config.get("color"), "off");
        assert_eq!(ctx.config.get("detection"), "off");
        assert!(!gs_color("color.header").nontrivial());
        assert_eq!(colorize_header("x"), "x");
    }

    #[test]
    fn loads_rules_and_precedence() {
        let _reset = Reset;
        set_stdout_tty_override(Some(true));
        let mut ctx = context(&[
            ("color", "on"),
            ("color.header", "red"),
            ("color.footnote", "green"),
            ("color.error", "white on red"),
            ("color.debug", "blue"),
            ("color.tag.next", "bold"),
            ("color.tagged", "yellow"),
            ("color.due", "red"),
            ("color.due.today", "red"),
            ("rule.precedence.color", "tag.,due,nosuch,"),
        ]);
        initialize_color_rules(&mut ctx).unwrap();
        assert!(ctx.errors.is_empty());

        assert_eq!(
            colorize_header("h"),
            Color::parse("red").unwrap().colorize("h")
        );
        assert_eq!(
            colorize_footnote("f"),
            Color::parse("green").unwrap().colorize("f")
        );
        assert_eq!(
            colorize_error("e"),
            Color::parse("white on red").unwrap().colorize("e")
        );
        assert_eq!(
            colorize_debug("d"),
            Color::parse("blue").unwrap().colorize("d")
        );
        assert_ne!(colorize_header("h"), "h");

        // "color.tag." matches "color.tag.next" only (partial); "color.due"
        // is an exact match and wins alone; "color.nosuch" adds nothing; the
        // trailing empty entry becomes "color.", a partial match of every
        // rule, so it adds them all, as in C++.
        let precedence = gs_precedence();
        assert_eq!(precedence[0], "color.tag.next");
        assert_eq!(precedence[1], "color.due");
        let mut rest: Vec<String> = precedence[2..].to_vec();
        rest.sort();
        let mut all: Vec<String> = ctx
            .config
            .keys()
            .filter(|k| k.starts_with("color."))
            .cloned()
            .collect();
        all.sort();
        assert_eq!(rest, all);
    }

    #[test]
    fn empty_rule_is_trivial() {
        let _reset = Reset;
        set_stdout_tty_override(Some(false));
        let mut ctx = context(&[("_forcecolor", "on"), ("color.header", "")]);
        initialize_color_rules(&mut ctx).unwrap();
        assert!(ctx.color());
        assert_eq!(colorize_header("x"), "x");
    }

    #[test]
    fn bad_color_is_queued_as_error_and_stops_loading() {
        let _reset = Reset;
        set_stdout_tty_override(Some(true));
        let mut ctx = context(&[
            ("color", "on"),
            ("color.alpha", "red"),
            ("color.beta", "chartreuse"),
            ("color.gamma", "blue"),
            ("rule.precedence.color", "alpha"),
        ]);
        initialize_color_rules(&mut ctx).unwrap();
        assert_eq!(ctx.errors, ["The color 'chartreuse' is not recognized."]);
        assert!(gs_color("color.alpha").nontrivial());
        assert!(!gs_color("color.gamma").nontrivial());
        assert!(gs_precedence().is_empty());
    }

    ////////////////////////////////////////////////////////////////////////
    // autoColorize.

    use crate::shared::localtime;

    fn col(spec: &str) -> Color {
        Color::parse(spec).unwrap()
    }

    /// A colour-enabled context with `settings` (plus `due=7`, merge off and
    /// case-sensitive search unless overridden), rules loaded.
    fn rules_context(settings: &[(&str, &str)]) -> Context {
        set_stdout_tty_override(Some(true));
        let mut ctx = context(&[
            ("color", "on"),
            ("due", "7"),
            ("rule.color.merge", "off"),
            ("search.case.sensitive", "yes"),
        ]);
        for (k, v) in settings {
            ctx.config.set(k, *v);
        }
        initialize_color_rules(&mut ctx).unwrap();
        assert!(ctx.errors.is_empty(), "{:?}", ctx.errors);
        ctx
    }

    /// The colour `auto_colorize` gives `task` starting from no colour.
    fn colorize(ctx: &mut Context, task: &Task) -> Color {
        let mut c = Color::new();
        auto_colorize(ctx, task, &mut c).unwrap();
        c
    }

    /// Asserts that the single rule `rule` (colour red) colours `yes` red
    /// and leaves `no` uncoloured.
    fn check_rule(rule: &str, extra: &[(&str, &str)], yes: &Task, no: &Task) {
        let precedence = rule.strip_prefix("color.").unwrap();
        let mut settings = vec![(rule, "red"), ("rule.precedence.color", precedence)];
        settings.extend_from_slice(extra);
        let mut ctx = rules_context(&settings);
        assert_eq!(gs_precedence(), [rule.to_string()], "{rule}");
        assert_eq!(colorize(&mut ctx, yes), col("red"), "{rule} should match");
        assert_eq!(
            colorize(&mut ctx, no),
            Color::new(),
            "{rule} should not match"
        );
    }

    fn task(attrs: &[(&str, &str)]) -> Task {
        let mut t = Task::new();
        for (k, v) in attrs {
            t.set(k, v);
        }
        t
    }

    fn epoch(offset: i64) -> String {
        (localtime::time_now() + offset).to_string()
    }

    #[test]
    fn color_off_resets_and_short_circuits() {
        let _reset = Reset;
        set_stdout_tty_override(Some(true));
        let mut ctx = context(&[
            ("color", "off"),
            ("color.tagged", "red"),
            ("rule.precedence.color", "tagged"),
        ]);
        initialize_color_rules(&mut ctx).unwrap();
        assert!(gs_precedence().is_empty());
        let mut t = task(&[]);
        t.add_tag("x");
        let mut c = col("blue");
        auto_colorize(&mut ctx, &t, &mut c).unwrap();
        assert_eq!(c, Color::new());

        // Colour off is decided before hasTag runs: a task with an invalid
        // status does not error.
        let bad = task(&[("status", "bogus"), ("tag_LATEST", "x")]);
        let mut c = col("blue");
        auto_colorize(&mut ctx, &bad, &mut c).unwrap();
        assert_eq!(c, Color::new());
    }

    #[test]
    fn nocolor_tag_overrides_everything() {
        let _reset = Reset;
        let mut ctx =
            rules_context(&[("color.tagged", "red"), ("rule.precedence.color", "tagged")]);
        let mut t = task(&[]);
        t.add_tag("nocolor");
        let mut c = col("blue");
        auto_colorize(&mut ctx, &t, &mut c).unwrap();
        assert_eq!(c, Color::new());

        t.remove_tag("nocolor");
        t.add_tag("other");
        assert_eq!(colorize(&mut ctx, &t), col("red"));
    }

    #[test]
    fn blocked_and_blocking() {
        let _reset = Reset;
        let mut yes = task(&[]);
        yes.is_blocked = true;
        check_rule("color.blocked", &[], &yes, &task(&[]));
        let mut yes = task(&[]);
        yes.is_blocking = true;
        check_rule("color.blocking", &[], &yes, &task(&[]));
    }

    #[test]
    fn tagged_and_tag_none() {
        let _reset = Reset;
        let mut tagged = task(&[]);
        tagged.add_tag("home");
        check_rule("color.tagged", &[], &tagged, &task(&[]));
        check_rule("color.tag.none", &[], &task(&[]), &tagged);
    }

    #[test]
    fn active_requires_start_without_end() {
        let _reset = Reset;
        let started = task(&[("start", &epoch(-60))]);
        let ended = task(&[("start", &epoch(-60)), ("end", &epoch(-30))]);
        check_rule("color.active", &[], &started, &ended);
    }

    #[test]
    fn scheduled_compares_against_file_static_now() {
        let _reset = Reset;
        let now = localtime::time_now();
        set_rules_now(Some(now));
        let at = task(&[("scheduled", &now.to_string())]);
        let past = task(&[("scheduled", &(now - 1).to_string())]);
        let future = task(&[("scheduled", &(now + 1).to_string())]);
        check_rule("color.scheduled", &[], &at, &future);
        check_rule("color.scheduled", &[], &past, &task(&[]));
        // `now` stays fixed once captured.
        assert_eq!(rules_now().to_epoch(), now);
    }

    #[test]
    fn until_recurring() {
        let _reset = Reset;
        check_rule(
            "color.until",
            &[],
            &task(&[("until", &epoch(3600))]),
            &task(&[]),
        );
        check_rule(
            "color.recurring",
            &[],
            &task(&[("recur", "weekly")]),
            &task(&[]),
        );
    }

    #[test]
    fn completed_and_deleted() {
        let _reset = Reset;
        let completed = task(&[("status", "completed")]);
        let deleted = task(&[("status", "deleted")]);
        check_rule("color.completed", &[], &completed, &deleted);
        check_rule("color.deleted", &[], &deleted, &completed);
    }

    #[test]
    fn due_due_today_and_overdue() {
        let _reset = Reset;
        let today = task(&[("due", &epoch(0))]);
        let soon = task(&[("due", &epoch(3 * 86400))]);
        let far = task(&[("due", &epoch(30 * 86400))]);
        let past = task(&[("due", &epoch(-3 * 86400))]);
        let done_past = task(&[("due", &epoch(-3 * 86400)), ("status", "completed")]);
        check_rule("color.due", &[], &soon, &far);
        check_rule("color.due", &[], &today, &past);
        check_rule("color.due.today", &[], &today, &soon);
        check_rule("color.overdue", &[], &past, &done_past);
    }

    #[test]
    fn project_none_and_project_prefix() {
        let _reset = Reset;
        let home = task(&[("project", "Home.Garden")]);
        let work = task(&[("project", "Work")]);
        check_rule("color.project.none", &[], &task(&[]), &home);
        check_rule("color.project.Home", &[], &home, &work);
        // Leftmost only.
        check_rule(
            "color.project.Garden",
            &[],
            &task(&[("project", "Garden")]),
            &home,
        );
        // Case sensitivity.
        check_rule(
            "color.project.home",
            &[],
            &task(&[("project", "home")]),
            &home,
        );
        check_rule(
            "color.project.home",
            &[("search.case.sensitive", "no")],
            &home,
            &work,
        );
        // Longer than the project never matches.
        check_rule(
            "color.project.Home.Garden.Shed",
            &[],
            &task(&[("project", "Home.Garden.Shed")]),
            &home,
        );
    }

    #[test]
    fn project_prefix_cuts_bytes() {
        let _reset = Reset;
        // A two-byte character cut in half by the rule length does not
        // panic and does not match.
        let t = task(&[("project", "a\u{e9}b")]);
        check_rule(
            "color.project.a\u{e9}",
            &[],
            &t,
            &task(&[("project", "ab")]),
        );
        let mut ctx = rules_context(&[
            ("color.project.ab", "red"),
            ("rule.precedence.color", "project.ab"),
        ]);
        assert_eq!(colorize(&mut ctx, &t), Color::new());
    }

    #[test]
    fn tag_rule_concrete_and_virtual() {
        let _reset = Reset;
        let mut home = task(&[]);
        home.add_tag("home");
        let mut other = task(&[]);
        other.add_tag("work");
        check_rule("color.tag.home", &[], &home, &other);
        // Virtual tags go through hasTag.
        check_rule(
            "color.tag.PROJECT",
            &[],
            &task(&[("project", "x")]),
            &task(&[]),
        );
    }

    #[test]
    fn keyword_description_annotations_and_case() {
        let _reset = Reset;
        let desc = task(&[("description", "buy Milk now")]);
        let mut ann = task(&[("description", "groceries")]);
        ann.add_annotation("also Milk");
        let none = task(&[("description", "nothing here")]);
        check_rule("color.keyword.Milk", &[], &desc, &none);
        check_rule("color.keyword.Milk", &[], &ann, &none);
        check_rule(
            "color.keyword.milk",
            &[],
            &task(&[("description", "milk")]),
            &desc,
        );
        check_rule(
            "color.keyword.milk",
            &[("search.case.sensitive", "no")],
            &desc,
            &none,
        );
        check_rule(
            "color.keyword.milk",
            &[("search.case.sensitive", "no")],
            &ann,
            &none,
        );

        // Applied once even when both the description and annotations match
        // (visible with merge on and a pre-set colour: blending twice is
        // idempotent, but replacing would not be).
        let mut both = task(&[("description", "Milk")]);
        both.add_annotation("Milk");
        let mut ctx = rules_context(&[
            ("color.keyword.Milk", "red"),
            ("rule.precedence.color", "keyword.Milk"),
            ("rule.color.merge", "on"),
        ]);
        let mut c = col("on blue");
        auto_colorize(&mut ctx, &both, &mut c).unwrap();
        assert_eq!(c, col("red on blue"));
    }

    #[test]
    fn uda_presence_value_and_none() {
        let _reset = Reset;
        let high = task(&[("estimate", "high")]);
        let low = task(&[("estimate", "low")]);
        check_rule("color.uda.estimate", &[], &high, &task(&[]));
        check_rule("color.uda.estimate.high", &[], &high, &low);
        check_rule("color.uda.estimate.none", &[], &task(&[]), &low);
    }

    #[test]
    fn non_task_rules_and_trivial_rules_are_ignored() {
        let _reset = Reset;
        let mut ctx = rules_context(&[
            ("color.header", "red"),
            ("color.tagged", ""),
            ("rule.precedence.color", "header,tagged"),
        ]);
        let mut t = task(&[]);
        t.add_tag("x");
        let mut c = col("blue");
        auto_colorize(&mut ctx, &t, &mut c).unwrap();
        assert_eq!(c, col("blue"));
    }

    #[test]
    fn precedence_first_rule_wins_without_merge() {
        let _reset = Reset;
        let mut t = task(&[("project", "home")]);
        t.add_tag("x");
        let settings = [("color.tagged", "red"), ("color.project.home", "green")];
        let mut ctx = rules_context(&[
            settings[0],
            settings[1],
            ("rule.precedence.color", "tagged,project."),
        ]);
        assert_eq!(gs_precedence(), ["color.tagged", "color.project.home"]);
        assert_eq!(colorize(&mut ctx, &t), col("red"));
        drop(ctx);

        let mut ctx = rules_context(&[
            settings[0],
            settings[1],
            ("rule.precedence.color", "project.,tagged"),
        ]);
        assert_eq!(colorize(&mut ctx, &t), col("green"));

        // Without merge the matching rule replaces a pre-assigned colour.
        let mut c = col("on blue");
        auto_colorize(&mut ctx, &t, &mut c).unwrap();
        assert_eq!(c, col("green"));
    }

    #[test]
    fn merge_blends_in_reverse_precedence_order() {
        let _reset = Reset;
        let mut t = task(&[("project", "home")]);
        t.add_tag("x");
        let mut ctx = rules_context(&[
            ("color.tagged", "red"),
            ("color.project.home", "bold green on yellow"),
            ("rule.precedence.color", "tagged,project."),
            ("rule.color.merge", "on"),
        ]);
        // project (last) blended first, then tagged on top: red foreground
        // wins, bold and the yellow background are inherited.
        assert_eq!(colorize(&mut ctx, &t), col("bold red on yellow"));

        // The pre-assigned colour is the base the rules blend into.
        let mut c = col("underline white on blue");
        auto_colorize(&mut ctx, &t, &mut c).unwrap();
        assert_eq!(c, col("bold underline red on yellow"));

        // An unmatched task keeps the pre-assigned colour.
        let mut c = col("on blue");
        auto_colorize(&mut ctx, &task(&[]), &mut c).unwrap();
        assert_eq!(c, col("on blue"));
    }

    #[test]
    fn exact_rules_take_priority_over_wildcards() {
        let _reset = Reset;
        // A task tagged literally "none" is not coloured by color.tag.none
        // (that rule means "no tags").
        let mut t = task(&[]);
        t.add_tag("none");
        check_rule("color.tag.none", &[], &task(&[]), &t);
        check_rule(
            "color.project.none",
            &[],
            &task(&[]),
            &task(&[("project", "none")]),
        );
    }

    #[test]
    fn errors_propagate() {
        let _reset = Reset;
        let bad = task(&[("status", "bogus")]);
        for rule in [
            "completed",
            "deleted",
            "due",
            "overdue",
            "due.today",
            "tag.PENDING",
        ] {
            let key = format!("color.{rule}");
            let mut ctx = rules_context(&[(&key, "red"), ("rule.precedence.color", rule)]);
            let mut bad = bad.clone();
            bad.set("due", &epoch(0));
            let mut c = Color::new();
            let err = auto_colorize(&mut ctx, &bad, &mut c).unwrap_err();
            assert_eq!(
                err.to_string(),
                "The status 'bogus' is not valid.",
                "{rule}"
            );
        }
    }
}
