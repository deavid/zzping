//! # zznet-builder
//!
//! High-level builder API for creating ZZPing network servers and clients.
//!
//! This crate provides fluent builder APIs that simplify the process of creating
//! network servers and clients with automatic HELLO handshake, SessionManager
//! integration, and connection lifecycle management.
//!
//! ## Features
//!
//! - **ServerBuilder**: Create TCP servers that accept multiple connections
//! - **ClientBuilder**: Create TCP clients with automatic reconnection
//! - **TLS Support**: Optional mutual TLS authentication
//! - **Automatic Connection Management**: Spawn HelloActors, manage lifecycle
//! - **SessionManager Integration**: Seamless integration with typed message routing
//!
//! ## Example: Server / Client

pub mod client_builder;
pub mod error;
pub mod server_builder;

pub use client_builder::ClientBuilder;
pub use error::{BuilderError, BuilderResult};
pub use server_builder::ServerBuilder;
