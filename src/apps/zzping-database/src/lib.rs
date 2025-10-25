//! Database application library.
//!
//! Contains testable business logic separated from main() entry point.
//! This allows unit testing of configuration, service orchestration, and
//! component integration without running the full binary.

// Module declarations
pub mod cli;
pub mod config;
pub mod error;
pub mod network;
// pub mod room_handlers;  // DELETED: Phase 3 - AI-generated factories not needed with proper Room<T> pattern
pub mod service;
