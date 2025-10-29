//! Network Manager for the CState component.
//!
//! This actor orchestrates peer lifecycle and manages per-peer NetworkActors.
//! It acts as a bridge between the business logic (CStateActor) and the network
//! layer (CStateNetworkActor instances).

use crate::{
    internal_messages::{
        BroadcastHeartbeat, SendCollectorList, SendHeartbeatAck, SendRegistrationRejected,
        SendToNetwork, SendUnauthorized,
    },
    network_messages::CStateMessage,
};
use actix::prelude::*;
use log::{debug, info, warn};
use std::collections::HashMap;
use zznet_api::types::PeerId;
use zznet_peer_manager::PeerManagerActor;
use zznet_room::room::TypedSender;
use zznet_router::RouterActor;

// Phase 6.2: Placeholder messages for backward compatibility during migration
// TODO Phase 7.3: Remove after full PeerLifecycleEvent integration
/// Placeholder for PeerAdded event - will be replaced with actual PeerLifecycleEvent
#[derive(Message)]
#[rtype(result = "()")]
pub struct PeerAdded {
    /// The peer ID that was added.
    pub peer_id: PeerId,
}

/// Placeholder for PeerRemoved event - will be replaced with actual PeerLifecycleEvent
#[derive(Message)]
#[rtype(result = "()")]
pub struct PeerRemoved {
    /// The peer ID that was removed.
    pub peer_id: PeerId,
}

/// The NetworkManager orchestrates peer lifecycle and manages NetworkActors.
///
/// Responsibilities:
/// - Subscribe to PeerLifecycleEvents from PeerManager
/// - Spawn/stop CStateNetworkActor per connected peer
/// - Register Room<CStateMessage> with PeerManager
/// - Route outbound messages to appropriate NetworkActors
/// - Handle peer disconnection cleanup
pub struct CStateNetworkManager {
    /// Link to the MainActor for business logic.
    main_actor: Addr<crate::actor::CStateActor>,

    /// Phase 7.3: Direct PeerManagerActor access for authorization and channels
    peer_manager: Addr<PeerManagerActor>,

    /// RouterActor for sending messages to peers
    router_actor: Addr<RouterActor>,

    /// Per-peer NetworkActor instances.
    network_actors: HashMap<PeerId, Addr<crate::network_actor::CStateNetworkActor>>,

    /// The TypedSender for broadcasting CState network messages.
    /// This is cloneable and can be shared with NetworkActors.
    typed_sender: Option<TypedSender<CStateMessage>>,
}

impl CStateNetworkManager {
    /// Creates a new CStateNetworkManager.
    ///
    /// # Arguments
    /// * `main_actor` - Address of the CStateActor (business logic)
    /// * `peer_manager` - PeerManagerActor for peer state queries
    /// * `router_actor` - RouterActor for sending messages to peers
    pub fn new(
        main_actor: Addr<crate::actor::CStateActor>,
        peer_manager: Addr<PeerManagerActor>,
        router_actor: Addr<RouterActor>,
    ) -> Self {
        Self {
            main_actor,
            peer_manager,
            router_actor,
            network_actors: HashMap::new(),
            typed_sender: None,
        }
    }

    /// Sets the TypedSender for network communication.
    ///
    /// The TypedSender is obtained from `room.typed_sender()` and can be cloned
    /// to share with NetworkActors for sending messages.
    pub fn with_typed_sender(mut self, typed_sender: TypedSender<CStateMessage>) -> Self {
        self.typed_sender = Some(typed_sender);
        self
    }
}

impl Actor for CStateNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("CStateNetworkManager started");

        // Note: PeerLifecycleEvent subscription deferred until PeerManager is standalone.
        // Currently using SessionManager bridge pattern (see IntentConfig for reference).

        // Note: Room<T> registration will be added when Room<T> integration is complete.
        // TypedSender is provided via with_typed_sender() during initialization.
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("CStateNetworkManager stopped");
        // TypedSender cleanup is automatic (drops when NetworkManager drops)
    }
}

// ============================================================================
// Peer Lifecycle Event Handlers
// ============================================================================

impl Handler<PeerAdded> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerAdded, ctx: &mut Context<Self>) -> Self::Result {
        debug!("Peer added: {:?}", msg.peer_id);

        // Check peer role using PeerManagerActor
        let rt = tokio::runtime::Handle::current();
        let role_future = self.peer_manager.send(zznet_peer_manager::GetPeerRole {
            peer_id: msg.peer_id.clone(),
        });
        if let Some(role) = rt.block_on(role_future).unwrap_or(None) {
            debug!(
                "Peer {:?} registered with role '{}'",
                msg.peer_id,
                role.as_str()
            );
        } else {
            debug!(
                "Peer {:?} registered without role (likely pending HELLO completion)",
                msg.peer_id
            );
        }

