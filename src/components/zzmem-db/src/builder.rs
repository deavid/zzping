//! Provides a builder for constructing `MemDBActor` instances.
//!
//! This builder creates the three-actor system:
//! 1. MemDBActor (MainActor - business logic)
//! 2. MemDBNetworkManager (Manager - peer lifecycle and routing)
//! 3. MemDBNetworkActor (per-peer, created by Manager)

use crate::{actor::MemDBActor, config::MemDBConfig, network_messages::MemDBMessage};
use actix::prelude::*;
use zznet_peer_manager::PeerManagerActor;
use zznet_room::room::TypedSender;
use zznet_router::RouterActor;

/// A builder for constructing `MemDBActor` instances.
///
/// Phase 7.4: Creates and wires all three actors together with PeerManager and Router.
pub struct MemDBBuilder {
    config: MemDBConfig,
    peer_manager: Option<Addr<PeerManagerActor>>,
    router_actor: Option<Addr<RouterActor>>,
    typed_sender: Option<TypedSender<MemDBMessage>>,
}

impl MemDBBuilder {
    /// Creates a new `MemDBBuilder`.
    pub fn new(config: MemDBConfig) -> Self {
        Self {
            config,
            peer_manager: None,
            router_actor: None,
            typed_sender: None,
        }
    }

    /// Set the PeerManager for network communication.
    ///
    /// This is required for the three-actor pattern. If not set, the actor
    /// will run in standalone mode without network capabilities.
    pub fn peer_manager(mut self, peer_manager: Addr<PeerManagerActor>) -> Self {
        self.peer_manager = Some(peer_manager);
        self
    }

    /// Set the RouterActor for message routing.
    ///
    /// This is required for the three-actor pattern. If not set, the actor
    /// will run in standalone mode without network capabilities.
    pub fn router(mut self, router_actor: Addr<RouterActor>) -> Self {
        self.router_actor = Some(router_actor);
        self
    }

    /// Set the TypedSender for sending network messages.
    ///
    /// This should be obtained from `room.typed_sender()` where room is a
    /// `Room<MemDBMessage>`. Required for network communication.
    pub fn typed_sender(mut self, typed_sender: TypedSender<MemDBMessage>) -> Self {
        self.typed_sender = Some(typed_sender);
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

        // Phase 7.4: Create NetworkManager if we have both PeerManager and RouterActor
        if let (Some(peer_manager), Some(router_actor)) = (self.peer_manager, self.router_actor) {
            tracing::info!("Creating MemDBNetworkManager for three-actor pattern");

            let mut network_manager = crate::network_manager::MemDBNetworkManager::new(
                actor_addr.clone(),
                peer_manager,
                router_actor,
            );

            // Set TypedSender if provided
            if let Some(typed_sender) = self.typed_sender {
                network_manager = network_manager.with_typed_sender(typed_sender);
            } else {
                tracing::warn!(
                    "NetworkManager created without TypedSender - network communication disabled"
                );
            }

            let network_manager = network_manager.start();

            // Wire NetworkManager back to MainActor
            actor_addr.do_send(crate::internal_messages::SetNetworkManager {
                network_manager: network_manager.clone(),
            });

            tracing::info!("✓ Three-actor system initialized (MainActor + NetworkManager)");
        } else {
            tracing::debug!("No PeerManager - NetworkManager not created (standalone mode)");
        }

        actor_addr
    }
}
