//! The `zzping-database` library.
//!
//! This crate contains the core logic for the database service. The main binary
//! in `main.rs` is a lightweight wrapper around the `run` function from this library.

pub mod auth;
pub mod config;
pub mod finalization;
pub mod grpc_server;
pub mod ingestion_item;
pub mod query;
pub mod runner;
pub mod scheduler;
pub mod storage_engine;

#[cfg(test)]
pub mod tests;

/// Default address for the ingestion gRPC server.
pub const INGESTION_ADDR: &str = "127.0.0.1:7878";
/// Default directory for persisted data files.
pub const DATA_DIR: &str = "data";
