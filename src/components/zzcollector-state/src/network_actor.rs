//! Per-peer Network Actor for the CState component.
//!
//! Each NetworkActor handles protocol translation for a single peer connection.
//! It translates between network messages (CStateMessage) and internal messages.

use crate::{
    internal_messages::{
        InboundCollectorList, InboundHeartbeat, InboundHeartbeatAck, InboundQueryCollectors,
        InboundRegistrationRejected, InboundUnauthorized, SendToNetwork,
    },
    network_messages::CStateMessage,
};
use actix::prelude::*;
use log::{debug, warn};
use zznet_api::types::PeerId;
use zznet_room::room::TypedSender;

/// Per-peer NetworkActor that handles protocol translation.
///
/// Responsibilities:
/// - Translate CStateMessage (network) to internal messages (MainActor)
/// - Send messages via TypedSender (broadcasts to all peers in room)
/// - Forward inbound messages from Room<T> to MainActor
/// - Handle network errors (peer disconnected, send failures)
pub struct CStateNetworkActor {
    /// The peer ID this actor manages.
    peer_id: PeerId,

    /// The TypedSender for broadcasting network messages.
    /// Note: CState uses broadcast, so all NetworkActors share the same sender.
    typed_sender: TypedSender<CStateMessage>,

    /// Link to MainActor for forwarding inbound messages.
    main_actor: Addr<crate::actor::CStateActor>,

    /// Link to NetworkManager for error reporting (reserved for future use).
    #[allow(dead_code)]
    manager: Addr<crate::network_manager::CStateNetworkManager>,
}

impl CStateNetworkActor {
    /// Creates a new CStateNetworkActor for a specific peer.
    ///
    /// # Arguments
    /// * `peer_id` - The peer ID this actor manages
    /// * `typed_sender` - The TypedSender for broadcasting messages
    /// * `main_actor` - Address of the CStateActor (business logic)
    /// * `manager` - Address of the NetworkManager (parent)
    pub fn new(
        peer_id: PeerId,
        typed_sender: TypedSender<CStateMessage>,
        main_actor: Addr<crate::actor::CStateActor>,
        manager: Addr<crate::network_manager::CStateNetworkManager>,
    ) -> Self {
        Self {
            peer_id,
            typed_sender,
            main_actor,
            manager,
        }
    }
}

impl Actor for CStateNetworkActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        debug!("CStateNetworkActor started for peer: {:?}", self.peer_id);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        debug!("CStateNetworkActor stopped for peer: {:?}", self.peer_id);
        // TypedSender cleanup is automatic (drops when NetworkActor drops)
    }
}

// ============================================================================
// Inbound Message Handlers (Room<T> → NetworkActor → MainActor)
// ============================================================================

impl Handler<CStateMessage> for CStateNetworkActor {
    type Result = ();

    fn handle(&mut self, msg: CStateMessage, _ctx: &mut Context<Self>) -> Self::Result {
        debug!(
            "Received network message from peer {:?}: {:?}",
            self.peer_id, msg
        );

        // Translate network message to internal message and forward to MainActor
        match msg {
            CStateMessage::Heartbeat {
                collector_id,
                uptime_secs,
                pings_sent,
                pings_received,
                batches_sent,
                last_config_update_ms,
                connection_nonce,
            } => {
                self.main_actor.do_send(InboundHeartbeat {
                    peer_id: self.peer_id.clone(),
                    collector_id,
                    uptime_secs,
                    pings_sent,
                    pings_received,
                    batches_sent,
                    last_config_update_ms,
                    connection_nonce,
                });
            }

            CStateMessage::HeartbeatAck {
                timestamp_ms,
                server_time_ms,
            } => {
                self.main_actor.do_send(InboundHeartbeatAck {
                    peer_id: self.peer_id.clone(),
                    timestamp_ms,
                    server_time_ms,
                });
            }

            CStateMessage::QueryCollectors => {
                self.main_actor.do_send(InboundQueryCollectors {
                    peer_id: self.peer_id.clone(),
                });
            }

            CStateMessage::CollectorList { collectors } => {
                self.main_actor.do_send(InboundCollectorList {
                    peer_id: self.peer_id.clone(),
                    collectors,
                });
            }

            CStateMessage::RegistrationRejected { reason } => {
                self.main_actor.do_send(InboundRegistrationRejected {
                    peer_id: self.peer_id.clone(),
                    reason,
                });
            }

            CStateMessage::Unauthorized { reason } => {
                self.main_actor.do_send(InboundUnauthorized {
                    peer_id: self.peer_id.clone(),
                    reason,
                });
            }
        }
    }
}

// ============================================================================
// Outbound Message Handlers (NetworkManager → NetworkActor → Room<T>)
// ============================================================================

impl Handler<SendToNetwork> for CStateNetworkActor {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, msg: SendToNetwork, _ctx: &mut Context<Self>) -> Self::Result {
        debug!(
            "Sending network message to peer {:?}: {:?}",
            self.peer_id, msg.message
        );

        // Phase 4.5: Send message via TypedSender (broadcast to all peers in room)
        let typed_sender = self.typed_sender.clone();
        let peer_id = self.peer_id.clone();
        let network_msg = msg.message;

        let fut = async move {
            if let Err(e) = typed_sender.send(network_msg).await {
                warn!("Failed to broadcast message for peer {:?}: {}", peer_id, e);
                // Note: In CState's broadcast model, we don't report individual send failures
                // to the NetworkManager. The Room<T> handles broadcast reliability.
            }
        };

        Box::pin(fut.into_actor(self))
    }
}

#[cfg(test)]
mod tests {
    // Phase 4.5: Tests will be added when implementing NetworkActor functionality.
    // These will cover:
    // - Message translation (network → internal)
    // - Message sending (internal → network)
    // - Error handling (send failures, peer disconnected)
    // - Integration with Room<T>
}
