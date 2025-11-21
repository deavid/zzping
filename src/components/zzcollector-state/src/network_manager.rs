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
use zznet_api::types::{PeerId, RoomId};
use zznet_room::actor::RoomActor;
use zznet_router::{NetworkComponent, RouterActor};

/// CState Manifest for the NetworkComponent pattern.
///
/// This Zero-Sized Type (ZST) binds together all the types for CState,
/// eliminating the need for custom factory and RegisterPeer implementations.
#[derive(Clone)]
pub struct CStateManifest;

impl NetworkComponent for CStateManifest {
    const ROOM_ID: &'static str = "cstate";

    type MainActor = crate::actor::CStateActor;
    type ProtocolMessage = CStateMessage;
    type NetworkActor = CStateNetworkActor;
    type ManagerActor = CStateNetworkManager;
    type Permissions = CStatePermissions;

    fn create_network_actor(
        peer_id: PeerId,
        perms: Self::Permissions,
        main: Addr<Self::MainActor>,
        mgr: Addr<Self::ManagerActor>,
    ) -> Self::NetworkActor {
        CStateNetworkActor::new(peer_id, perms, main, mgr)
    }
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
    translator_actors: HashMap<PeerId, Addr<CStateNetworkActor>>,
    /// Per-peer room actors for serialization
    room_actors: HashMap<PeerId, Addr<RoomActor<CStateMessage>>>,
    /// Permissions map for role-to-permissions translation
    permissions_map: HashMap<String, CStatePermissions>,
}

impl Clone for CStateNetworkManager {
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
            main_actor: self.main_actor.clone(),
            translator_actors: self.translator_actors.clone(),
            room_actors: self.room_actors.clone(),
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
            translator_actors: HashMap::new(),
            room_actors: HashMap::new(),
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

        // Register with router using the StandardRoomFactory
        let factory = std::sync::Arc::new(zznet_router::StandardRoomFactory::new(
            CStateManifest,
            self.main_actor.clone(),
            ctx.address(),
            self.permissions_map.clone(),
        ));
        let rooms = vec![RoomId::from("cstate")];
        let register_msg = zznet_router::RegisterManager { factory, rooms };
        self.router.do_send(register_msg);

        // Note: PeerLifecycleEvent subscription deferred until PeerManager is standalone.
        // Currently using SessionManager bridge pattern (see IntentConfig for reference).

        // Note: Room<T> registration will be added when Room<T> integration is complete.
        // TypedSender is provided via with_typed_sender() during initialization.
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(
            "CStateNetworkManager stopped, cleaning up {} NetworkActors and {} RoomActors",
            self.translator_actors.len(),
            self.room_actors.len()
        );
        // Actors will be automatically stopped when dropped
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

        if let Some(room_actor) = self.room_actors.get(&msg.peer_id) {
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

        if let Some(room_actor) = self.room_actors.get(&msg.peer_id) {
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

        if let Some(room_actor) = self.room_actors.get(&msg.peer_id) {
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

        if let Some(room_actor) = self.room_actors.get(&msg.peer_id) {
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
            self.room_actors.len()
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
        for (peer_id, room_actor) in self.room_actors.iter() {
            debug!("Sending heartbeat to peer: {:?}", peer_id);
            room_actor.do_send(network_msg.clone());
        }
    }
}

// ============================================================================
// Handler: RegisterPeer (from RoomFactory)
// ============================================================================

/// Handler for the RegisterPeer message from the RoomFactory.
///
/// The factory creates actors synchronously and sends this fire-and-forget message
/// to register them with the manager for broadcasting and peer tracking.
impl Handler<zznet_router::RegisterPeer<CStateManifest>> for CStateNetworkManager {
    type Result = ();

    fn handle(
        &mut self,
        msg: zznet_router::RegisterPeer<CStateManifest>,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        debug!(
            "CStateNetworkManager: Registering peer {} with network and room actors",
            msg.peer_id
        );
        self.translator_actors
            .insert(msg.peer_id.clone(), msg.network_actor);
        self.room_actors.insert(msg.peer_id, msg.room_actor);
    }
}

#[cfg(test)]
mod tests {
    // Phase 4.4: Tests will be added when implementing NetworkManager functionality.
    // These will cover:
    // - NetworkActor spawning and cleanup
    // - Message routing (unicast and broadcast)
    // - SessionManager bridge pattern
}
