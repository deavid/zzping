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
        // Default implementation uses OS signals to stop the service.
        // We create a oneshot receiver that never receives, so awaiting it is
        // equivalent to waiting indefinitely and lets OS signals trigger the shutdown.
        let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
        self.run_service_with_stop::<S, _>(async move {
            let _ = rx.await;
        })
        .map(|_| ())
    }

    /// Run the service but allow the caller to provide an explicit "stop" future.
    ///
    /// This is useful for tests or environments where OS signals are not suitable
    /// and a programmatic shutdown is required. The provided `stop` future is
    /// awaited in parallel with OS signals; whichever resolves first will cause
    /// shutdown to proceed.
    pub(crate) fn run_service_with_stop<S, StopFut>(self, stop: StopFut) -> Result<()>
    where
        S: ZZNetService,
        StopFut: std::future::Future<Output = ()> + Send + 'static,
    {
        // Install crypto provider early
        install_crypto_provider();

        // Parse CLI arguments. `try_parse()` will return an Error instead of panicking
        // when test runners pass their own unknown flags (e.g., nextest `--exact`).
        // Fall back to default CLI args when parsing fails to keep behavior stable
        // under test runners and in non-standard environments.
        let mut args = match StandardCliArgs::try_parse() {
            Ok(a) => a,
            Err(_) => StandardCliArgs {
                config: self.default_config_path.clone(),
                debug: false,
                trace: false,
            },
        };

        // If no config was provided via CLI use the builder's default config path
        if args.config == "config.ron" {
            args.config = self.default_config_path.clone();
        }

        // Initialize logging
        init_logging_from_args(&args);

        // Log startup
        tracing::info!("{} v{} starting", self.app_name, self.app_version);
        tracing::info!("Loading configuration from: {}", args.config);

        // Load configuration
        let content = std::fs::read_to_string(&args.config)
            .map_err(|e| anyhow::anyhow!("Failed to read config file {}: {}", args.config, e))
            .with_context(|| format!("Failed to load configuration from {}", args.config))?;

        let config: S::Config = ron::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))
            .with_context(|| format!("Failed to load configuration from {}", args.config))?;

        tracing::info!("Configuration loaded successfully");

        // Run in Actix runtime
        run_actix(|| async move {
            // Create service
            let mut service = S::new(config)
                .map_err(|e| anyhow::anyhow!(e))
                .with_context(|| format!("Failed to create {} service", S::service_name()))?;

            // Run service
            service
                .startup()
                .await
                .map_err(|e| anyhow::anyhow!(e))
                .with_context(|| format!("{} service failed", S::service_name()))?;

            tracing::info!(
                "{} started successfully; waiting for shutdown signal",
                S::service_name()
            );

            // Wait for either a programmatic stop (test harness) or OS signals
            let mut signals = ShutdownSignals::new()?;
            tokio::select! {
                _ = signals.wait() => { /* OS signal received */ }
                _ = stop => { /* programmatic stop requested */ }
            }

            tracing::info!("Signal received. Initiating graceful shutdown...");

            // Call service shutdown hook
            if let Err(e) = service.shutdown().await {
                tracing::error!("Graceful shutdown failed: {}", e.into());
            }

            tracing::info!("{} shutdown complete", S::service_name());
            Ok(())
        })
    }

    /// Run the service using the provided `config` object and allow a custom stop future.
    pub fn run_service_with_config_and_stop<S, StopFut>(
        self,
        config: S::Config,
        stop: StopFut,
    ) -> Result<()>
    where
        S: ZZNetService,
        StopFut: std::future::Future<Output = ()> + Send + 'static,
    {
        // Install crypto provider early
        install_crypto_provider();

        // Initialize logging using CLI args but we don't need a config file, so parse args and initialize
        let args = match StandardCliArgs::try_parse() {
            Ok(a) => a,
            Err(_) => StandardCliArgs {
                config: self.default_config_path.clone(),
                debug: false,
                trace: false,
            },
        };
        init_logging_from_args(&args);

        // Log startup
        tracing::info!("{} v{} starting", self.app_name, self.app_version);

        // Run in Actix runtime
        run_actix(|| async move {
            // Build service from provided config
            let mut service = S::new(config)
                .map_err(|e| anyhow::anyhow!(e))
                .with_context(|| format!("Failed to create {} service", S::service_name()))?;

            // Run service, which may start actors/tasks and return immediately
            service
                .startup()
                .await
                .map_err(|e| anyhow::anyhow!(e))
                .with_context(|| format!("{} service failed", S::service_name()))?;

            tracing::info!(
                "{} started successfully; waiting for shutdown signal",
                S::service_name()
            );

            // Wait for either OS signals or programmatic stop
            let mut signals = ShutdownSignals::new()?;
            tokio::select! {
                _ = signals.wait() => { /* os signal */ }
                _ = stop => { /* programmatic stop */ }
            }

            tracing::info!("Signal received. Initiating graceful shutdown...");

            // Call service shutdown hook
            if let Err(e) = service.shutdown().await {
                tracing::error!("Graceful shutdown failed: {}", e.into());
            }

            tracing::info!("{} shutdown complete", S::service_name());
            Ok(())
        })
    }

    /// Construct a service instance from a provided configuration object and return it.
    /// This allows tests and other code to exercise `S::new(config)` using the
    /// builder's standard config resolution and validation logic without running
    /// the full application runtime.
    pub fn build_service_from_config<S>(&self, config: S::Config) -> Result<S>
    where
        S: ZZNetService,
    {
        // Delegate to the service constructor. Validation should be handled by the
        // service or the builder; callers using this method must ensure config
        // is validated if required.
        S::new(config).map_err(|e| anyhow::anyhow!(e))
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

    #[test]
    fn test_init_logging_from_args_does_not_panic() {
        // Build args with --debug
        let args = crate::cli::StandardCliArgs {
            config: "x.ron".into(),
            debug: true,
            trace: false,
        };

        init_logging_from_args(&args);
        // No panic means it is ok
    }

    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, Deserialize, Serialize)]
    struct DummyConfig {
        pub name: String,
    }

    impl crate::traits::ZZNetConfig for DummyConfig {}

    struct DummyService {
        _cfg: DummyConfig,
    }

    use async_trait::async_trait;

    #[async_trait]
    impl crate::traits::ZZNetService for DummyService {
        type Config = DummyConfig;
        type Error = anyhow::Error;

        fn new(config: Self::Config) -> Result<Self, Self::Error> {
            Ok(Self { _cfg: config })
        }

        async fn startup(&mut self) -> Result<(), Self::Error> {
            // Simulate a short-lived startup and then return Ok
            tracing::info!("DummyService started");
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_build_service_from_config_works() {
        let builder = AppBuilder::new("Dummy", "0.1");
        let cfg = DummyConfig { name: "x".into() };
        let svc = builder
            .build_service_from_config::<DummyService>(cfg)
            .unwrap();
        // Ensure that the service was constructed
        let _ = svc;
    }

    #[tokio::test]
    async fn test_run_service_with_config_and_stop_works() {
        let builder = AppBuilder::new("Dummy", "0.1");
        let cfg = DummyConfig { name: "x".into() };

        // Create a stop future that resolves after a short delay
        let stop_fut = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
        };

        // Run the service in background: spawn in a dedicated task
        let handle = tokio::task::spawn_blocking(move || {
            // This will block the current thread and run the actix system inside.
            builder
                .run_service_with_config_and_stop::<DummyService, _>(cfg, stop_fut)
                .unwrap();
        });

        // Wait for the background run to finish after stop occurs
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_run_service_with_config_and_stop_fails_when_new_errors() {
        #[derive(Debug, Deserialize, Serialize)]
        struct FailConfig {
            pub name: String,
        }

        impl crate::traits::ZZNetConfig for FailConfig {}

        struct FailService;

        #[async_trait]
        impl crate::traits::ZZNetService for FailService {
            type Config = FailConfig;
            type Error = anyhow::Error;

            fn new(_config: Self::Config) -> Result<Self, Self::Error> {
                Err(anyhow::anyhow!("explicit failure in new"))
            }

            async fn startup(&mut self) -> Result<(), Self::Error> {
                Ok(())
            }
        }

        let builder = AppBuilder::new("Fail", "0.1");
        let cfg = FailConfig { name: "x".into() };

        let stop_fut = async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };

        // Running should produce an Err because `new` fails.
        let handle = tokio::task::spawn_blocking(move || {
            builder.run_service_with_config_and_stop::<FailService, _>(cfg, stop_fut)
        });

        let res = handle.await.unwrap();
        assert!(res.is_err(), "builder returned Ok for failing service");
    }
}
