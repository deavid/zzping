//! Provides a builder for constructing `MemDBActor` instances.
//!
//! This builder creates the three-actor system:
//! 1. MemDBActor (MainActor - business logic)
//! 2. MemDBNetworkManager (Manager - peer lifecycle and routing)
//! 3. MemDBNetworkActor (per-peer, created by Manager)

use crate::{actor::MemDBActor, config::MemDBConfig};
use actix::prelude::*;
use zznet_router::RouterActor;

/// A builder for constructing `MemDBActor` instances.
///
/// Phase 7.4: Creates and wires all three actors together with Router.
pub struct MemDBBuilder {
    config: MemDBConfig,
    router_actor: Option<Addr<RouterActor>>,
}

impl MemDBBuilder {
    /// Creates a new `MemDBBuilder`.
    pub fn new(config: MemDBConfig) -> Self {
        Self {
            config,
            router_actor: None,
        }
    }

    /// Set the RouterActor for message routing.
    ///
    /// This is required for the three-actor pattern. If not set, the actor
    /// will run in standalone mode without network capabilities.
    pub fn router(mut self, router_actor: Addr<RouterActor>) -> Self {
        self.router_actor = Some(router_actor);
        self
    }

    /// Builds and starts the `MemDBActor` along with its NetworkManager.
    ///
    /// Phase 7.4: This creates the complete three-actor system:
    /// - MemDBActor (business logic, zero network dependencies)
    /// - MemDBNetworkManager (orchestrates peer lifecycle)
    /// - MemDBNetworkActor instances (created per peer by NetworkManager)
    ///
    /// Returns the address of the MainActor.
    pub fn build(self) -> Addr<MemDBActor> {
        // Create and start the MainActor first
        let actor_addr = MemDBActor::create(move |_ctx| MemDBActor::new(self.config));

        // Phase 7.4: Create NetworkManager if we have RouterActor
        if let Some(router_actor) = self.router_actor {
            tracing::info!("Creating MemDBNetworkManager for three-actor pattern");

            let network_manager =
                crate::network_manager::MemDBNetworkManager::new(actor_addr.clone(), router_actor);

            let network_manager = network_manager.start();

            // Wire NetworkManager back to MainActor
            actor_addr.do_send(crate::internal_messages::SetNetworkManager {
                network_manager: network_manager.clone(),
            });

            tracing::info!("✓ Three-actor system initialized (MainActor + NetworkManager)");
        } else {
            tracing::debug!("No Router - NetworkManager not created (standalone mode)");
        }

        actor_addr
    }
}
