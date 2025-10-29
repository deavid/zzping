//! Provides a builder for constructing `CStateActor` instances.
//!
//! This builder creates the three-actor system:
//! 1. CStateActor (MainActor - business logic)
//! 2. CStateNetworkManager (Manager - peer lifecycle and routing)
//! 3. CStateNetworkActor (per-peer, created by Manager)

use crate::{actor::CStateActor, config::CStateConfig, network_messages::CStateMessage};
use actix::prelude::*;
use std::sync::Arc;
use zznet_api::MessageRouter;
use zznet_api::PeerRegistry;
use zznet_room::room::TypedSender;

/// A builder for constructing `CStateActor` instances.
///
/// Phase 7.3: Creates and wires all three actors together with PeerManagerActor.
pub struct CStateBuilder {
    config: CStateConfig,
    peer_registry: Option<Arc<dyn PeerRegistry>>,
    message_router: Option<Arc<dyn MessageRouter>>,
    typed_sender: Option<TypedSender<CStateMessage>>,
}

impl CStateBuilder {
    /// Creates a new `CStateBuilder`.
    pub fn new(config: CStateConfig) -> Self {
        Self {
            config,
            peer_registry: None,
            message_router: None,
            typed_sender: None,
        }
    }

    /// Set the PeerRegistry and MessageRouter for network communication.
    ///
    /// This is required for the three-actor pattern. If not set, the actor
    /// will run in standalone mode without network capabilities.
    pub fn peer_manager(mut self, peer_registry: Arc<dyn PeerRegistry>) -> Self {
        self.peer_registry = Some(peer_registry);
        self
    }

    /// Set the MessageRouter for network communication.
    ///
    /// This is required for the three-actor pattern. If not set, the actor
    /// will run in standalone mode without network capabilities.
    pub fn router(mut self, message_router: Arc<dyn MessageRouter>) -> Self {
        self.message_router = Some(message_router);
        self
    }

    /// Set the TypedSender for broadcasting network messages.
    ///
    /// This should be obtained from `room.typed_sender()` where room is a
    /// `Room<CStateMessage>`. Required for network message broadcasting.
    pub fn typed_sender(mut self, typed_sender: TypedSender<CStateMessage>) -> Self {
        self.typed_sender = Some(typed_sender);
        self
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

        // Phase 7.3: Create NetworkManager if we have PeerRegistry and MessageRouter
        if let (Some(peer_registry), Some(message_router)) =
            (self.peer_registry, self.message_router)
        {
            log::info!("Creating CStateNetworkManager for three-actor pattern");

            let mut network_manager = crate::network_manager::CStateNetworkManager::new(
                actor_addr.clone(),
                peer_registry,
                message_router,
            );

            // Set TypedSender if provided
            if let Some(typed_sender) = self.typed_sender {
                network_manager = network_manager.with_typed_sender(typed_sender);
            } else {
                log::warn!(
                    "NetworkManager created without TypedSender - network communication disabled"
                );
            }

            let network_manager = network_manager.start();

            // Wire NetworkManager back to MainActor
            actor_addr.do_send(crate::internal_messages::SetNetworkManager {
                network_manager: network_manager.clone(),
            });

            log::info!("✓ Three-actor system initialized (MainActor + NetworkManager)");
        } else {
            log::debug!(
                "No PeerRegistry/MessageRouter - NetworkManager not created (standalone mode)"
            );
        }

        actor_addr
    }
}
