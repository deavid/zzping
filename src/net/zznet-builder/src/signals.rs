//! Graceful shutdown signal handling.
//!
//! Provides cross-platform signal handling for application shutdown.

use anyhow::{Context, Result};
use tracing::info;

#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

/// Manages shutdown signals (Ctrl+C, SIGTERM, etc.)
pub(crate) struct ShutdownSignals {
    #[cfg(unix)]
    sigterm: tokio::signal::unix::Signal,
    #[cfg(unix)]
    sigint: tokio::signal::unix::Signal,
}

impl ShutdownSignals {
    /// Create a new shutdown signal handler.
    ///
    /// On Unix systems, listens to SIGTERM and SIGINT (Ctrl+C).
    /// On Windows, only Ctrl+C is available (handled by Tokio).
    pub(crate) fn new() -> Result<Self> {
        #[cfg(unix)]
        {
            let sigterm =
                signal(SignalKind::terminate()).context("Failed to setup SIGTERM handler")?;
            let sigint =
                signal(SignalKind::interrupt()).context("Failed to setup SIGINT handler")?;

            Ok(Self { sigterm, sigint })
        }

        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }

    /// Wait for a shutdown signal.
    ///
    /// Blocks until SIGTERM, SIGINT (Ctrl+C), or equivalent is received.
    /// Logs which signal was received.
    pub(crate) async fn wait(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.sigterm.recv() => {
                    info!("Received SIGTERM, shutting down gracefully");
                }
                _ = self.sigint.recv() => {
                    info!("Received SIGINT (Ctrl+C), shutting down gracefully");
                }
            }
            Ok(())
        }

        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .context("Failed to listen for Ctrl+C")?;
            info!("Received Ctrl+C, shutting down gracefully");
            Ok(())
        }
    }
}

impl Default for ShutdownSignals {
    fn default() -> Self {
        Self::new().expect("Failed to create shutdown signals")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_signals_creation() {
        let _signals = ShutdownSignals::new().expect("Failed to create signals");
    }
}
