//! Provides a builder for constructing `CStateActor` instances.
//!
//! This builder creates the three-actor system:
//! 1. CStateActor (MainActor - business logic)
//! 2. CStateNetworkManager (Manager - peer lifecycle and routing)
//! 3. CStateNetworkActor (per-peer, created by Manager)

use crate::{actor::CStateActor, config::CStateConfig, permissions::CStatePermissions};
use actix::prelude::*;
use std::collections::HashMap;
use zznet_router::RouterActor;

/// A builder for constructing `CStateActor` instances.
///
/// Phase 7.3: Creates and wires all three actors together with Router.
pub struct CStateBuilder {
    config: CStateConfig,
    router_actor: Option<Addr<RouterActor>>,
    permissions_map: Option<HashMap<String, CStatePermissions>>,
}

impl CStateBuilder {
    /// Creates a new `CStateBuilder`.
    pub fn new(config: CStateConfig) -> Self {
        Self {
            config,
            router_actor: None,
            permissions_map: None,
        }
    }

    /// Set the RouterActor for network communication.
    ///
    /// This is required for the three-actor pattern. If not set, the actor
    /// will run in standalone mode without network capabilities.
    pub fn router(mut self, router_actor: Addr<RouterActor>) -> Self {
        self.router_actor = Some(router_actor);
        self
    }

    /// Set the permissions map for role-to-permissions translation.
    ///
    /// This maps role strings to the specific permissions that peers with
    /// those roles should have within this component. If not set, the
    /// NetworkManager will deny all peer connections.
    pub fn permissions_map(mut self, permissions_map: HashMap<String, CStatePermissions>) -> Self {
        self.permissions_map = Some(permissions_map);
        self
    }

    /// Get the permissions map (for testing)
    pub fn get_permissions_map(&self) -> Option<&HashMap<String, CStatePermissions>> {
        self.permissions_map.as_ref()
    }

    /// Builds and starts the `CStateActor` along with its NetworkManager.
    ///
    /// Phase 7.3: This creates the complete three-actor system:
    /// - CStateActor (business logic, zero network dependencies)
    /// - CStateNetworkManager (orchestrates peer lifecycle)
    /// - CStateNetworkActor instances (created per peer by NetworkManager)
    ///
    /// Returns the address of the MainActor.
    pub fn build(self) -> Addr<CStateActor> {
        // Create and start the MainActor first
        let config = self.config.clone();
        let actor_addr = CStateActor::create(move |_ctx| CStateActor::new(config));

        // Phase 7.3: Create NetworkManager if we have RouterActor
        if let Some(router_actor) = self.router_actor {
            log::info!("Creating CStateNetworkManager for three-actor pattern");

            let permissions_map = self.permissions_map.unwrap_or_else(|| {
                log::warn!("No permissions_map provided - all peer connections will be denied");
                HashMap::new()
            });

            let network_manager = crate::network_manager::CStateNetworkManager::new(
                actor_addr.clone(),
                router_actor,
                permissions_map,
            );

            let network_manager = network_manager.start();

            // Wire NetworkManager back to MainActor
            actor_addr.do_send(crate::internal_messages::SetNetworkManager {
                network_manager: network_manager.clone(),
            });

            log::info!("✓ Three-actor system initialized (MainActor + NetworkManager)");
        } else {
            log::debug!("No Router - NetworkManager not created (standalone mode)");
        }

        actor_addr
    }
}
