//! Core traits for the ZZNet application builder framework.
//!
//! Decouples services from the `AppBuilder` by defining a standard contract
//! for lifecycle management and configuration.

use async_trait::async_trait;
use serde::de::DeserializeOwned;

/// A marker trait that allows a struct to be used as a service's
/// deserializable configuration.
pub trait ZZNetConfig: DeserializeOwned + Send + Sync + 'static {}

/// Defines a service's lifecycle, allowing the `AppBuilder` to manage
/// its construction, execution, and identity.
#[async_trait]
pub trait ZZNetService: Sized + Send + 'static {
    /// The type-safe, deserializable configuration for the service.
    type Config: ZZNetConfig;

    /// A service-specific error type that can be unified by the `AppBuilder`.
    type Error: Into<anyhow::Error> + Send + Sync + 'static;

    /// Constructs the service, separating configuration-based setup
    /// from the execution logic in `run`.
    fn new(config: Self::Config) -> Result<Self, Self::Error>;

    /// Contains the service's primary logic, such as starting actors or listeners.
    async fn startup(&mut self) -> Result<(), Self::Error>;

    /// Graceful shutdown hook for cleaning up resources.
    async fn shutdown(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Provides a stable, human-readable identifier for logging.
    fn service_name() -> &'static str {
        std::any::type_name::<Self>()
    }
}
