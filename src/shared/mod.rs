//! Hand translation of GothenburgBitFactory/libshared (commit `d899eba6`).
//!
//! One module per libshared source file.

// Mirrors libshared's `shared.cpp`; the name intentionally matches its parent.
#[allow(clippy::module_inception)]
pub mod shared;
pub mod format;
pub mod utf8;
pub mod unicode;
pub mod pig;
pub mod fs;
pub mod timer;
pub mod configuration;
pub mod datetime;
pub mod duration;
pub mod json;
pub mod color;
pub mod table;
pub mod rx;
