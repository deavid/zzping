//! Builder pattern for creating PingerActor instances.
//!
//! Provides fluent API for configuring pingers with targets, backends, and MemDB integration.
//! Enables testable construction by allowing backend injection.

use crate::actor::PingerActor;
use crate::api::PingerHandle;
use crate::error::PingerError;
use crate::messages::TargetConfig;
use crate::pinger::PingBackend;
use actix::{Actor, Addr};
use std::sync::Arc;
use zzmem_db::actor::MemDBActor;
use zznet_router::RouterActor;

/// A builder for configuring and starting a `PingerActor`.
///
/// This builder provides a fluent interface for setting up the pinger's initial state,
/// including its targets, backend implementation, and integration with a results collector.
/// It ensures that the actor is always created in a valid and consistent state.
pub struct PingerBuilder {
    memdb_addr: Option<Addr<MemDBActor>>,
    memdb_recipient: Option<actix::Recipient<zzmem_db::messages::StorePingResult>>,
    initial_targets: Vec<TargetConfig>,
    enabled: bool,
    backend: Option<Arc<dyn PingBackend>>,
    router_actor: Option<Addr<RouterActor>>,
}

impl PingerBuilder {
    /// Creates a new `PingerBuilder` with default settings.
    ///
    /// By default, the pinger starts with no targets and uses a `MockBackend`, making it safe
    /// for testing environments out-of-the-box.
    pub fn new() -> Self {
        Self {
            memdb_addr: None,
            memdb_recipient: None,
            initial_targets: Vec::new(),
            enabled: true,
            backend: None,
            router_actor: None,
        }
    }

    /// Configures the address of a `MemDBActor` for result submission.
    ///
    /// This is the standard method for integrating with `zzmem-db` in a production environment.
    pub fn memdb_addr(mut self, addr: Addr<MemDBActor>) -> Self {
        self.memdb_addr = Some(addr);
        self
    }

    /// Configures a `Recipient` for result submission, intended for testing.
    ///
    /// This allows tests to provide a mock actor to receive `StorePingResult` messages,
    /// enabling verification of the result submission logic without a real `MemDBActor`.
    pub fn memdb_recipient(
        mut self,
        recipient: actix::Recipient<zzmem_db::messages::StorePingResult>,
    ) -> Self {
        self.memdb_recipient = Some(recipient);
        self
    }

    /// Injects a custom `PingBackend`.
    ///
    /// This is a key method for testing, allowing the injection of a `MockBackend` to prevent
    /// real network operations and ensure deterministic test outcomes.
    pub fn backend(mut self, backend: Arc<dyn PingBackend>) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Sets the initial list of targets for the pinger to monitor.
    ///
    /// Each `TargetConfig` defines a host to be pinged, along with its specific rate and timeout.
    pub fn targets(mut self, targets: Vec<TargetConfig>) -> Self {
        self.initial_targets = targets;
        self
    }

    /// Sets the initial enabled state of the pinger upon startup.
    ///
    /// If `true`, the pinger will start its monitoring tasks immediately. If `false`, it will
    /// remain idle until explicitly enabled.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Configures the RouterActor for network communication.
    ///
    /// This enables the pinger to receive configuration updates over the network.
    pub fn router_actor(mut self, router_actor: Addr<RouterActor>) -> Self {
        self.router_actor = Some(router_actor);
        self
    }

    /// Consumes the builder to construct, start, and return a handle to the `PingerActor`.
    ///
    /// This method finalizes the configuration, starts the actor, and provides a `PingerHandle`
    /// for interacting with the running actor instance.
    pub fn start(self) -> Result<PingerHandle, PingerError> {
        let mut actor = PingerActor::new();

        if let Some(backend) = self.backend {
            actor = actor.with_targets_with_backend(self.initial_targets, backend);
        } else {
            actor = actor.with_targets(self.initial_targets);
        }

        if let Some(memdb_addr) = self.memdb_addr {
            actor = actor.with_memdb_addr(memdb_addr);
        }
        if let Some(recipient) = self.memdb_recipient {
            actor = actor.with_memdb_recipient(recipient);
        }

        actor = actor.with_enabled(self.enabled);

        let addr = Actor::start(actor);

        // Create NetworkManager if we have RouterActor
        if let Some(router_actor) = self.router_actor {
            log::info!("Creating PingerNetworkManager for Router integration");

            let network_manager =
                crate::network_manager::PingerNetworkManager::new(addr.clone(), router_actor);

            network_manager.start();

            log::info!("✓ PingerNetworkManager created and registered with Router");
        } else {
            log::debug!("No RouterActor provided - NetworkManager not created");
        }

        Ok(PingerHandle::new(addr))
    }
}

impl Default for PingerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
