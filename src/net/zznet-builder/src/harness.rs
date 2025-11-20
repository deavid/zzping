//! Application harness for managing ZZNet application lifecycle.

use crate::runtime::{install_crypto_provider, run_actix};
use crate::signals::ShutdownSignals;
use crate::traits::ZZNetApplication;
use anyhow::Result;
use tracing::{error, info};

/// Application harness for running ZZNet applications.
///
/// Manages the application lifecycle including logging initialization, runtime setup,
/// signal handling, and graceful shutdown.
pub struct AppHarness {
    log_level: String,
}

impl Default for AppHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl AppHarness {
    /// Create a new application harness with default settings.
    ///
    /// Default log level is "info".
    pub fn new() -> Self {
        Self {
            log_level: "info".to_string(),
        }
    }

    /// Explicitly set log level (e.g., from CLI args)
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = level.into();
        self
    }

    /// Initialize logging immediately (call this early in main)
    pub fn init_logging(&self) {
        // Re-use your existing logging logic here or inline it
        // For simplicity, assuming crate::logging::init_logging exists:
        crate::logging::init_logging(&self.log_level);
    }

    /// Run the application until a shutdown signal is received.
    ///
    /// This method:
    /// 1. Installs the crypto provider
    /// 2. Starts the Actix runtime
    /// 3. Calls `app.startup()`
    /// 4. Waits for OS signals (SIGINT/SIGTERM)
    /// 5. Calls `app.shutdown()`
    ///
    /// # Errors
    ///
    /// Returns an error if startup fails, signal handling fails, or shutdown fails.
    pub fn run<A: ZZNetApplication>(self, mut app: A) -> Result<()> {
        install_crypto_provider();

        run_actix(move || async move {
            info!("Starting {}", app.service_name());

            // 1. Start the App (Actors are spawned here)
            if let Err(e) = app.startup().await {
                error!("Startup failed: {:?}", e);
                return Err(e);
            }

            info!("{} running. Waiting for signals...", app.service_name());

            // 2. Wait for OS Signals
            let mut signals = ShutdownSignals::new()?;
            signals.wait().await?;

            info!("Signal received. Shutting down {}", app.service_name());

            // 3. Shutdown the App
            if let Err(e) = app.shutdown().await {
                error!("Graceful shutdown failed: {:?}", e);
            }

            info!("{} shutdown complete.", app.service_name());
            Ok(())
        })
    }
}
