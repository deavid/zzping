//! Network Manager for the CState component.
//!
//! This actor orchestrates peer lifecycle and manages per-peer NetworkActors.
//! It acts as a bridge between the business logic (CStateActor) and the network
//! layer (CStateNetworkActor instances).

use crate::{
    internal_messages::{
        BroadcastHeartbeat, SendCollectorList, SendHeartbeatAck, SendRegistrationRejected,
        SendUnauthorized,
    },
    network_actor::CStateNetworkActor,
    network_messages::CStateMessage,
    permissions::CStatePermissions,
};
use actix::prelude::*;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use zznet_api::types::{PeerId, RoomId};
use zznet_room::actor::RoomActor;
use zznet_room::room_manager::{CreateError, CreateRoomForPeer, RoomInboundRecipient};
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
/// - Subscribe to PeerLifecycleEvents from Router
/// - Spawn/stop CStateNetworkActor per connected peer
/// - Register Room<CStateMessage> with Router
/// - Route outbound messages to appropriate NetworkActors
/// - Handle peer disconnection cleanup
pub struct CStateNetworkManager {
    /// The router actor for sending messages
    router: Addr<RouterActor>,
    /// The main actor for handling internal messages
    main_actor: Addr<crate::actor::CStateActor>,
    /// Per-peer translator actors for message translation
    translator_actors: Arc<RwLock<HashMap<PeerId, Addr<CStateNetworkActor>>>>,
    /// Per-peer room actors for serialization
    room_actors: Arc<RwLock<HashMap<PeerId, Addr<RoomActor<CStateMessage>>>>>,
    /// Address of this NetworkManager (set in started())
    self_addr: Option<Addr<CStateNetworkManager>>,
    /// Permissions map for role-to-permissions translation
    permissions_map: HashMap<String, CStatePermissions>,
}

impl Clone for CStateNetworkManager {
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
            main_actor: self.main_actor.clone(),
            translator_actors: Arc::clone(&self.translator_actors),
            room_actors: Arc::clone(&self.room_actors),
            self_addr: self.self_addr.clone(),
            permissions_map: self.permissions_map.clone(),
        }
    }
}

impl CStateNetworkManager {
    /// Creates a new CStateNetworkManager.
    ///
    /// # Arguments
    /// * `main_actor` - Address of the CStateActor (business logic)
    /// * `router` - RouterActor for data-plane message routing
    /// * `permissions_map` - Map of role strings to CStatePermissions
    pub fn new(
        main_actor: Addr<crate::actor::CStateActor>,
        router: Addr<RouterActor>,
        permissions_map: HashMap<String, CStatePermissions>,
    ) -> Self {
        Self {
            router,
            main_actor,
            translator_actors: Arc::new(RwLock::new(HashMap::new())),
            room_actors: Arc::new(RwLock::new(HashMap::new())),
            self_addr: None,
            permissions_map,
        }
    }

    /// Get the permissions map (for testing)
    pub fn permissions_map(&self) -> &HashMap<String, CStatePermissions> {
        &self.permissions_map
    }
}

impl Actor for CStateNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("CStateNetworkManager started");

        // Set our own address for RoomManager implementation
        let addr: Addr<CStateNetworkManager> = ctx.address();
        self.self_addr = Some(addr);

        // Register ourselves as a RoomManager with the Router
        let manager = self.self_addr.as_ref().unwrap().clone().recipient::<CreateRoomForPeer>();
        let rooms = vec![RoomId::from("cstate")];
        let register_msg = zznet_router::RegisterManager { manager, rooms };
        self.router.do_send(register_msg);

        // Note: PeerLifecycleEvent subscription deferred until PeerManager is standalone.
        // Currently using SessionManager bridge pattern (see IntentConfig for reference).

        // Note: Room<T> registration will be added when Room<T> integration is complete.
        // TypedSender is provided via with_typed_sender() during initialization.
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(
            "CStateNetworkManager stopped, cleaning up {} NetworkActors and {} RoomActors",
            self.translator_actors.read().unwrap().len(),
            self.room_actors.read().unwrap().len()
        );
        // Actors will be automatically stopped when dropped
    }
}

// ============================================================================
// Peer Lifecycle Event Handlers
// ============================================================================

impl Handler<PeerAdded> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerAdded, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Peer added: {:?}", msg.peer_id);
        // Actors are now created in create_for_peer when Router calls it
    }
}

impl Handler<PeerRemoved> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerRemoved, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Peer removed: {:?}", msg.peer_id);

        let mut removed_translator = false;
        let mut removed_room_actor = false;

        if let Some(_actor) = self.translator_actors.write().unwrap().remove(&msg.peer_id) {
            removed_translator = true;
        }

        if let Some(_actor) = self.room_actors.write().unwrap().remove(&msg.peer_id) {
            removed_room_actor = true;
        }

        if removed_translator || removed_room_actor {
            debug!(
                "Removed actors for peer {}, remaining translators: {}, room actors: {}",
                msg.peer_id,
                self.translator_actors.read().unwrap().len(),
                self.room_actors.read().unwrap().len()
            );
        } else {
            warn!("Peer {} not found in actor maps", msg.peer_id);
        }
    }
}

