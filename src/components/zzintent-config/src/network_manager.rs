//! IntentConfig Network Manager Actor
//!
//! The Manager Actor in the three-actor pattern. Responsibilities:
//! - Subscribe to PeerLifecycleEvent bus from zznet-peer-manager
//! - Spawn IntentConfigNetworkActor when peer joins with "intent-config" room
//! - Destroy IntentConfigNetworkActor when peer disconnects
//! - Handle broadcast requests from MainActor (BroadcastConfigUpdate)
//! - Forward inbound requests to MainActor (after authorization check)
//! - Query PeerManager for role/permission checks

use crate::internal_messages::{
    BroadcastConfigUpdate, InboundConfigChangeRequest, InboundGetConfigRequest,
    NetworkConfigChangeRequest, SendConfigUpdateToPeer, SendErrorMessageToPeer, SendErrorToPeer,
};
use crate::messages::{GetCurrentConfig, IntentConfigData};
use crate::network_actor::IntentConfigNetworkActor;
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::broadcast;

// Phase 7.2: Use traits for interface segregation (control-plane vs data-plane)
use zznet_api::types::{PeerId, PeerLifecycleEvent};
use zznet_peer_manager::PeerManagerActor;
use zznet_router::RouterActor;

/// IntentConfigNetworkManager - Orchestrates per-peer network actors
///
/// This actor is the bridge between the business logic (IntentConfigActor)
/// and the network (per-peer IntentConfigNetworkActors). It subscribes to
/// peer lifecycle events and manages the fleet of network actors.
///
/// # Responsibilities
/// - Lifecycle: Spawn/destroy NetworkActors as peers connect/disconnect
/// - Broadcast: Fan-out config updates to all connected peers
/// - Authorization: Query PeerManager for role/permission checks before forwarding requests
/// - Coordination: Aggregate responses from multiple peers when needed
///
/// # Message Flow
/// See `internal_messages.rs` for detailed message flow diagrams.
pub struct IntentConfigNetworkManager {
    /// Address of the main business logic actor
    main_actor: Addr<crate::actor::IntentConfigActor>,

    /// Per-peer network actors (one per connected peer with "intent-config" room)
    network_actors: HashMap<PeerId, Addr<IntentConfigNetworkActor>>,

    /// Receiver for peer lifecycle events
    _event_rx: broadcast::Receiver<PeerLifecycleEvent>,

    /// PeerManagerActor for control-plane queries
    peer_manager: Addr<PeerManagerActor>,

    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,
}

impl IntentConfigNetworkManager {
    /// Create a new NetworkManager tied to the provided IntentConfigActor.
    pub fn new(
        main_actor: Addr<crate::actor::IntentConfigActor>,
        event_rx: broadcast::Receiver<PeerLifecycleEvent>,
        peer_manager: Addr<PeerManagerActor>,
        router_actor: Addr<RouterActor>,
    ) -> Self {
        Self {
            main_actor,
            network_actors: HashMap::new(),
            _event_rx: event_rx,
            peer_manager,
            router_actor,
        }
    }

    /// Spawn a new NetworkActor for a peer (synchronous version using RouterActor)
    ///
    /// Called when PeerAdded/PeerConnected event indicates the peer
    /// has joined the "intent-config" room.
    fn spawn_network_actor_sync(
        peer_manager: &Addr<PeerManagerActor>,
        router_actor: &Addr<RouterActor>,
        peer_id: PeerId,
        manager_addr: Addr<IntentConfigNetworkManager>,
        network_actors: &mut HashMap<PeerId, Addr<IntentConfigNetworkActor>>,
    ) {
        log::info!("Spawning IntentConfigNetworkActor for peer: {}", peer_id);

        // Check if peer is connected using PeerManagerActor
        let rt = tokio::runtime::Handle::current();
        let is_connected_future = peer_manager.send(zznet_peer_manager::IsPeerConnected {
            peer_id: peer_id.clone(),
        });
        let is_connected = rt.block_on(is_connected_future).unwrap_or(false);

        if !is_connected {
            log::warn!(
                "Peer {} not yet reported as connected in PeerManager when spawning NetworkActor",
                peer_id
            );
        }

        // Get peer role using PeerManagerActor
        let role_future = peer_manager.send(zznet_peer_manager::GetPeerRole {
            peer_id: peer_id.clone(),
        });
        if let Some(role) = rt.block_on(role_future).unwrap_or(None) {
            log::debug!(
                "Peer {} authorized with role '{}' before wiring network actor",
                peer_id,
                role.as_str()
            );
        }

        // For migration: block on async RouterActor calls
        // TODO: Make this fully async
        let rt = tokio::runtime::Handle::current();
        let peer_sender_future = router_actor.send(zznet_router::PeerSender {
            peer_id: peer_id.clone(),
        });
        let peer_receiver_future = router_actor.send(zznet_router::SubscribePeerInbound {
            peer_id: peer_id.clone(),
        });

        let peer_sender = match rt.block_on(peer_sender_future) {
            Ok(Some(sender)) => sender,
            _ => {
                log::error!("Failed to get peer sender for {}: peer not found", peer_id);
                return;
            }
        };

        let peer_receiver = match rt.block_on(peer_receiver_future) {
            Ok(Some(receiver)) => receiver,
            _ => {
                log::error!(
                    "Failed to subscribe to peer inbound for {}: peer not found",
                    peer_id
                );
                return;
            }
        };

        // Step 3: Create NetworkActor with real channels
        let network_actor = IntentConfigNetworkActor::new(
            peer_id.clone(),
            manager_addr,
            peer_sender,
            peer_receiver,
        )
        .start();

        network_actors.insert(peer_id.clone(), network_actor);
        log::info!(
            "NetworkActor spawned for peer {} - {} active actors",
            peer_id,
            network_actors.len()
        );
    }

