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
use zznet_api::types::PeerId;
use zznet_room::room::TypedSender;

use crate::actor::MemDBActor;
use crate::internal_messages::{SendBatchAck, SendQueryResponse, SendSubmitBatch, SendToNetwork};
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
/// - Routes outbound messages to appropriate peer's NetworkActor
/// - Manages TypedSender for network communication
pub struct MemDBNetworkManager {
    /// Reference to the MainActor for business logic
    main_actor: Addr<MemDBActor>,

    /// Active NetworkActors, one per connected peer
    network_actors: HashMap<PeerId, Addr<MemDBNetworkActor>>,

    /// TypedSender for sending MemDBMessages over the network
    /// Can be cloned and shared with all NetworkActors
    typed_sender: Option<TypedSender<MemDBMessage>>,
}

impl MemDBNetworkManager {
    /// Create a new NetworkManager.
    ///
    /// The typed_sender should be set via `with_typed_sender()` before starting.
    pub fn new(main_actor: Addr<MemDBActor>) -> Self {
        Self {
            main_actor,
            network_actors: HashMap::new(),
            typed_sender: None,
        }
    }

    /// Set the TypedSender for network communication.
    ///
    /// The TypedSender is obtained from `room.typed_sender()` and can be cloned
    /// to share with multiple NetworkActors.
    pub fn with_typed_sender(mut self, typed_sender: TypedSender<MemDBMessage>) -> Self {
        self.typed_sender = Some(typed_sender);
        self
    }
}

impl Actor for MemDBNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!("MemDBNetworkManager started");
        // TypedSender is provided via with_typed_sender() during initialization.
        // NOTE: Will subscribe to PeerManager events when PeerManager is extracted from SessionManager
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!(
            "MemDBNetworkManager stopped, cleaning up {} NetworkActors",
            self.network_actors.len()
        );
        // NetworkActors will be automatically stopped when dropped
        // TypedSender cleanup is automatic (drops when NetworkManager drops)
    }
}

// ============================================================================
// PEER LIFECYCLE HANDLERS
// ============================================================================

impl Handler<PeerAdded> for MemDBNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerAdded, ctx: &mut Self::Context) -> Self::Result {
        tracing::debug!("Peer added: {}", msg.peer_id);

        // Note: Room membership checks will be added when Room<T> is fully integrated

        // Get TypedSender for the NetworkActor
        let typed_sender = match &self.typed_sender {
            Some(ts) => ts.clone(),
            None => {
                tracing::error!("No TypedSender available for peer {}", msg.peer_id);
                return;
            }
        };

        // Spawn NetworkActor for this peer
        let network_actor = MemDBNetworkActor::new(
            msg.peer_id.clone(),
            typed_sender,
            self.main_actor.clone(),
            ctx.address(),
        )
        .start();

        self.network_actors
            .insert(msg.peer_id.clone(), network_actor);
        tracing::debug!(
            "Spawned NetworkActor for peer {}, total actors: {}",
            msg.peer_id,
            self.network_actors.len()
        );
    }
}

impl Handler<PeerRemoved> for MemDBNetworkManager {
    type Result = ();

    fn handle(&mut self, msg: PeerRemoved, _ctx: &mut Self::Context) -> Self::Result {
        tracing::debug!("Peer removed: {}", msg.peer_id);

        if let Some(_actor) = self.network_actors.remove(&msg.peer_id) {
            tracing::debug!(
                "Removed NetworkActor for peer {}, remaining actors: {}",
                msg.peer_id,
                self.network_actors.len()
            );
            // NetworkActor will be automatically stopped when dropped
        } else {
            tracing::warn!("Peer {} not found in network_actors", msg.peer_id);
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

        if let Some(actor) = self.network_actors.get(&msg.peer_id) {
            actor.do_send(SendToNetwork {
                message: network_msg,
            });
        } else {
            tracing::warn!("No NetworkActor found for peer {}", msg.peer_id);
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

        if let Some(actor) = self.network_actors.get(&msg.peer_id) {
            actor.do_send(SendToNetwork {
                message: network_msg,
            });
        } else {
            tracing::warn!("No NetworkActor found for peer {}", msg.peer_id);
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

        if let Some(actor) = self.network_actors.get(&msg.peer_id) {
            actor.do_send(SendToNetwork {
                message: network_msg,
            });
        } else {
            tracing::warn!("No NetworkActor found for peer {}", msg.peer_id);
        }
    }
}