        // TODO: Update to use RouterActor API
        // if self.router_actor.peer_sender(&msg.peer_id).is_none() {
        //     warn!(
        //         "Router has no sender for peer {:?} yet; registration wiring still pending",
        //         msg.peer_id
        //     );
        // }

        // Note: Room membership checks will be added when Room<T> is fully integrated.
        // For now, we spawn a NetworkActor for every peer connection.

        // Spawn NetworkActor for this peer
        if let Some(typed_sender) = &self.typed_sender {
            let network_actor = crate::network_actor::CStateNetworkActor::new(
                msg.peer_id.clone(),
                typed_sender.clone(),
                self.main_actor.clone(),
                ctx.address(),
            )
            .start();

            self.network_actors
                .insert(msg.peer_id.clone(), network_actor);
            debug!(
                "Spawned NetworkActor for peer: {:?}, total actors: {}",
                msg.peer_id,
                self.network_actors.len()
            );
        } else {
            warn!("Cannot spawn NetworkActor: TypedSender not initialized");
        }
    }
}

impl Handler<PeerRemoved> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerRemoved, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Peer removed: {:?}", msg.peer_id);

        // Stop and remove NetworkActor for this peer
        if let Some(_actor) = self.network_actors.remove(&msg.peer_id) {
            // Actor will stop automatically when dropped
            info!(
                "NetworkActor removed for peer: {:?}, remaining actors: {}",
                msg.peer_id,
                self.network_actors.len()
            );
        } else {
            debug!("No NetworkActor found for removed peer: {:?}", msg.peer_id);
        }
    }
}

// ============================================================================
// Outbound Message Handlers (MainActor → NetworkManager → NetworkActor)
// ============================================================================

impl Handler<SendHeartbeatAck> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendHeartbeatAck, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Forwarding HeartbeatAck to peer: {:?}", msg.peer_id);

        // Forward to specific NetworkActor
        if let Some(actor) = self.network_actors.get(&msg.peer_id) {
            let network_msg = CStateMessage::HeartbeatAck {
                timestamp_ms: msg.timestamp_ms,
                server_time_ms: msg.server_time_ms,
            };
            actor.do_send(SendToNetwork {
                message: network_msg,
            });
        } else {
            warn!("No NetworkActor found for peer: {:?}", msg.peer_id);
        }
    }
}

impl Handler<SendCollectorList> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendCollectorList, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Forwarding CollectorList to peer: {:?}", msg.peer_id);

        // Forward to specific NetworkActor
        if let Some(actor) = self.network_actors.get(&msg.peer_id) {
            let network_msg = CStateMessage::CollectorList {
                collectors: msg.collectors,
            };
            actor.do_send(SendToNetwork {
                message: network_msg,
            });
        } else {
            warn!("No NetworkActor found for peer: {:?}", msg.peer_id);
        }
    }
}

impl Handler<SendRegistrationRejected> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendRegistrationRejected, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Forwarding RegistrationRejected to peer: {:?}", msg.peer_id);

        // Forward to specific NetworkActor
        if let Some(actor) = self.network_actors.get(&msg.peer_id) {
            let network_msg = CStateMessage::RegistrationRejected { reason: msg.reason };
            actor.do_send(SendToNetwork {
                message: network_msg,
            });
        } else {
            warn!("No NetworkActor found for peer: {:?}", msg.peer_id);
        }
    }
}

impl Handler<SendUnauthorized> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendUnauthorized, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Forwarding Unauthorized to peer: {:?}", msg.peer_id);

        // Forward to specific NetworkActor
        if let Some(actor) = self.network_actors.get(&msg.peer_id) {
            let network_msg = CStateMessage::Unauthorized { reason: msg.reason };
            actor.do_send(SendToNetwork {
                message: network_msg,
            });
        } else {
            warn!("No NetworkActor found for peer: {:?}", msg.peer_id);
        }
    }
}

impl Handler<BroadcastHeartbeat> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: BroadcastHeartbeat, _ctx: &mut Context<Self>) -> Self::Result {
        debug!(
            "Broadcasting heartbeat from collector {} to {} peers",
            msg.collector_id,
            self.network_actors.len()
        );

        // Convert to network message
        let network_msg = CStateMessage::Heartbeat {
            collector_id: msg.collector_id,
            uptime_secs: msg.uptime_secs,
            pings_sent: msg.pings_sent,
            pings_received: msg.pings_received,
            batches_sent: msg.batches_sent,
            last_config_update_ms: msg.last_config_update_ms,
            connection_nonce: msg.connection_nonce,
        };

        // Broadcast to all NetworkActors
        for (peer_id, actor) in &self.network_actors {
            debug!("Sending heartbeat to peer: {:?}", peer_id);
            actor.do_send(SendToNetwork {
                message: network_msg.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    // Phase 4.4: Tests will be added when implementing NetworkManager functionality.
    // These will cover:
    // - PeerAdded/PeerRemoved handling
    // - NetworkActor spawning and cleanup
    // - Message routing (unicast and broadcast)
    // - SessionManager bridge pattern
}
