//! Rust translation of `src/columns/Column.cpp`.
//!
//! Not yet ported: this module is filled in by milestone 6 (reports and columns).
//!
//! Defines the `Column` trait and the column factory. For now [`Column`] is
//! an empty trait so `Context::columns` (C++ `std::map<std::string,
//! Column*>`) can be declared.

/// C++ `Column`. Milestone 6 adds its members and `Column::factory`.
pub trait Column {}
