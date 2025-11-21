//! NetworkManager for MemDB component - orchestrates peer lifecycle and message routing.
//!
//! This module implements the NetworkManager actor in the three-actor pattern:
//! - **MainActor** (MemDBActor): Pure business logic, zero network dependencies
//! - **NetworkManager** (this file): Peer lifecycle, message routing orchestration
//! - **NetworkActor**: Per-peer protocol translation
//!
//! ## Responsibilities
//!
//! 1. **Peer Lifecycle Management**:
//!    - Spawns MemDBNetworkActor when peer joins
//!    - Removes NetworkActor when peer leaves
//!    - Tracks all active peer connections
//!
//! 2. **Message Routing**:
//!    - Routes SendBatchAck to specific collector peer
//!    - Routes SendQueryResponse to specific admin peer
//!    - Routes SendSubmitBatch to database peer
//!
//! 3. **Room Management**:
//!    - Stores Room<MemDBMessage> for network communication
//!    - Provides Room access to NetworkActors

use actix::prelude::*;
use std::collections::HashMap;
use zznet_api::types::{PeerId, RoomId};
use zznet_room::actor::RoomActor;
use zznet_router::{NetworkComponent, RouterActor};

use crate::actor::MemDBActor;
use crate::internal_messages::{SendBatchAck, SendQueryResponse, SendSubmitBatch};
use crate::network_actor::MemDBNetworkActor;
use crate::network_messages::MemDBMessage;
use crate::permissions::MemDBPermissions;

/// MemDB Manifest for the NetworkComponent pattern.
///
/// This Zero-Sized Type (ZST) binds together all the types for MemDB,
/// eliminating the need for custom factory and RegisterPeer implementations.
#[derive(Clone)]
pub struct MemDBManifest;

impl NetworkComponent for MemDBManifest {
    const ROOM_ID: &'static str = "memdb";

    type MainActor = MemDBActor;
    type ProtocolMessage = MemDBMessage;
    type NetworkActor = MemDBNetworkActor;
    type ManagerActor = MemDBNetworkManager;
    type Permissions = MemDBPermissions;

    fn create_network_actor(
        peer_id: PeerId,
        perms: Self::Permissions,
        main: Addr<Self::MainActor>,
        mgr: Addr<Self::ManagerActor>,
    ) -> Self::NetworkActor {
        MemDBNetworkActor::new(peer_id, perms, main, mgr)
    }
}

/// NetworkManager orchestrates peer lifecycle and message routing for MemDB.
///
/// This actor sits between MainActor and NetworkActors:
/// - Spawns/removes NetworkActors as peers join/leave
/// - Routes outbound messages to appropriate peer's RoomActor
/// - Manages RoomActor addresses for network communication
pub struct MemDBNetworkManager {
    /// Reference to the MainActor for business logic
    main_actor: Addr<MemDBActor>,

    /// Active NetworkActors, one per connected peer
    translators: HashMap<PeerId, Addr<MemDBNetworkActor>>,

    /// RoomActor addresses for outbound sends
    room_actors: HashMap<PeerId, Addr<RoomActor<MemDBMessage>>>,

    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,

    /// Permissions map for role-to-permissions translation
    permissions_map: HashMap<String, MemDBPermissions>,
}

impl Clone for MemDBNetworkManager {
    fn clone(&self) -> Self {
        Self {
            main_actor: self.main_actor.clone(),
            translators: self.translators.clone(),
            room_actors: self.room_actors.clone(),
            router_actor: self.router_actor.clone(),
            permissions_map: self.permissions_map.clone(),
        }
    }
}

impl MemDBNetworkManager {
    /// Create a new NetworkManager.
    pub fn new(
        main_actor: Addr<MemDBActor>,
        router_actor: Addr<RouterActor>,
        permissions_map: HashMap<String, MemDBPermissions>,
    ) -> Self {
        Self {
            main_actor,
            translators: HashMap::new(),
            room_actors: HashMap::new(),
            router_actor,
            permissions_map,
        }
    }
}

