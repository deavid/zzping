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
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};
use zznet_api::PeerId;
use zznet_room::RoomActor;

use crate::actor::MemDBActor;
use crate::events::MemDBEvent;
use crate::network_messages::{MemDBMessage, PingResult};
use crate::permissions::MemDBPermissions;

/// NetworkActor handles protocol translation for a single peer.
///
/// One NetworkActor is created per connected peer. It:
/// - Receives network messages and translates them to internal messages
/// - Enforces component-specific permissions for authorization
/// - Sends network messages on behalf of MainActor
/// - Provides per-peer context (peer_id) to all messages
pub(crate) struct MemDBNetworkActor {
    /// The peer ID this actor represents
    peer_id: PeerId,

    /// Permissions for this peer (for authorization checks)
    permissions: MemDBPermissions,

    /// Reference to MainActor for forwarding inbound messages
    main_actor: Addr<MemDBActor>,

    /// Reference to the RoomActor for outbound messaging
    room_actor: Addr<RoomActor<MemDBMessage>>,

    /// Broadcast subscription for outbound events from MainActor
    event_rx: tokio::sync::broadcast::Receiver<MemDBEvent>,
}

impl MemDBNetworkActor {
    /// Create a new NetworkActor for the given peer.
    pub(crate) fn new(
        peer_id: PeerId,
        permissions: MemDBPermissions,
        main_actor: Addr<MemDBActor>,
        event_rx: tokio::sync::broadcast::Receiver<MemDBEvent>,
        room_actor: Addr<RoomActor<MemDBMessage>>,
    ) -> Self {
        Self {
            peer_id,
            permissions,
            main_actor,
            room_actor,
            event_rx,
        }
    }

    fn enqueue_batch(&mut self, timestamp_ms: u64, results: Vec<PingResult>) {
        if !self.permissions.can_receive_batches {
            tracing::debug!(
                "Peer {} is not authorized to receive batches; skipping outbound event",
                self.peer_id
            );
            return;
        }

        tracing::trace!(
            "MemDBNetworkActor.enqueue_batch: peer {} timestamp={} results={} -> RoomActor",
            self.peer_id,
            timestamp_ms,
            results.len()
        );

        tracing::debug!(
            "MemDBNetworkActor.enqueue_batch: enqueuing SubmitBatch -> peer={} timestamp={} results={}",
            self.peer_id,
            timestamp_ms,
            results.len()
        );

        self.room_actor.do_send(MemDBMessage::SubmitBatch {
            sender_peer_id: String::new(),
            timestamp_ms,
            results,
        });

        tracing::trace!(
            "MemDBNetworkActor.enqueue_batch: peer {} handed SubmitBatch to RoomActor",
            self.peer_id
        );
    }
}

impl Actor for MemDBNetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        tracing::debug!(
            "MemDBNetworkActor started for peer {}; subscribing to event bus",
            self.peer_id
        );
        let stream = BroadcastStream::new(self.event_rx.resubscribe());
        tracing::trace!(
            "MemDBNetworkActor peer {} attaching BroadcastStream subscriber",
            self.peer_id
        );
        ctx.add_stream(stream);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::trace!("MemDBNetworkActor stopped for peer {}", self.peer_id);
    }
}

// ============================================================================
// Event Stream Handler
// ============================================================================

impl StreamHandler<Result<MemDBEvent, BroadcastStreamRecvError>> for MemDBNetworkActor {
    fn handle(
        &mut self,
        item: Result<MemDBEvent, BroadcastStreamRecvError>,
        _ctx: &mut Context<Self>,
    ) {
        match item {
            Ok(MemDBEvent::BatchReady {
                timestamp_ms,
                results,
            }) => {
                tracing::trace!(
                    "MemDBNetworkActor peer {} received BatchReady event (timestamp={} results={})",
                    self.peer_id,
                    timestamp_ms,
                    results.len()
                );
                self.enqueue_batch(timestamp_ms, results);
            }
            Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                tracing::warn!(
                    "Peer {} lagged on MemDB event stream; skipped {} events",
                    self.peer_id,
                    skipped
                );
            }
        }
    }

    fn finished(&mut self, _ctx: &mut Context<Self>) {
        tracing::debug!("Event bus stream finished for peer {}", self.peer_id);
    }
}

// ============================================================================
// INBOUND PROTOCOL TRANSLATION (Network → MainActor)
// ============================================================================

impl Handler<MemDBMessage> for MemDBNetworkActor {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: MemDBMessage, _ctx: &mut Self::Context) -> Self::Result {
        tracing::debug!(
            "MemDBNetworkActor.handle(MemDBMessage) from peer {}: {:?}",
            self.peer_id,
            msg
        );

        let main_actor = self.main_actor.clone();
        let peer_id = self.peer_id.clone();
        let room_actor = self.room_actor.clone();

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

                    tracing::debug!(
                        "MemDBNetworkActor -> forwarding SubmitBatch to MainActor for peer {}: timestamp={} results={} ",
                        peer_id,
                        timestamp_ms,
                        request.results.len()
                    );

                    match main_actor.send(request).await {
                        Ok(Ok(ack_response)) => {
                            tracing::debug!(
                                "Batch accepted: {} results",
                                ack_response.received_count
                            );
                            room_actor.do_send(MemDBMessage::BatchAck {
                                received_count: ack_response.received_count,
                                timestamp_ms: ack_response.timestamp_ms,
                            });
                            tracing::debug!(
                                "MemDBNetworkActor -> enqueued BatchAck to room for peer {} timestamp={}",
                                peer_id,
                                ack_response.timestamp_ms
                            );
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
                            room_actor.do_send(MemDBMessage::QueryResponse { results });
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
