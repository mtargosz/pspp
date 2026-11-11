//! `pspp` ("ps++") — interactive btop-style TUI and scriptable query CLI for
//! viewing process memory grouped by parent/child (`pid <- ppid`) relationships.
//!
//! The binary (`src/main.rs`) only parses arguments; everything else lives here so it can be
//! exercised by integration tests (`tests/model_tests.rs`).

pub mod collect;
pub mod config;
pub mod model;
pub mod query;
pub mod ui;
