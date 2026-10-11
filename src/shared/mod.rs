//! Hand translation of GothenburgBitFactory/libshared (commit `d899eba6`).
//!
//! One module per libshared source file.

// Mirrors libshared's `shared.cpp`; the name intentionally matches its parent.
pub mod color;
pub mod configuration;
pub mod datetime;
pub mod duration;
pub mod format;
pub mod fs;
pub mod json;
pub mod localtime;
pub mod pig;
pub mod rx;
#[allow(clippy::module_inception)]
pub mod shared;
pub mod table;
pub mod timer;
pub mod unicode;
pub mod utf8;
pub mod wcwidth;
