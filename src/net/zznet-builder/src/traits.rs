//! # ZZNet Service and Configuration Traits
//!
//! This module defines the core traits (`ZZNetService` and `ZZNetConfig`) that enable
//! the `AppBuilder` to manage the entire lifecycle of a ZZNet application in a
//! generic, type-safe, and boilerplate-free way.
//!
//! By implementing these traits for your application's service and configuration
//! structs, you delegate all the repetitive setup, execution, and shutdown logic
//! to the builder. This allows you to reduce your `main.rs` to just a few lines.
//!
//! ## `ZZNetConfig`
//!
//! The `ZZNetConfig` trait defines a protocol for application configurations. It
//! requires configurations to be deserializable, and provides hooks for validation
//! and logging.
//!
//! ### Example
//!
//! ```rust
//! use serde::Deserialize;
//! use zznet_builder::traits::ZZNetConfig;
//!
//! #[derive(Deserialize)]
//! pub struct MyAppConfig {
//!     address: String,
//!     port: u16,
//! }
//!
//! impl ZZNetConfig for MyAppConfig {
//!     fn validate(&self) -> anyhow::Result<()> {
//!         if self.port == 0 {
//!             anyhow::bail!("Port cannot be 0");
//!         }
//!         Ok(())
//!     }
//!
//!     fn log_startup_info(&self) {
//!         tracing::info!("Binding to {}:{}", self.address, self.port);
//!     }
//! }
//! ```
//!
//! ## `ZZNetService`
//!
//! The `ZZNetService` trait defines the lifecycle of a ZZNet application service.
//! It requires a service to be constructible from a `ZZNetConfig` and to have an
//! async `run` method.
//!
//! ### Example
//!
//! ```rust
//! use anyhow::Result;
//! use zznet_builder::traits::{ZZNetConfig, ZZNetService};
//! # use serde::Deserialize;
//!
//! # #[derive(Deserialize, Clone)]
//! # pub struct MyAppConfig {}
//! # impl ZZNetConfig for MyAppConfig {
//! #     fn validate(&self) -> anyhow::Result<()> { Ok(()) }
//! # }
//!
//! pub struct MyAppService;
//!
//! #[async_trait::async_trait]
//! impl ZZNetService for MyAppService {
//!     type Config = MyAppConfig;
//!     type Error = anyhow::Error;
//!
//!     fn new(config: Self::Config) -> Result<Self> {
//!         // Service construction logic
//!         Ok(Self)
//!     }
//!
//!     async fn run(self) -> Result<(), Self::Error> {
//!         // Main service logic, e.g., starting actors, listening for connections.
//!         // This method should return when the service is running, not when it
//!         // shuts down. The builder handles waiting for a shutdown signal.
//!         tracing::info!("My service is running!");
//!         Ok(())
//!     }
//! }
//! ```

use async_trait::async_trait;
use serde::de::DeserializeOwned;

/// A protocol for application configurations, enabling validation and startup logging.
pub trait ZZNetConfig: DeserializeOwned + Send + Sync + 'static {}

/// A protocol defining the lifecycle of a ZZNet application service.
#[async_trait]
pub trait ZZNetService: Sized + Send + 'static {
    /// The configuration type required by this service. Must implement `ZZNetConfig`.
    type Config: ZZNetConfig;

    /// The error type returned by the service's `new` and `run` methods.
    type Error: Into<anyhow::Error> + Send + Sync + 'static;

    /// Creates a new instance of the service from a validated configuration.
    fn new(config: Self::Config) -> Result<Self, Self::Error>;

    /// Starts the service and its components.
    ///
    /// This method should be responsible for initializing and starting all actors
    /// and long-running tasks. It should return `Ok(())` once the service is
    /// successfully running. The `AppBuilder` will handle waiting for a shutdown
    /// signal and orchestrating a graceful exit.
    async fn run(self) -> Result<(), Self::Error>;

    /// Returns the name of the service for logging purposes.
    ///
    /// The default implementation returns the type name of the service struct.
    /// It's recommended to override this to provide a more human-friendly name.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use anyhow::Result;
    /// # use async_trait::async_trait;
    /// # use zznet_builder::traits::{ZZNetConfig, ZZNetService};
    /// # use serde::Deserialize;
    /// #
    /// # #[derive(Deserialize, Clone)]
    /// # pub struct MyConfig {}
    /// # impl ZZNetConfig for MyConfig {
    /// #     fn validate(&self) -> anyhow::Result<()> { Ok(()) }
    /// # }
    /// # pub struct MyService;
    /// #[async_trait]
    /// impl ZZNetService for MyService {
    /// #    type Config = MyConfig;
    /// #    type Error = anyhow::Error;
    /// #    fn new(config: Self::Config) -> Result<Self> { Ok(Self) }
    /// #    async fn run(self) -> Result<(), Self::Error> { Ok(()) }
    ///     fn service_name() -> &'static str {
    ///         "My Awesome App"
    ///     }
    /// }
    /// ```
    fn service_name() -> &'static str {
        std::any::type_name::<Self>()
    }
}
