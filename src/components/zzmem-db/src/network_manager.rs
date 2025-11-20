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
use std::sync::{Arc, RwLock};
use zznet_api::types::{PeerId, RoomId};
use zznet_room::actor::RoomActor;
use zznet_room::room_manager::{CreateRoomForPeer, RoomInboundRecipient};
use zznet_router::RouterActor;

use crate::actor::MemDBActor;
use crate::internal_messages::{SendBatchAck, SendQueryResponse, SendSubmitBatch};
use crate::network_actor::MemDBNetworkActor;
use crate::network_messages::MemDBMessage;

/// Temporary bridge messages for SessionManager integration.
/// Phase 6.4: Using real PeerLifecycleEvent from zznet-peer-manager
/// Keeping placeholder messages for backward compatibility during migration
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct PeerAdded {
    /// Peer identifier for the newly added peer
    pub peer_id: PeerId,
}

/// Message sent when a peer is removed from the network
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct PeerRemoved {
    /// Peer identifier for the removed peer
    pub peer_id: PeerId,
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
    translators: Arc<RwLock<HashMap<PeerId, Addr<MemDBNetworkActor>>>>,

    /// RoomActor addresses for outbound sends
    room_actors: Arc<RwLock<HashMap<PeerId, Addr<RoomActor<MemDBMessage>>>>>,

    /// Address of this NetworkManager (set in started())
    self_addr: Option<Addr<MemDBNetworkManager>>,

    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,
}

impl Clone for MemDBNetworkManager {
    fn clone(&self) -> Self {
        Self {
            main_actor: self.main_actor.clone(),
            translators: Arc::clone(&self.translators),
            room_actors: Arc::clone(&self.room_actors),
            self_addr: self.self_addr.clone(),
            router_actor: self.router_actor.clone(),
        }
    }
}

impl MemDBNetworkManager {
    /// Create a new NetworkManager.
    pub fn new(main_actor: Addr<MemDBActor>, router_actor: Addr<RouterActor>) -> Self {
        Self {
            main_actor,
            translators: Arc::new(RwLock::new(HashMap::new())),
            room_actors: Arc::new(RwLock::new(HashMap::new())),
            self_addr: None,
            router_actor,
        }
    }
}

impl Actor for MemDBNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        tracing::debug!("MemDBNetworkManager started");
        // Set our own address for RoomManager implementation
        let addr: Addr<MemDBNetworkManager> = ctx.address();
        self.self_addr = Some(addr);

        // Register ourselves as a RoomManager with the Router
        let manager = self
            .self_addr
            .as_ref()
            .unwrap()
            .clone()
            .recipient::<CreateRoomForPeer>();
        let rooms = vec![RoomId::from("memdb")];
        let register_msg = zznet_router::RegisterManager { manager, rooms };
        self.router_actor.do_send(register_msg);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!(
            "MemDBNetworkManager stopped, cleaning up {} NetworkActors and {} RoomActors",
            self.translators.read().unwrap().len(),
            self.room_actors.read().unwrap().len()
        );
        // Actors will be automatically stopped when dropped
    }
}

// ============================================================================
// PEER LIFECYCLE HANDLERS
// ============================================================================

impl Handler<PeerAdded> for MemDBNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerAdded, _ctx: &mut Self::Context) -> Self::Result {
        tracing::debug!("Peer added: {}", msg.peer_id);
        // Actors are now created in create_for_peer when Router calls it
    }
}

impl Handler<PeerRemoved> for MemDBNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerRemoved, _ctx: &mut Self::Context) -> Self::Result {
        tracing::debug!("Peer removed: {}", msg.peer_id);

        let mut removed_translator = false;
        let mut removed_room_actor = false;

        if let Some(_actor) = self.translators.write().unwrap().remove(&msg.peer_id) {
            removed_translator = true;
        }

        if let Some(_actor) = self.room_actors.write().unwrap().remove(&msg.peer_id) {
            removed_room_actor = true;
        }

        if removed_translator || removed_room_actor {
            tracing::debug!(
                "Removed actors for peer {}, remaining translators: {}, room actors: {}",
                msg.peer_id,
                self.translators.read().unwrap().len(),
                self.room_actors.read().unwrap().len()
            );
        } else {
            tracing::warn!("Peer {} not found in actor maps", msg.peer_id);
        }
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

        if let Some(room_actor) = self.room_actors.read().unwrap().get(&msg.peer_id) {
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

        if let Some(room_actor) = self.room_actors.read().unwrap().get(&msg.peer_id) {
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

        if let Some(room_actor) = self.room_actors.read().unwrap().get(&msg.peer_id) {
            room_actor.do_send(network_msg);
        } else {
            tracing::warn!("No RoomActor found for peer {}", msg.peer_id);
        }
    }
}

impl Handler<CreateRoomForPeer> for MemDBNetworkManager {
    type Result = Result<Option<RoomInboundRecipient>, ()>;

    fn handle(&mut self, msg: CreateRoomForPeer, _ctx: &mut Context<Self>) -> Self::Result {
        // Only handle the "memdb" room
        if msg.room_id != RoomId::from("memdb") {
            return Ok(None);
        }

        tracing::debug!(
            "Creating MemDBNetworkActor and RoomActor for peer: {:?}",
            msg.peer_id
        );

        // Create the translator actor with the peer's role
        let translator = MemDBNetworkActor::new(
            msg.peer_id.clone(),
            msg.role,
            self.main_actor.clone(),
            self.self_addr.as_ref().unwrap().clone(),
        );

        // Start the translator actor
        let translator_addr = translator.start();

        // Create the RoomActor<MemDBMessage>
        let room_actor = RoomActor::new(
            RoomId::from("memdb"),
            msg.outbound_to_peer,
            translator_addr.clone().recipient::<MemDBMessage>(),
        );

        // Start the RoomActor
        let room_actor_addr = room_actor.start();

        // Store the addresses in the maps
        {
            let mut translators = self.translators.write().unwrap();
            translators.insert(msg.peer_id.clone(), translator_addr);

            let mut room_actors = self.room_actors.write().unwrap();
            room_actors.insert(msg.peer_id.clone(), room_actor_addr.clone());
        }

        // Return the RoomActor's raw inbound recipient
        let recipient = RoomActor::inbound_recipient(&room_actor_addr);

        Ok(Some(recipient))
    }
}
