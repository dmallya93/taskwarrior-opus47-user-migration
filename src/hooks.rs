//! Rust translation of `src/Hooks.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 7 (mutations,
//! hooks, recurrence, undo, sync). Until then [`Hooks`] is the placeholder
//! held in `Context::hooks`; its methods are the hook points `Context` and
//! `tdb2::add` / `tdb2::modify` already call, and do nothing.

use crate::error::Result;
use crate::task::Task;

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

    /// C++ `Hooks::onAdd (Task&)`: on-add hook scripts may replace the new
    /// task (called by `tdb2::add`). Milestone 7 hook: no-op.
    pub fn on_add(&mut self, _task: &mut Task) -> Result<()> {
        Ok(())
    }

    /// C++ `Hooks::onModify (const Task& before, Task& after)`: on-modify
    /// hook scripts may replace the modified task (called by
    /// `tdb2::modify`). Milestone 7 hook: no-op.
    pub fn on_modify(&mut self, _before: &Task, _after: &mut Task) -> Result<()> {
        Ok(())
    }
}
