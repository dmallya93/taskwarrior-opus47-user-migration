//! Rust translation of `src/rules.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 6 (reports and
//! columns). The entry point `Context::initialize` calls exists already as a
//! hook: [`initialize_color_rules`] performs the C++ short-circuit (which has
//! a side effect `version` and every width-dependent command rely on) and
//! leaves the rule loading to milestone 6.

use crate::context::Context;

/// C++ `initializeColorRules ()`.
///
/// The C++ first calls `Context::color ()`, which decides colour use once
/// and, when stdout is not a TTY, turns `detection` and `color` off in the
/// configuration (so `getWidth` falls back to `defaultwidth`). That part is
/// ported. Milestone 6 hook: loading the `color.*` rules into `gsColor` /
/// `gsPrecedence` is not done yet.
pub fn initialize_color_rules(context: &mut Context) {
    // If color is not enable/supported, short circuit.
    // (C++ `if (!color ()) return;`: both branches end here until milestone 6.)
    let use_color = context.color();

    // Milestone 6 hook: when `use_color`, load the rules.
    let _ = use_color;
}