// ============================================================================
// Outbound Message Handlers (MainActor → NetworkManager → NetworkActor)
// ============================================================================

impl Handler<SendHeartbeatAck> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendHeartbeatAck, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Routing HeartbeatAck to peer: {:?}", msg.peer_id);

        let network_msg = CStateMessage::HeartbeatAck {
            timestamp_ms: msg.timestamp_ms,
            server_time_ms: msg.server_time_ms,
        };

        if let Some(room_actor) = self.room_actors.read().unwrap().get(&msg.peer_id) {
            room_actor.do_send(network_msg);
        } else {
            warn!("No RoomActor found for peer: {:?}", msg.peer_id);
        }
    }
}

impl Handler<SendCollectorList> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendCollectorList, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Routing CollectorList to peer: {:?}", msg.peer_id);

        let network_msg = CStateMessage::CollectorList {
            collectors: msg.collectors,
        };

        if let Some(room_actor) = self.room_actors.read().unwrap().get(&msg.peer_id) {
            room_actor.do_send(network_msg);
        } else {
            warn!("No RoomActor found for peer: {:?}", msg.peer_id);
        }
    }
}

impl Handler<SendRegistrationRejected> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendRegistrationRejected, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Routing RegistrationRejected to peer: {:?}", msg.peer_id);

        let network_msg = CStateMessage::RegistrationRejected { reason: msg.reason };

        if let Some(room_actor) = self.room_actors.read().unwrap().get(&msg.peer_id) {
            room_actor.do_send(network_msg);
        } else {
            warn!("No RoomActor found for peer: {:?}", msg.peer_id);
        }
    }
}

impl Handler<SendUnauthorized> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendUnauthorized, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Routing Unauthorized to peer: {:?}", msg.peer_id);

        let network_msg = CStateMessage::Unauthorized { reason: msg.reason };

        if let Some(room_actor) = self.room_actors.read().unwrap().get(&msg.peer_id) {
            room_actor.do_send(network_msg);
        } else {
            warn!("No RoomActor found for peer: {:?}", msg.peer_id);
        }
    }
}

impl Handler<BroadcastHeartbeat> for CStateNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: BroadcastHeartbeat, _ctx: &mut Context<Self>) -> Self::Result {
        debug!(
            "Broadcasting heartbeat from collector {} to {} peers",
            msg.collector_id,
            self.room_actors.read().unwrap().len()
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

        // Broadcast to all RoomActors
        for (peer_id, room_actor) in self.room_actors.read().unwrap().iter() {
            debug!("Sending heartbeat to peer: {:?}", peer_id);
            room_actor.do_send(network_msg.clone());
        }
    }
}

#[async_trait::async_trait]
impl Handler<CreateRoomForPeer> for CStateNetworkManager {
    type Result = Result<Option<RoomInboundRecipient>, CreateError>;

    fn handle(&mut self, msg: CreateRoomForPeer, _ctx: &mut Context<Self>) -> Self::Result {
        // Only handle the "cstate" room
        if msg.room_id != RoomId::from("cstate") {
            return Ok(None);
        }

        // Translate Role to Permissions using the policy map
        let permissions = self.permissions_map
            .get(msg.role.as_str())
            .cloned()
            .ok_or_else(|| {
                warn!(
                    "Role '{}' not found in permissions map for peer {}",
                    msg.role.as_str(),
                    msg.peer_id
                );
                CreateError::InvalidPermission {
                    room_id: msg.room_id.clone(),
                }
            })?;

        debug!(
            "Creating NetworkActor for peer {} with role '{}': permissions = {:?}",
            msg.peer_id,
            msg.role.as_str(),
            permissions
        );

        // Create the translator actor with the peer's permissions (not role)
        let translator = CStateNetworkActor::new(
            msg.peer_id.clone(),
            permissions,
            self.main_actor.clone(),
            self.self_addr.as_ref().unwrap().clone(),
        );

        // Start the translator actor
        let translator_addr = translator.start();

        // Create the RoomActor<CStateMessage>
        let room_actor = RoomActor::new(
            RoomId::from("cstate"),
            msg.outbound_to_peer,
            translator_addr.clone().recipient::<CStateMessage>(),
        );

        // Start the RoomActor
        let room_actor_addr = room_actor.start();

        // Store the addresses in the maps
        {
            let mut translators = self.translator_actors.write().unwrap();
            translators.insert(msg.peer_id.clone(), translator_addr);

            let mut room_actors = self.room_actors.write().unwrap();
            room_actors.insert(msg.peer_id.clone(), room_actor_addr.clone());
        }

        // Return the RoomActor's raw inbound recipient
        let recipient = RoomActor::inbound_recipient(&room_actor_addr);

        Ok(Some(recipient))
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
