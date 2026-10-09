//! Rust translation of `src/TDB2.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 4 (Task model and
//! TDB2). Until then [`Tdb2`] is the placeholder held in `Context::tdb2`; its
//! methods are the hook points `Context` already calls, and do nothing.

use crate::error::Result;
use crate::shared::fs::Path;

/// Placeholder for C++ `TDB2` (the taskchampion replica wrapper).
#[derive(Debug, Default)]
pub struct Tdb2 {
    /// C++ `static bool TDB2::debug_mode`, set by
    /// `Context::staticInitialization` from `rc.debug`.
    pub debug_mode: bool,
}

impl Tdb2 {
    /// C++ `TDB2::TDB2`.
    pub fn new() -> Self {
        Tdb2::default()
    }

    /// C++ `TDB2::open_replica (location, create_if_missing, read_write)`.
    /// Milestone 4 hook: no-op.
    pub fn open_replica(&mut self, _location: &Path, _create_if_missing: bool, _read_write: bool) -> Result<()> {
        Ok(())
    }

    /// C++ `TDB2::gc`. Milestone 4 hook: no-op.
    pub fn gc(&mut self) -> Result<()> {
        Ok(())
    }
}
