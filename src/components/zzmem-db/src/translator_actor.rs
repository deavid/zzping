//! TranslatorActor for MemDB component - per-peer protocol translation.
//!
//! This module implements the TranslatorActor in the three-actor pattern:
//! - **MainActor** (MemDBActor): Pure business logic, zero network dependencies
//! - **NetworkManager**: Peer lifecycle, message routing orchestration
//! - **TranslatorActor** (this file): Per-peer protocol translation
//!
//! ## Responsibilities
//!
//! 1. **Inbound Translation** (Network → MainActor):
//!    - Receives MemDBMessage from RoomActor<T> via Handler<MemDBMessage>
//!    - Translates to internal messages (InboundSubmitBatch, InboundQuery, etc.)
//!    - Forwards to MainActor with peer_id context
//!
//! 2. **Per-Peer Context**:
//!    - Each TranslatorActor is tied to one peer
//!    - Adds peer_id to all messages
//!    - Handles protocol-level concerns

use actix::prelude::*;
use zznet_api::types::PeerId;

use crate::actor::MemDBActor;
use crate::internal_messages::{
    InboundBatchAck, InboundQuery, InboundQueryResponse, InboundSubmitBatch,
};
use crate::network_manager::MemDBNetworkManager;
use crate::network_messages::MemDBMessage;

/// TranslatorActor handles protocol translation for a single peer.
///
/// One TranslatorActor is created per connected peer. It:
/// - Receives network messages and translates them to internal messages
/// - Sends network messages on behalf of MainActor
/// - Provides per-peer context (peer_id) to all messages
pub struct MemDBTranslatorActor {
    /// The peer ID this actor represents
    peer_id: PeerId,

    /// Reference to MainActor for forwarding inbound messages
    main_actor: Addr<MemDBActor>,

    /// Reference to NetworkManager (reserved for future error reporting)
    #[allow(dead_code)]
    manager: Addr<MemDBNetworkManager>,
}

impl MemDBTranslatorActor {
    /// Create a new TranslatorActor for the given peer.
    pub fn new(
        peer_id: PeerId,
        main_actor: Addr<MemDBActor>,
        manager: Addr<MemDBNetworkManager>,
    ) -> Self {
        Self {
            peer_id,
            main_actor,
            manager,
        }
    }
}

impl Actor for MemDBTranslatorActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::trace!("MemDBTranslatorActor started for peer {}", self.peer_id);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::trace!("MemDBTranslatorActor stopped for peer {}", self.peer_id);
    }
}

// ============================================================================
// INBOUND PROTOCOL TRANSLATION (Network → MainActor)
// ============================================================================

impl Handler<MemDBMessage> for MemDBTranslatorActor {
    type Result = ();

    fn handle(&mut self, msg: MemDBMessage, _ctx: &mut Self::Context) -> Self::Result {
        tracing::trace!(
            "TranslatorActor received message from peer {}: {:?}",
            self.peer_id,
            msg
        );

        match msg {
            MemDBMessage::SubmitBatch {
                sender_peer_id: _,
                timestamp_ms,
                results,
            } => {
                self.main_actor.do_send(InboundSubmitBatch {
                    peer_id: self.peer_id.clone(),
                    timestamp_ms,
                    results,
                });
            }
            MemDBMessage::Query {
                sender_peer_id: _,
                target,
                from_ms,
                to_ms,
            } => {
                self.main_actor.do_send(InboundQuery {
                    peer_id: self.peer_id.clone(),
                    target,
                    from_ms,
                    to_ms,
                });
            }
            MemDBMessage::BatchAck {
                received_count,
                timestamp_ms,
            } => {
                self.main_actor.do_send(InboundBatchAck {
                    peer_id: self.peer_id.clone(),
                    received_count,
                    timestamp_ms,
                });
            }
            MemDBMessage::QueryResponse { results } => {
                self.main_actor.do_send(InboundQueryResponse {
                    peer_id: self.peer_id.clone(),
                    results,
                });
            }
        }
    }
}

// ============================================================================
// OUTBOUND PROTOCOL TRANSLATION (MainActor → Network)
// ============================================================================
// OUTBOUND HANDLING REMOVED - Now handled by RoomActor<T>
// ============================================================================