impl Actor for MemDBNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        tracing::debug!("MemDBNetworkManager started");

        // Register with router using the StandardRoomFactory
        let factory = std::sync::Arc::new(zznet_router::StandardRoomFactory::new(
            MemDBManifest,
            self.main_actor.clone(),
            ctx.address(),
            self.permissions_map.clone(),
        ));
        let rooms = vec![RoomId::from("memdb")];
        let register_msg = zznet_router::RegisterManager { factory, rooms };
        self.router_actor.do_send(register_msg);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!(
            "MemDBNetworkManager stopped, cleaning up {} NetworkActors and {} RoomActors",
            self.translators.len(),
            self.room_actors.len()
        );
        // Actors will be automatically stopped when dropped
    }
}

// ============================================================================
// PEER LIFECYCLE HANDLERS
// ============================================================================

/// Handler for the RegisterPeer message from the RoomFactory.
///
/// The factory creates actors synchronously and sends this fire-and-forget message
/// to register them with the manager for broadcasting and peer tracking.
impl Handler<zznet_router::RegisterPeer<MemDBManifest>> for MemDBNetworkManager {
    type Result = ();

    fn handle(
        &mut self,
        msg: zznet_router::RegisterPeer<MemDBManifest>,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        tracing::debug!(
            "MemDBNetworkManager: Registering peer {} with network and room actors",
            msg.peer_id
        );
        self.translators
            .insert(msg.peer_id.clone(), msg.network_actor);
        self.room_actors.insert(msg.peer_id, msg.room_actor);
    }
}

// ============================================================================
// MESSAGE ROUTING HANDLERS
// ============================================================================

/// Route batch acknowledgment to specific collector peer.
impl Handler<SendBatchAck> for MemDBNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendBatchAck, _ctx: &mut Self::Context) -> Self::Result {
        tracing::trace!(
            "Routing BatchAck to peer {}: {} results",
            msg.peer_id,
            msg.received_count
        );

        let network_msg = MemDBMessage::BatchAck {
            received_count: msg.received_count,
            timestamp_ms: msg.timestamp_ms,
        };

        if let Some(room_actor) = self.room_actors.get(&msg.peer_id) {
            room_actor.do_send(network_msg);
        } else {
            tracing::warn!("No RoomActor found for peer {}", msg.peer_id);
        }
    }
}

/// Route query response to specific admin peer.
impl Handler<SendQueryResponse> for MemDBNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendQueryResponse, _ctx: &mut Self::Context) -> Self::Result {
        tracing::trace!(
            "Routing QueryResponse to peer {}: {} results",
            msg.peer_id,
            msg.results.len()
        );

        let network_msg = MemDBMessage::QueryResponse {
            results: msg.results,
        };

        if let Some(room_actor) = self.room_actors.get(&msg.peer_id) {
            room_actor.do_send(network_msg);
        } else {
            tracing::warn!("No RoomActor found for peer {}", msg.peer_id);
        }
    }
}

/// Route submit batch to database peer.
impl Handler<SendSubmitBatch> for MemDBNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: SendSubmitBatch, _ctx: &mut Self::Context) -> Self::Result {
        tracing::trace!(
            "Routing SubmitBatch to peer {}: {} results",
            msg.peer_id,
            msg.results.len()
        );

        // Note: sender_peer_id will be filled by SessionManager during actual send
        let network_msg = MemDBMessage::SubmitBatch {
            sender_peer_id: String::new(), // Filled by SessionManager/transport layer
            timestamp_ms: msg.timestamp_ms,
            results: msg.results,
        };

        if let Some(room_actor) = self.room_actors.get(&msg.peer_id) {
            room_actor.do_send(network_msg);
        } else {
            tracing::warn!("No RoomActor found for peer {}", msg.peer_id);
        }
    }
}
