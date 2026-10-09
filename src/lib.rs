//! Taskwarrior 3.5.0, translated from C++ to Rust.
//!
//! The modules map one-to-one onto the C++ source files (see `README.md`).
//! `shared` is the hand port of libshared; `columns` and `commands` hold one
//! module per `Col*.cpp` / `Cmd*.cpp`.

pub mod error;
pub mod version;

pub mod shared;

pub mod cli2;
pub mod context;
pub mod dom;
pub mod eval;
pub mod filter;
pub mod lexer;
pub mod variant;

pub mod dependency;
pub mod feedback;
pub mod hooks;
pub mod legacy;
pub mod nag;
pub mod operation;
pub mod recur;
pub mod rules;
pub mod sort;
pub mod task;
pub mod tdb2;
pub mod tf2;
pub mod util;
pub mod view_task;

pub mod columns;
pub mod commands;

pub use error::{Result, TwError};
