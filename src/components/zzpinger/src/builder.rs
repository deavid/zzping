//! Builder pattern for creating PingerActor instances.
//!
//! Provides fluent API for configuring pingers with targets, backends, and MemDB integration.
//! Enables testable construction by allowing backend injection.

use actix::{Actor, Addr};

use zzmem_db::actor::MemDBActor;
use zzmem_db::permissions::MemDBPermission;

use crate::actor::PingerActor;
use crate::api::PingerHandle;
use crate::error::PingerError;
use crate::messages::TargetConfig;
use crate::pinger::PingBackend;
use std::sync::Arc;

/// Builder for PingerActor configuration. Allows step-by-step setup of ping targets, backends, and integration.
/// Ensures proper initialization and validation before actor creation.
pub struct PingerBuilder {
    memdb_addr: Option<Addr<MemDBActor<MemDBPermission>>>,
    memdb_recipient: Option<actix::Recipient<zzmem_db::messages::StorePingResult>>,
    initial_targets: Vec<TargetConfig>,
    enabled: bool,
    backend: Option<Arc<dyn PingBackend>>,
}

impl PingerBuilder {
    /// Creates a new builder with default settings. Starts with no targets and MockBackend.
    pub fn new() -> Self {
        Self {
            memdb_addr: None,
            memdb_recipient: None,
            initial_targets: Vec::new(),
            enabled: true,
            backend: None,
        }
    }

    /// Configures MemDB actor for result submission. Used in production with real MemDB instances.
    pub fn memdb_addr(mut self, addr: Addr<MemDBActor<MemDBPermission>>) -> Self {
        self.memdb_addr = Some(addr);
        self
    }

    /// Configures MemDB recipient for testing. Allows injection of mock recipients without Addr requirement.
    pub fn memdb_recipient(
        mut self,
        recipient: actix::Recipient<zzmem_db::messages::StorePingResult>,
    ) -> Self {
        self.memdb_recipient = Some(recipient);
        self
    }

    /// Injects a PingBackend for testing. Allows MockBackend to avoid real ICMP in tests.
    pub fn backend(mut self, backend: Arc<dyn PingBackend>) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Sets initial ping targets. Defines which hosts to monitor and their timing parameters.
    pub fn targets(mut self, targets: Vec<TargetConfig>) -> Self {
        self.initial_targets = targets;
        self
    }

    /// Sets whether pinging starts immediately. Controls initial actor behavior.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Builds and starts the PingerActor. Returns handle for interaction. Validates configuration before startup.
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
        Ok(PingerHandle::new(addr))
    }
}

impl Default for PingerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
