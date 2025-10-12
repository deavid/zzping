//! Public API for interacting with running PingerActor instances.
//!
//! Provides convenient async methods for configuration and monitoring.
//! Handles Actix message sending and error conversion for callers.

use crate::actor::PingerActor;
use crate::error::PingerError;
use crate::messages::{PingerHealth, TargetConfig};
use actix::Addr;

/// Handle for interacting with a running PingerActor. Wraps Addr and provides async convenience methods.
/// Converts Actix send errors into PingerError for consistent error handling.
#[derive(Clone)]
pub struct PingerHandle {
    addr: Addr<PingerActor>,
}

impl PingerHandle {
    /// Creates a new handle from an actor address. Used internally by PingerBuilder.
    pub fn new(addr: Addr<PingerActor>) -> Self {
        Self { addr }
    }

    /// Updates the list of ping targets. Sends UpdateTargets message and awaits response.
    /// Validates targets before applying to prevent runtime errors.
    pub async fn update_targets(&self, targets: Vec<TargetConfig>) -> Result<(), PingerError> {
        self.addr
            .send(crate::messages::UpdateTargets { targets })
            .await
            .map_err(|e| PingerError::ActorError(format!("send failed: {}", e)))??;
        Ok(())
    }

    /// Enables or disables pinging. Sends SetPingingEnabled message.
    /// Allows runtime control of ping operations without reconfiguration.
    pub async fn set_enabled(&self, enabled: bool) -> Result<(), PingerError> {
        self.addr
            .send(crate::messages::SetPingingEnabled { enabled })
            .await
            .map_err(|e| PingerError::ActorError(format!("send failed: {}", e)))?;
        Ok(())
    }

    /// Retrieves current health status. Sends GetHealth message and returns snapshot.
    /// Provides operational metrics for monitoring without side effects.
    pub async fn get_health(&self) -> Result<PingerHealth, PingerError> {
        let h = self
            .addr
            .send(crate::messages::GetHealth)
            .await
            .map_err(|e| PingerError::ActorError(format!("send failed: {}", e)))?;
        Ok(h)
    }
}
