//! Provides a public API handle for interacting with the `CStateActor`.

use crate::actor::CStateActor;
use crate::messages::{
    CStateError, CStateHealth, GetCollectorState, GetHealth, UpdateHealthMetrics,
};
use crate::state::CollectorStateData;
use actix::Addr;

/// A handle for interacting with the `CStateActor`.
///
/// This provides a clean, async-friendly API for other components
/// to communicate with the collector state component.
#[derive(Clone)]
pub struct CStateHandle {
    addr: Addr<CStateActor>,
}

impl CStateHandle {
    /// Creates a new `CStateHandle`.
    pub fn new(addr: Addr<CStateActor>) -> Self {
        Self { addr }
    }

    /// Updates the health metrics for the collector.
    /// This is typically called by other components (e.g., `zzpinger`) to report their status.
    pub async fn update_metrics(&self, metrics: UpdateHealthMetrics) -> Result<(), CStateError> {
        self.addr.send(metrics).await.map_err(From::from)
    }

    /// Retrieves the full internal state of the collector.
    /// This is only applicable when the actor is in the `Collector` role.
    pub async fn get_state(&self) -> Result<CollectorStateData, CStateError> {
        self.addr.send(GetCollectorState).await?
    }

    /// Retrieves a health summary of the component.
    pub async fn get_health(&self) -> Result<CStateHealth, CStateError> {
        self.addr.send(GetHealth).await?
    }
}
