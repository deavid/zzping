//! Top-level design notes for the collector library.
//!
//! This crate provides composable components for implementing the collector.
//! It emphasizes correctness, simplicity, and modularity.
//!
//! Key principles:
//! - The orchestrator drives configuration and state transitions.
//! - Components react to orchestrator decisions, ensuring a clear separation of concerns.
//! - Focused optimizations are preferred over global complexity.
//!
pub mod batch_submitter;

pub mod cli;

pub mod database_client;

pub mod orchestrator;

pub mod ping_client;

pub mod ping_mock_client;

pub mod ping_surge_client;

pub mod runner;

pub mod state_machine;

pub mod task_supervisor;
