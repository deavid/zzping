//! Complete application builder for ZZNet applications.
//!
//! This module provides the `AppBuilder` - a fluent API for constructing
//! fully-configured ZZNet applications with minimal boilerplate.

use crate::cli::StandardCliArgs;
use crate::logging::init_logging_from_args;
use crate::runtime::{install_crypto_provider, run_actix};
use crate::signals::ShutdownSignals;
use crate::traits::ZZNetService;
use anyhow::{Context, Result};
use clap::Parser;

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
    pub fn new(app_name: impl Into<String>, app_version: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            app_version: app_version.into(),
            default_config_path: "config.ron".to_string(),
        }
    }

    /// Set the default configuration file path to be used if the user doesn't specify `--config` on the command line.
    pub fn with_default_config(mut self, path: impl Into<String>) -> Self {
        self.default_config_path = path.into();
        self
    }

    /// Build and run with custom CLI arguments.
    pub fn run_service<S: ZZNetService>(self) -> Result<()> {
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