    /// Destroy NetworkActor for a peer when lifecycle events indicate removal.
    fn destroy_network_actor(&mut self, peer_id: &PeerId) {
        if let Some(_actor) = self.network_actors.remove(peer_id) {
            log::info!("Destroying IntentConfigNetworkActor for peer: {}", peer_id);
            log::debug!(
                "NetworkActor destroyed - {} active actors remain",
                self.network_actors.len()
            );
        } else {
            log::warn!(
                "Attempted to destroy non-existent NetworkActor for peer: {}",
                peer_id
            );
        }
    }
}

// ============================================================================
// Actor Implementation
// ============================================================================

impl Actor for IntentConfigNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::info!("IntentConfigNetworkManager started");

        // Note: PeerLifecycleEvent subscription deferred until PeerManager is standalone
        // Events currently handled manually via Handler<PeerLifecycleEventWrapper>

        // Keep actor alive
        ctx.set_mailbox_capacity(1000);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::info!("IntentConfigNetworkManager stopped");

        // Clean up all network actors
        let actor_count = self.network_actors.len();
        self.network_actors.clear();
        log::debug!("Stopped {} NetworkActors", actor_count);
    }
}

// ============================================================================
// Handler: PeerLifecycleEvent (wrapped for actix compatibility)
// ============================================================================

/// Wrapper to make PeerLifecycleEvent work with actix Handler
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct PeerLifecycleEventWrapper(pub PeerLifecycleEvent);

impl Handler<PeerLifecycleEventWrapper> for IntentConfigNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerLifecycleEventWrapper, ctx: &mut Self::Context) -> Self::Result {
        match msg.0 {
            PeerLifecycleEvent::PeerAdded { peer_id } => {
                log::debug!("PeerLifecycleEvent::PeerAdded: {}", peer_id);
                // Create the actor synchronously using RouterActor
                let manager_addr = ctx.address();

                Self::spawn_network_actor_sync(
                    &self.peer_manager,
                    &self.router_actor,
                    peer_id,
                    manager_addr,
                    &mut self.network_actors,
                );
            }
            PeerLifecycleEvent::PeerConnected { peer_id } => {
                log::debug!("PeerLifecycleEvent::PeerConnected: {}", peer_id);
                // Network actor already created on PeerAdded
            }
            PeerLifecycleEvent::PeerDisconnected { peer_id } => {
                log::debug!("PeerLifecycleEvent::PeerDisconnected: {}", peer_id);
                self.destroy_network_actor(&peer_id);
            }
            PeerLifecycleEvent::PeerRemoved { peer_id } => {
                log::debug!("PeerLifecycleEvent::PeerRemoved: {}", peer_id);
                self.destroy_network_actor(&peer_id);
            }
            PeerLifecycleEvent::PeerIdentityUpdated { peer_id, identity } => {
                log::debug!(
                    "PeerLifecycleEvent::PeerIdentityUpdated: {} - {:?}",
                    peer_id,
                    identity
                );
                // Identity updates don't affect IntentConfig NetworkActors
                // Authorization is checked on each request, not cached
            }
        }
    }
}

