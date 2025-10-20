//! Collector application library.
//!
//! Contains testable business logic separated from main() entry point.
//! This allows unit testing of configuration, service orchestration, and
//! component integration without running the full binary.

// Module declarations
pub mod cli;
pub mod config;
pub mod error;
/// Network wiring module (collector)
pub mod network;
/// Room handler factories for component wiring
pub mod room_handlers;
/// Service module.
pub mod service;
