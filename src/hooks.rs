//! Rust translation of `src/Hooks.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations,
//! hooks, recurrence, undo, sync). Until then [`Hooks`] is the placeholder
//! held in `Context::hooks`; its methods are the hook points `Context`
//! already calls, and do nothing.

use crate::error::Result;

/// Placeholder for C++ `Hooks`.
#[derive(Debug, Default)]
pub struct Hooks {}

impl Hooks {
    /// C++ `Hooks::Hooks`.
    pub fn new() -> Self {
        Hooks::default()
    }

    /// C++ `Hooks::initialize`. Milestone 7 hook: no-op.
    pub fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    /// C++ `Hooks::onLaunch`. Milestone 7 hook: no-op.
    pub fn on_launch(&mut self) -> Result<()> {
        Ok(())
    }

    /// C++ `Hooks::onExit`. Milestone 7 hook: no-op.
    pub fn on_exit(&mut self) -> Result<()> {
        Ok(())
    }
}
