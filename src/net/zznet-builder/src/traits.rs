//! Core traits for the ZZNet runtime harness.
//!
//! Defines the lifecycle contract that applications must implement so the
//! `AppHarness` can manage startup, shutdown, and logging.

use anyhow::Result;
use async_trait::async_trait;

/// Represents a fully constructed application ready to run.
///
/// Implementors of this trait should hold their dependencies (Builders) in `Option` fields.
/// During `startup()`, they should take those builders, start them, and store the resulting
/// Actor Addresses in other `Option` fields.
#[async_trait]
pub trait ZZNetApplication: Send + 'static {
    /// Initialize and start all internal actors/tasks.
    /// This is called AFTER the Tokio/Actix runtime is active.
    async fn startup(&mut self) -> Result<()>;

    /// Graceful shutdown hook.
    /// Called when a shutdown signal (Ctrl+C) is received.
    /// Use this to flush buffers and stop actors using stored Addresses.
    async fn shutdown(&mut self) -> Result<()>;

    /// Application name for logging purposes.
    fn service_name(&self) -> &str;
}
