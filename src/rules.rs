//! Rust translation of `src/rules.cpp`.
//!
//! Ported so far: the file statics `gsColor` / `gsPrecedence`,
//! [`initialize_color_rules`] (called by `Context::initialize`), and the
//! message colourers [`colorize_header`], [`colorize_footnote`],
//! [`colorize_error`] and [`colorize_debug`] used by `Context` when it dumps
//! headers, footnotes, errors and debug messages.
//!
//! Milestone 6 hook (reports and columns): the per-task rule functions
//! (`autoColorize` and the static `colorize*` / `applyColor` helpers, and the
//! file-static `Datetime now` they compare against) are not ported yet; only
//! report rendering calls them.
//!
//! The C++ file statics become thread-local statics (the binary is
//! single-threaded; unit tests each run on their own thread).

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::context::Context;
use crate::error::{Result, TwError};
use crate::shared::color::Color;
use crate::shared::shared::{auto_complete, split};

thread_local! {
    /// C++ `static std::map<std::string, Color> gsColor`.
    static GS_COLOR: RefCell<BTreeMap<String, Color>> = const { RefCell::new(BTreeMap::new()) };
    /// C++ `static std::vector<std::string> gsPrecedence`.
    static GS_PRECEDENCE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
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
        let mut ctx = context(&[("color", "on"), ("detection", "on"), ("color.header", "red")]);
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

        assert_eq!(colorize_header("h"), Color::parse("red").unwrap().colorize("h"));
        assert_eq!(colorize_footnote("f"), Color::parse("green").unwrap().colorize("f"));
        assert_eq!(colorize_error("e"), Color::parse("white on red").unwrap().colorize("e"));
        assert_eq!(colorize_debug("d"), Color::parse("blue").unwrap().colorize("d"));
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
        let mut all: Vec<String> = ctx.config.keys().filter(|k| k.starts_with("color.")).cloned().collect();
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
}
