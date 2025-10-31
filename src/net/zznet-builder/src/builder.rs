//! Complete application builder for ZZNet applications.
//!
//! This module provides the `AppBuilder` - a fluent API for constructing
//! fully-configured ZZNet applications with minimal boilerplate.
//!
//! # Architecture
//!
//! The builder handles all standard application concerns:
//! - Command-line argument parsing
//! - Logging initialization
//! - Configuration loading and validation
//! - TLS setup (optional)
//! - Actix runtime setup
//! - Crypto provider installation
//!
//! # Example
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
//!     AppBuilder::new("MyApp", "1.0.0")
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

use crate::cli::StandardCliArgs;
use crate::logging::init_logging_from_args;
use crate::runtime::{install_crypto_provider, run_actix};
use crate::signals::ShutdownSignals;
use crate::traits::{ZZNetConfig, ZZNetService};
use anyhow::{Context, Result};
use clap::Parser;
use serde::de::DeserializeOwned;
use std::future::Future;

/// Builder for ZZNet applications.
///
/// Provides a fluent API for constructing applications with all the standard
/// infrastructure: CLI parsing, logging, configuration, runtime setup, etc.
pub struct AppBuilder {
    app_name: String,
    app_version: String,
    default_config_path: String,
}

impl AppBuilder {
    /// Create a new application builder.
    ///
    /// # Arguments
    ///
    /// * `app_name` - Display name of the application
    /// * `app_version` - Version string (typically from CARGO_PKG_VERSION)
    ///
    /// # Example
    ///
    /// ```rust
    /// use zznet_builder::builder::AppBuilder;
    ///
    /// let builder = AppBuilder::new("MyApp", env!("CARGO_PKG_VERSION"));
    /// ```
    pub fn new(app_name: impl Into<String>, app_version: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            app_version: app_version.into(),
            default_config_path: "config.ron".to_string(),
        }
    }

    /// Set the default configuration file path.
    ///
    /// This will be used if the user doesn't specify `--config` on the command line.
    ///
    /// # Example
    ///
    /// ```rust
    /// use zznet_builder::builder::AppBuilder;
    ///
    /// let builder = AppBuilder::new("MyApp", "1.0.0")
    ///     .with_default_config("myapp.ron");
    /// ```
    pub fn with_default_config(mut self, path: impl Into<String>) -> Self {
        self.default_config_path = path.into();
        self
    }

    /// Build and run the application with the provided async function.
    ///
    /// This method:
    /// 1. Installs the crypto provider
    /// 2. Parses CLI arguments
    /// 3. Initializes logging
    /// 4. Loads and validates configuration
    /// 5. Sets up Actix runtime
    /// 6. Runs your application function
    ///
    /// # Type Parameters
    ///
    /// * `C` - Configuration type (must implement `DeserializeOwned + Send + 'static`)
    /// * `F` - Your application function
    /// * `Fut` - The future returned by your function
    ///
    /// # Arguments
    ///
    /// * `app_fn` - Your application logic as an async closure that receives
    ///   the loaded configuration and returns a `Result<()>`
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use zznet_builder::builder::AppBuilder;
    /// use serde::{Deserialize, Serialize};
    /// use anyhow::Result;
    ///
    /// #[derive(Debug, Deserialize, Serialize)]
    /// struct Config {
    ///     message: String,
    /// }
    ///
    /// fn main() -> Result<()> {
    ///     AppBuilder::new("MyApp", "1.0.0")
    ///         .build_and_run(|config: Config| async move {
    ///             tracing::info!("Message: {}", config.message);
    ///             Ok(())
    ///         })
    /// }
    /// ```
    pub fn build_and_run<C, F, Fut>(self, app_fn: F) -> Result<()>
    where
        C: DeserializeOwned + Send + 'static,
        F: FnOnce(C) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send,
    {
        // Install crypto provider early
        install_crypto_provider();

        // Parse CLI arguments
        let args = StandardCliArgs::parse();

        // Initialize logging
        init_logging_from_args(&args);

        // Log startup
        tracing::info!("{} v{} starting", self.app_name, self.app_version);
        tracing::info!("Loading configuration from: {}", args.config);

        // Load configuration
        let config: C = crate::config::load_ron_config(&args.config)
            .map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))
            .with_context(|| format!("Failed to load configuration from {}", args.config))?;

        tracing::info!("Configuration loaded successfully");

        // Run in Actix runtime
        run_actix(|| async move { app_fn(config).await })
    }

    /// Build and run with custom CLI arguments.
    ///
    /// Use this when you need application-specific CLI arguments beyond
    /// the standard `--config`, `--debug`, and `--trace`.
    ///
    /// # Type Parameters
    ///
    /// * `A` - CLI arguments type (must implement `Parser`)
    /// * `C` - Configuration type
    /// * `F` - Your application function
    /// * `Fut` - The future returned by your function
    ///
    /// # Arguments
    ///
    /// * `app_fn` - Your application logic receiving both CLI args and config
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use zznet_builder::builder::AppBuilder;
    /// use zznet_builder::cli::StandardCliArgs;
    /// use clap::Parser;
    /// use serde::{Deserialize, Serialize};
    /// use anyhow::Result;
    ///
    /// #[derive(Parser)]
    /// struct MyArgs {
    ///     #[command(flatten)]
    ///     standard: StandardCliArgs,
    ///
    ///     #[arg(long)]
    ///     custom: String,
    /// }
    ///
    /// #[derive(Deserialize, Serialize)]
    /// struct Config {
    ///     value: i32,
    /// }
    ///
    /// fn main() -> Result<()> {
    ///     AppBuilder::new("MyApp", "1.0.0")
    ///         .build_and_run_with_args(|args: MyArgs, config: Config| async move {
    ///             tracing::info!("Custom arg: {}", args.custom);
    ///             tracing::info!("Config value: {}", config.value);
    ///             Ok(())
    ///         })
    /// }
    /// ```
    /// Build and run a service using the trait-based lifecycle.
    ///
    /// This is the recommended way to build ZZNet applications. It handles:
    /// 1. Crypto provider installation
    /// 2. CLI argument parsing
    /// 3. Logging initialization
    /// 4. Configuration loading and validation
    /// 5. Service creation and startup
    /// 6. Graceful shutdown on signals
    /// 7. Complete error handling with context
    ///
    /// # Type Parameters
    ///
    /// * `S` - Service type implementing `ZZNetService`
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use zznet_builder::builder::AppBuilder;
    /// use zznet_builder::traits::{ZZNetService, ZZNetConfig};
    /// use anyhow::Result;
    /// use async_trait::async_trait;
    /// use serde::Deserialize;
    ///
    /// #[derive(Clone, Deserialize)]
    /// struct MyConfig {}
    /// impl ZZNetConfig for MyConfig {
    ///    fn validate(&self) -> Result<()> { Ok(()) }
    /// }
    ///
    /// // Your service must implement the trait
    /// struct MyService { /* ... */ }
    /// #[async_trait]
    /// impl ZZNetService for MyService {
    ///     type Config = MyConfig;
    ///     type Error = anyhow::Error;
    ///     fn new(config: MyConfig) -> Result<Self> { /* ... */ Ok(Self {}) }
    ///     async fn run(self) -> Result<(), Self::Error> { /* ... */ Ok(()) }
    /// }
    ///
    /// fn main() -> Result<()> {
    ///     AppBuilder::new("MyApp", "1.0.0")
    ///         .run_service::<MyService>()
    /// }
    /// ```
    pub fn run_service<S>(self) -> Result<()>
    where
        S: ZZNetService,
    {
        // Install crypto provider early
        install_crypto_provider();

        // Parse CLI arguments, using default if not provided
        let mut args = StandardCliArgs::parse();
        if args.config.is_empty() {
            args.config = self.default_config_path;
        }

        // Initialize logging
        init_logging_from_args(&args);

        // Log startup
        tracing::info!("{} v{} starting", self.app_name, self.app_version);
        tracing::info!("Loading configuration from: {}", args.config);

        // Load configuration
        let config: S::Config = crate::config::load_ron_config(&args.config)
            .map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))
            .with_context(|| format!("Failed to load configuration from {}", args.config))?;

        tracing::info!("Configuration loaded successfully");

        // Run in Actix runtime
        run_actix(|| async move {
            // Validate configuration
            config
                .validate()
                .context("Configuration validation failed")?;

            // Log app-specific startup info
            config.log_startup_info();

            // Create service
            let service = S::new(config)
                .map_err(|e| anyhow::anyhow!(e))
                .with_context(|| format!("Failed to create {} service", S::service_name()))?;

            // Run service
            service
                .run()
                .await
                .map_err(|e| anyhow::anyhow!(e))
                .with_context(|| format!("{} service failed", S::service_name()))?;

            tracing::info!(
                "{} started successfully; waiting for shutdown signal",
                S::service_name()
            );

            // Wait for shutdown signal
            let mut signals = ShutdownSignals::new()?;
            signals.wait().await?;

            tracing::info!("{} shutdown complete", S::service_name());
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_builder_creation() {
        let builder = AppBuilder::new("TestApp", "1.0.0");
        assert_eq!(builder.app_name, "TestApp");
        assert_eq!(builder.app_version, "1.0.0");
        assert_eq!(builder.default_config_path, "config.ron");
    }

    #[test]
    fn test_with_default_config() {
        let builder = AppBuilder::new("TestApp", "1.0.0").with_default_config("custom.ron");
        assert_eq!(builder.default_config_path, "custom.ron");
    }
}
