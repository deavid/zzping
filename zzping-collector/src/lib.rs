//! The `zzping-collector` library.
//!
//! This crate contains the core logic for the pinging service. The main binary
//! in `main.rs` is a lightweight wrapper around the `run` function from this library.

pub mod cli;
pub mod connection_manager;
pub mod ping_client;
pub mod ping_mock_client;
pub mod ping_surge_client;
pub mod runner;
pub mod target_manager;
