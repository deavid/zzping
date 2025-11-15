//! Core traits for the ZZNet application builder framework.
//!
//! This module defines the essential abstractions that allow a ZZNet application
//! to be managed by the `AppBuilder`. By implementing these traits, an application
//! provides the necessary hooks for the builder to handle its full lifecycle,
//! from configuration loading to graceful shutdown.

use async_trait::async_trait;
use serde::de::DeserializeOwned;

/// A marker trait for service configuration structs.
///
/// This trait must be implemented by any struct that serves as the configuration
/// for a `ZZNetService`. It acts as a bound, ensuring that the configuration
/// can be deserialized from a file (e.g., RON) and safely shared across threads.
///
/// # Example
///
/// ```rust,ignore
/// use serde::Deserialize;
/// use zznet_builder::traits::ZZNetConfig;
///
/// #[derive(Deserialize)]
/// pub struct MyServiceConfig {
///     pub listen_address: String,
///     pub thread_pool_size: usize,
/// }
///
/// impl ZZNetConfig for MyServiceConfig {}
/// ```
pub trait ZZNetConfig: DeserializeOwned + Send + Sync + 'static {}

/// A trait that defines the lifecycle of a ZZNet service.
///
/// This is the central trait for any application that will be run by the `AppBuilder`.
/// It establishes a contract for how the service is constructed and executed.
#[async_trait]
pub trait ZZNetService: Sized + Send + 'static {
    /// The configuration type for this service.
    ///
    /// This associated type must be a struct that implements the `ZZNetConfig` trait.
    /// The `AppBuilder` will deserialize this configuration from a file and pass it
    /// to the `new` method.
    type Config: ZZNetConfig;

    /// The error type for this service.
    ///
    /// Any errors returned during the service's lifecycle must be convertible into
    /// `anyhow::Error` to allow for standardized error handling and reporting by the builder.
    type Error: Into<anyhow::Error> + Send + Sync + 'static;

    /// Creates a new instance of the service from its configuration.
    ///
    /// This function is called by the `AppBuilder` after it has successfully loaded
    /// and deserialized the configuration file. Implementors should perform any
    /// necessary setup that does not involve starting long-running tasks (which
    /// should be handled in `run`).
    ///
    /// # Errors
    ///
    /// Returns an error if the service cannot be constructed, for example, due to
    /// invalid configuration values.
    fn new(config: Self::Config) -> Result<Self, Self::Error>;

    /// Starts the service and runs its main logic.
    ///
    /// This method is called by the `AppBuilder` after the service is constructed.
    /// It should start any necessary actors, background tasks, or network listeners.
    ///
    /// The `run` method can either complete immediately (if it spawns all work into
    /// background tasks) or it can run indefinitely until a shutdown signal is received.
    /// The `AppBuilder` will handle waiting for shutdown signals externally.
    ///
    /// # Errors
    ///
    /// Returns an error if the service fails during its startup sequence.
    async fn run(self) -> Result<(), Self::Error>;

    /// Returns a human-readable name for the service, used in logging.
    ///
    /// The default implementation returns the struct's type name, which is usually
    /// sufficient.
    fn service_name() -> &'static str {
        std::any::type_name::<Self>()
    }
}
