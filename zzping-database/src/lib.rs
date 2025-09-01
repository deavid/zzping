//! The `zzping-database` library.
//!
//! This crate contains the core logic for the database service. The main binary
//! in `main.rs` is a lightweight wrapper around the `run` function from this library.

pub mod finalization;
pub mod grpc_server;
pub mod ingestion;
pub mod ingestion_item;
pub mod query;
pub mod runner;
pub mod storage_engine;

pub const INGESTION_ADDR: &str = "127.0.0.1:7878";
pub const DATA_DIR: &str = "data";

#[cfg(feature = "test-utils")]
pub use grpc_server::{spawn_test_server, timeout};
