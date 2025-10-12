//! Public API for interacting with running PingerActor instances.
//!
//! Provides convenient async methods for configuration and monitoring.
//! Handles Actix message sending and error conversion for callers.

use crate::actor::PingerActor;
use crate::error::PingerError;
use crate::messages::{PingerHealth, TargetConfig};
use actix::Addr;

/// A handle for interacting with a running `PingerActor`.
///
/// This struct provides a clean, asynchronous public API for controlling and monitoring
/// the pinger. It wraps the `Addr<PingerActor>` and translates method calls into
/// actor messages, abstracting away the underlying message-passing mechanism for the user.
#[derive(Clone)]
pub struct PingerHandle {
    pub(crate) addr: Addr<PingerActor>,
}

impl PingerHandle {
    /// Creates a new handle from an actor address.
    ///
    /// This is typically called by the `PingerBuilder` upon starting the actor.
    pub fn new(addr: Addr<PingerActor>) -> Self {
        Self { addr }
    }

    /// Dynamically updates the list of targets being monitored.
    ///
    /// This will replace the entire set of existing targets with the new list. The actor
    /// will gracefully stop pinging any removed targets and start pinging any new ones.
    pub async fn update_targets(&self, targets: Vec<TargetConfig>) -> Result<(), PingerError> {
        match self
            .addr
            .send(crate::messages::UpdateTargets { targets })
            .await
        {
            Ok(inner_res) => inner_res,
            Err(e) => Err(PingerError::ActorError(format!("send failed: {e}"))),
        }
    }

    /// Pauses or resumes all pinging operations.
    ///
    /// This provides a way to temporarily suspend monitoring without losing the current
    /// target configuration. When re-enabled, the pinger will resume its tasks.
    pub async fn set_enabled(&self, enabled: bool) -> Result<(), PingerError> {
        self.addr
            .send(crate::messages::SetPingingEnabled { enabled })
            .await
            .map_err(|e| PingerError::ActorError(format!("send failed: {e}")))
    }

    /// Retrieves a snapshot of the pinger's current health and operational metrics.
    ///
    /// This is a read-only operation that provides insight into the actor's state,
    /// such as the number of active targets and total pings sent.
    pub async fn get_health(&self) -> Result<PingerHealth, PingerError> {
        self.addr
            .send(crate::messages::GetHealth)
            .await
            .map_err(|e| PingerError::ActorError(format!("send failed: {e}")))
    }
}
