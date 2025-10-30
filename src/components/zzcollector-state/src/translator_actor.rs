//! Per-peer Translator Actor for the CState component.
//!
//! Each TranslatorActor handles protocol translation for a single peer connection.
//! It translates between network messages (CStateMessage) and internal messages.
//! This replaces the old NetworkActor which handled both serialization and translation.

use crate::{
    internal_messages::{
        InboundCollectorList, InboundHeartbeat, InboundHeartbeatAck, InboundQueryCollectors,
        InboundRegistrationRejected, InboundUnauthorized,
    },
    network_messages::CStateMessage,
};
use actix::prelude::*;
use log::debug;
use zznet_api::types::PeerId;

/// Per-peer TranslatorActor that handles protocol translation.
///
/// Responsibilities:
/// - Translate CStateMessage (network, typed) to internal messages (MainActor)
/// - Handle network errors (peer disconnected, send failures)
/// - No longer handles raw bytes or serialization (delegated to RoomActor<T>)
pub struct CStateTranslatorActor {
    /// The peer ID this actor manages.
    peer_id: PeerId,

    /// Link to MainActor for forwarding inbound messages.
    main_actor: Addr<crate::actor::CStateActor>,

    /// Link to NetworkManager for error reporting (reserved for future use).
    #[allow(dead_code)]
    manager: Addr<crate::network_manager::CStateNetworkManager>,
}

impl CStateTranslatorActor {
    /// Creates a new CStateTranslatorActor for a specific peer.
    ///
    /// # Arguments
    /// * `peer_id` - The peer ID this actor manages
    /// * `main_actor` - Address of the CStateActor (business logic)
    /// * `manager` - Address of the NetworkManager (parent)
    pub fn new(
        peer_id: PeerId,
        main_actor: Addr<crate::actor::CStateActor>,
        manager: Addr<crate::network_manager::CStateNetworkManager>,
    ) -> Self {
        Self {
            peer_id,
            main_actor,
            manager,
        }
    }
}

impl Actor for CStateTranslatorActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        debug!("CStateTranslatorActor started for peer: {:?}", self.peer_id);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        debug!("CStateTranslatorActor stopped for peer: {:?}", self.peer_id);
    }
}

// ============================================================================
// Inbound Message Handlers (RoomActor<T> → TranslatorActor → MainActor)
// ============================================================================

impl Handler<CStateMessage> for CStateTranslatorActor {
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
// Outbound handling removed - Now handled by RoomActor<T>
// ============================================================================

#[cfg(test)]
mod tests {
    // Tests will be added in Phase 3 integration testing
}
