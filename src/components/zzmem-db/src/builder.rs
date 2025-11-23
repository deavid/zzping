//! Provides a builder for constructing `MemDBActor` instances.
//!
//! This builder creates the three-actor system:
//! 1. MemDBActor (MainActor - business logic)
//! 2. GenericNetworkManager (Manager - peer lifecycle and routing)
//! 3. MemDBNetworkActor (per-peer, created by Manager)

use crate::{actor::MemDBActor, config::MemDBConfig, permissions::MemDBPermissions};
use actix::prelude::*;
use std::collections::HashMap;
use zznet_router::RouterActor;

/// A builder for constructing `MemDBActor` instances.
pub struct MemDBBuilder {
    config: MemDBConfig,
    router_actor: Option<Addr<RouterActor>>,
    permissions_map: HashMap<String, MemDBPermissions>,
}

impl MemDBBuilder {
    /// Creates a new `MemDBBuilder`.
    pub fn new(config: MemDBConfig) -> Self {
        // Default permissions map for common roles
        let mut permissions_map = HashMap::new();
        permissions_map.insert("Collector".to_string(), MemDBPermissions::collector());
        permissions_map.insert("Database".to_string(), MemDBPermissions::database());
        permissions_map.insert("Admin".to_string(), MemDBPermissions::admin());

        Self {
            config,
            router_actor: None,
            permissions_map,
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

    /// Set custom permissions map for role-to-permissions translation.
    ///
    /// If not set, a default map is used with:
    /// - "Collector" -> can submit batches
    /// - "Database" -> can receive batches and query
    /// - "Admin" -> full access
    pub fn permissions_map(mut self, permissions_map: HashMap<String, MemDBPermissions>) -> Self {
        self.permissions_map = permissions_map;
        self
    }

    /// Builds and starts the `MemDBActor` along with its NetworkManager.
    ///
    /// This creates the complete three-actor system:
    /// - MemDBActor (business logic, zero network dependencies)
    /// - GenericNetworkManager (orchestrates peer lifecycle)
    /// - MemDBNetworkActor instances (created per peer by NetworkManager)
    ///
    /// Returns the address of the MainActor.
    pub fn build(self) -> Addr<MemDBActor> {
        // Instantiate the MainActor so we can clone its event bus before starting it
        let actor = MemDBActor::new(self.config);
        let event_bus = actor.event_bus();
        let actor_addr = actor.start();

        // Create NetworkManager if we have RouterActor
        if let Some(router_actor) = self.router_actor {
            tracing::info!("Creating GenericNetworkManager for three-actor pattern");

            let _network_manager =
                zznet_component::GenericNetworkManager::<crate::spec::MemDBSpec>::new(
                    actor_addr.clone(),
                    router_actor,
                    event_bus,
                    self.permissions_map,
                )
                .start();

            tracing::info!("✓ Three-actor system initialized (MainActor + GenericNetworkManager)");
        } else {
            tracing::debug!("No Router - NetworkManager not created (standalone mode)");
        }

        actor_addr
    }
}
