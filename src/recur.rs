//! Rust translation of `src/recur.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations,
//! hooks, recurrence, undo, sync). The two entry points `Context::dispatch`
//! calls exist already as no-op hook functions.

use crate::context::Context;
use crate::error::Result;

/// C++ `handleUntil ()`: deletes expired `until` tasks. Milestone 7 hook:
/// no-op.
pub fn handle_until(_context: &mut Context) -> Result<()> {
    Ok(())
}

/// C++ `handleRecurrence ()`: generates due recurring task instances.
/// Milestone 7 hook: no-op.
pub fn handle_recurrence(_context: &mut Context) -> Result<()> {
    Ok(())
}
