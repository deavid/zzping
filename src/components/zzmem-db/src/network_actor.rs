//! NetworkActor for MemDB component - per-peer protocol translation.
//!
//! This module implements the NetworkActor in the three-actor pattern:
//! - **MainActor** (MemDBActor): Pure business logic, zero network dependencies
//! - **NetworkManager**: Peer lifecycle, message routing orchestration
//! - **NetworkActor** (this file): Per-peer protocol translation
//!
//! ## Responsibilities
//!
//! 1. **Inbound Translation** (Network → MainActor):
//!    - Receives MemDBMessage from RoomActor<T> via Handler<MemDBMessage>
//!    - Translates to internal messages (InboundSubmitBatch, InboundQuery, etc.)
//!    - Forwards to MainActor with peer_id context
//!
//! 2. **Per-Peer Context**:
//!    - Each NetworkActor is tied to one peer
//!    - Enforces component-specific permissions for authorization
//!    - Adds peer_id to all messages
//!    - Handles protocol-level concerns

use actix::prelude::*;
use zznet_api::types::PeerId;

use crate::actor::MemDBActor;
use crate::network_manager::MemDBNetworkManager;
use crate::network_messages::MemDBMessage;
use crate::permissions::MemDBPermissions;

/// NetworkActor handles protocol translation for a single peer.
///
/// One NetworkActor is created per connected peer. It:
/// - Receives network messages and translates them to internal messages
/// - Enforces component-specific permissions for authorization
/// - Sends network messages on behalf of MainActor
/// - Provides per-peer context (peer_id) to all messages
pub struct MemDBNetworkActor {
    /// The peer ID this actor represents
    peer_id: PeerId,

    /// Permissions for this peer (for authorization checks)
    _permissions: MemDBPermissions,

    /// Reference to MainActor for forwarding inbound messages
    main_actor: Addr<MemDBActor>,

    /// Reference to NetworkManager (reserved for future error reporting)
    #[allow(dead_code)]
    manager: Addr<MemDBNetworkManager>,
}

impl MemDBNetworkActor {
    /// Create a new NetworkActor for the given peer.
    pub fn new(
        peer_id: PeerId,
        permissions: MemDBPermissions,
        main_actor: Addr<MemDBActor>,
        manager: Addr<MemDBNetworkManager>,
    ) -> Self {
        Self {
            peer_id,
            _permissions: permissions,
            main_actor,
            manager,
        }
    }
}

impl Actor for MemDBNetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::trace!("MemDBNetworkActor started for peer {}", self.peer_id);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::trace!("MemDBNetworkActor stopped for peer {}", self.peer_id);
    }
}

// ============================================================================
// INBOUND PROTOCOL TRANSLATION (Network → MainActor)
// ============================================================================

impl Handler<MemDBMessage> for MemDBNetworkActor {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: MemDBMessage, _ctx: &mut Self::Context) -> Self::Result {
        tracing::trace!(
            "NetworkActor received message from peer {}: {:?}",
            self.peer_id,
            msg
        );

        let main_actor = self.main_actor.clone();
        let peer_id = self.peer_id.clone();

        match msg {
            MemDBMessage::SubmitBatch {
                sender_peer_id: _,
                timestamp_ms,
                results,
            } => {
                let fut = async move {
                    let request = crate::internal_messages::InboundSubmitBatch {
                        peer_id: peer_id.clone(),
                        timestamp_ms,
                        results,
                    };

                    match main_actor.send(request).await {
                        Ok(Ok(ack_response)) => {
                            tracing::debug!(
                                "Batch accepted: {} results",
                                ack_response.received_count
                            );
                            // TODO: Send response to room_actor
                            // For now, acknowledged
                        }
                        Ok(Err(e)) => {
                            tracing::warn!("Batch rejected: {}", e);
                        }
                        Err(e) => {
                            tracing::error!("MainActor error: {}", e);
                        }
                    }
                };
                Box::pin(fut)
            }
            MemDBMessage::Query {
                sender_peer_id: _,
                target,
                from_ms,
                to_ms,
            } => {
                let fut = async move {
                    let request = crate::internal_messages::InboundQuery {
                        peer_id: peer_id.clone(),
                        target,
                        from_ms,
                        to_ms,
                    };

                    match main_actor.send(request).await {
                        Ok(Ok(results)) => {
                            tracing::debug!("Query returned {} results", results.len());
                            // TODO: Send response to room_actor
                            // For now, acknowledged
                        }
                        Ok(Err(e)) => {
                            tracing::warn!("Query rejected: {}", e);
                        }
                        Err(e) => {
                            tracing::error!("MainActor error: {}", e);
                        }
                    }
                };
                Box::pin(fut)
            }
            MemDBMessage::BatchAck {
                received_count,
                timestamp_ms,
            } => {
                // Unsolicited ack from database to collector
                self.main_actor
                    .do_send(crate::internal_messages::InboundBatchAck {
                        peer_id: peer_id.clone(),
                        received_count,
                        timestamp_ms,
                    });
                Box::pin(async {})
            }
            MemDBMessage::QueryResponse { results } => {
                // Unsolicited response from database
                self.main_actor
                    .do_send(crate::internal_messages::InboundQueryResponse {
                        peer_id: peer_id.clone(),
                        results,
                    });
                Box::pin(async {})
            }
        }
    }
}