// ============================================================================
// Handler: BroadcastConfigUpdate (from MainActor)
// ============================================================================

impl Handler<BroadcastConfigUpdate> for IntentConfigNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: BroadcastConfigUpdate, _ctx: &mut Self::Context) -> Self::Result {
        log::info!(
            "Broadcasting config update to {} peers",
            self.network_actors.len()
        );

        // Fan-out to all network actors
        for (peer_id, actor) in &self.network_actors {
            log::debug!("Sending config update to peer: {}", peer_id);
            actor.do_send(SendConfigUpdateToPeer {
                config: msg.config.clone(),
            });
        }

        log::debug!("Broadcast complete");
    }
}

// ============================================================================
// Handler: SendErrorToPeer (from MainActor)
// ============================================================================

impl Handler<SendErrorToPeer> for IntentConfigNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendErrorToPeer, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(actor) = self.network_actors.get(&msg.peer_id) {
            log::info!(
                "Sending error to peer {}: {}",
                msg.peer_id,
                msg.error_message
            );
            actor.do_send(SendErrorMessageToPeer {
                error_message: msg.error_message,
            });
        } else {
            log::warn!(
                "Cannot send error to peer {} - no NetworkActor exists",
                msg.peer_id
            );
        }
    }
}

// ============================================================================
// Handler: InboundConfigChangeRequest (from NetworkActor)
// ============================================================================

impl Handler<InboundConfigChangeRequest> for IntentConfigNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: InboundConfigChangeRequest, ctx: &mut Self::Context) -> Self::Result {
        log::info!("Received config change request from peer: {}", msg.peer_id);

        let main_actor = self.main_actor.clone();
        let peer_id = msg.peer_id.clone();
        let targets = msg.targets;
        let ping_rate_pps = msg.ping_rate_pps;
        let manager_addr = ctx.address();

        // Check authorization using PeerManagerActor
        let rt = tokio::runtime::Handle::current();
        let role_future = self.peer_manager.send(zznet_peer_manager::GetPeerRole {
            peer_id: peer_id.clone(),
        });
        let role = rt.block_on(role_future).unwrap_or(None);
        let authorized = match role {
            Some(role) => {
                let is_admin = role.as_str() == "client-admin";
                if !is_admin {
                    log::warn!(
                        "Peer {} has role '{}' - not authorized (requires 'client-admin')",
                        peer_id,
                        role.as_str()
                    );
                }
                is_admin
            }
            None => {
                log::warn!("Peer {} has no role - not authorized", peer_id);
                false
            }
        };

        if !authorized {
            log::warn!(
                "Config change request from peer {} DENIED - insufficient permissions",
                peer_id
            );

            // Send error back to peer
            manager_addr.do_send(SendErrorToPeer {
                peer_id: peer_id.clone(),
                error_message: "Unauthorized: ClientAdmin role required".to_string(),
            });
            return;
        }

        // Forward to MainActor for processing
        log::debug!(
            "Config change request from peer {} AUTHORIZED - forwarding to MainActor",
            peer_id
        );

        let auth_request = NetworkConfigChangeRequest {
            peer_id,
            targets,
            ping_rate_pps,
            authorized: true,
        };

        if let Err(e) = main_actor.try_send(auth_request) {
            log::error!(
                "Failed to forward config change request to main actor: {}",
                e
            );
        }
    }
}

// ============================================================================
// Handler: InboundGetConfigRequest (from NetworkActor)
// ============================================================================

impl Handler<InboundGetConfigRequest> for IntentConfigNetworkManager {
    type Result = ResponseFuture<IntentConfigData>;

    fn handle(&mut self, msg: InboundGetConfigRequest, _ctx: &mut Self::Context) -> Self::Result {
        log::debug!("Received GetConfig request from peer: {}", msg.peer_id);

        let main_actor = self.main_actor.clone();

        Box::pin(async move {
            let result = main_actor.send(GetCurrentConfig).await;

            match result {
                Ok(config) => {
                    log::debug!("Returning config to peer: {}", msg.peer_id);
                    config
                }
                Err(e) => {
                    log::error!(
                        "Failed to get config from MainActor for peer {}: {}",
                        msg.peer_id,
                        e
                    );
                    // Return default config on error
                    IntentConfigData::default()
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    // Phase 3.9 COMPLETE: Integration tests in tests/three_actor_integration_tests.rs
    // These tests cover the happy-path scenarios for the three-actor pattern.
    // Unit tests for NetworkManager are deferred until Room<T> integration is complete.
}
