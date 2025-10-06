//! The `zzping-database` library.
//!
//! This crate contains the core logic for the database service. The main binary
//! in `main.rs` is a lightweight wrapper around the `run` function from this library.

pub mod auth;
pub mod config;
pub mod finalization;
/// gRPC server for ingestion endpoints.
pub mod grpc_server;

/// Types representing an ingestion item to store.
pub mod ingestion_item;

/// Query layer utilities.
pub mod query;

/// Runner that orchestrates the binary's runtime.
pub mod runner;
/// Scheduling helpers for background tasks.
pub mod scheduler;
/// Storage engine implementation and on-disk format.
pub mod storage_engine;

/// Integration and unit test modules for the database crate.
#[cfg(test)]
pub mod tests;

/// Default address for the ingestion gRPC server.
pub const INGESTION_ADDR: &str = "127.0.0.1:7878";
/// Default directory for persisted data files.
pub const DATA_DIR: &str = "data";

#[cfg(feature = "test-utils")]
pub use grpc_server::spawn_test_server;
#[cfg(feature = "test-utils")]
pub use tokio::task::JoinHandle;
