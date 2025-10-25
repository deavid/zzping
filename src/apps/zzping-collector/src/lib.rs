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
// pub mod room_handlers;  // DELETED - Phase 3 migration: AI-generated RoomHandlerFactory pattern removed
/// Service module.
pub mod service;
