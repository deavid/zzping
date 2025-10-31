//! ZZNet Application Builder - Complete Application Scaffolding Framework
//!
//! This crate provides a complete application framework for building ZZNet applications
//! with minimal boilerplate. It handles all the standard concerns: CLI parsing, logging,
//! configuration, TLS, runtime setup, and more.
//!
//! # Quick Start - The AppBuilder Way
//!
//! The recommended way to build ZZNet applications:
//!
//! ```rust,no_run
//! use zznet_builder::builder::AppBuilder;
//! use serde::{Deserialize, Serialize};
//! use anyhow::Result;
//!
//! #[derive(Debug, Deserialize, Serialize)]
//! struct MyConfig {
//!     host: String,
//!     port: u16,
//! }
//!
//! impl MyConfig {
//!     fn validate(&self) -> Result<()> {
//!         if self.port == 0 {
//!             anyhow::bail!("Port cannot be zero");
//!         }
//!         Ok(())
//!     }
//! }
//!
//! fn main() -> Result<()> {
//!     AppBuilder::new("MyApp", env!("CARGO_PKG_VERSION"))
//!         .with_default_config("myapp.ron")
//!         .build_and_run(|config: MyConfig| async move {
//!             config.validate()?;
//!             tracing::info!("App running on {}:{}", config.host, config.port);
//!
//!             // Your application logic here
//!             let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
//!             let _ = rx.await;
//!
//!             Ok(())
//!         })
//! }
//! ```
//!
//! # Manual Usage - Individual Utilities
//!
//! You can also use individual utilities if you need more control:
//!
//! ## TLS Configuration
//!
//! ```rust,ignore
//! use zznet_builder::tls::{load_client_tls, ClientTlsConfig};
//!
//! let tls_config = ClientTlsConfig {
//!     ca_cert_path: "certs/ca.pem",
//!     client_cert_path: "certs/client.pem",
//!     client_key_path: "certs/client.key",
//! };
//!
//! let rustls_config = load_client_tls(&tls_config)?;
//! ```
//!
//! ## Configuration Loading
//!
//! ```rust,ignore
//! use zznet_builder::config::load_ron_config;
//!
//! #[derive(Deserialize)]
//! struct MyConfig {
//!     host: String,
//!     port: u16,
//! }
//!
//! let config: MyConfig = load_ron_config("config.ron")?;
//! ```
//!
//! # Modules
//!
//! - `builder` - Complete application builder (recommended)
//! - `cli` - Standard CLI argument parsing
//! - `config` - Configuration file loading
//! - `logging` - Logging initialization
//! - `runtime` - Runtime and crypto provider setup
//! - `tls` - TLS certificate loading
//! - `error` - Error types

pub mod builder;
pub mod cli;
pub mod config;
pub mod error;
pub mod logging;
pub mod runtime;
pub mod signals;
pub mod tls;
pub mod traits;

pub use error::{Error, Result};
