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
use zzstorage::actor::StorageActor;

/// A builder for constructing `MemDBActor` instances.
pub struct MemDBBuilder {
    config: MemDBConfig,
    router_actor: Option<Addr<RouterActor>>,
    permissions_map: HashMap<String, MemDBPermissions>,
    storage_actor: Option<Addr<StorageActor>>,
}

impl MemDBBuilder {
    /// Creates a new `MemDBBuilder`.
    pub fn new(config: MemDBConfig) -> Self {
        // Default permissions map for common roles
        // Role strings provided by zznet_api::Role are lowercase (e.g. "collector").
        // Use lowercase keys here so default permissions apply without extra adapters.
        let mut permissions_map = HashMap::new();
        permissions_map.insert("collector".to_string(), MemDBPermissions::collector());
        permissions_map.insert("database".to_string(), MemDBPermissions::database());
        permissions_map.insert("admin".to_string(), MemDBPermissions::admin());

        Self {
            config,
            router_actor: None,
            permissions_map,
            storage_actor: None,
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
    /// If not set, a default map is used with lowercase role keys:
    /// - "collector" -> can submit batches
    /// - "database" -> can receive batches and query
    /// - "admin" -> full access
    pub fn permissions_map(mut self, permissions_map: HashMap<String, MemDBPermissions>) -> Self {
        self.permissions_map = permissions_map;
        self
    }

    /// (Test only) Inject a pre-built storage actor.
    pub fn with_storage_actor(mut self, storage_actor: Addr<StorageActor>) -> Self {
        self.storage_actor = Some(storage_actor);
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
        let mut actor = MemDBActor::new(self.config);
        if let Some(storage_actor) = self.storage_actor {
            actor.set_storage_actor(storage_actor);
        }
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
