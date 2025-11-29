//! Database application library.
//!
//! Contains testable business logic separated from main() entry point.
//! This allows unit testing of configuration, service orchestration, and
//! component integration without running the full binary.

pub mod config;
pub mod error;
