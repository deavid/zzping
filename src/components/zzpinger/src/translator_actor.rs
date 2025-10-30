//! Per-peer Translator Actor for the Pinger component.
//!
//! Each TranslatorActor handles protocol translation for a single peer connection.
//! It translates between network messages (PingerMessage) and internal messages.
//! This replaces the old NetworkActor which handled both serialization and translation.

use crate::messages::UpdateTargets as InternalUpdateTargets;
use crate::network_messages::PingerMessage;
use actix::prelude::*;
use log::debug;
use zznet_api::types::PeerId;

/// Per-peer TranslatorActor that handles protocol translation.
///
/// Responsibilities:
/// - Translate PingerMessage (network, typed) to internal messages (MainActor)
/// - Handle network errors (peer disconnected, send failures)
/// - No longer handles raw bytes or serialization (delegated to RoomActor<T>)
pub struct PingerTranslatorActor {
    /// The peer ID this actor manages.
    peer_id: PeerId,

    /// Link to MainActor for forwarding inbound messages.
    main_actor: Addr<crate::actor::PingerActor>,

    /// Link to NetworkManager for error reporting (reserved for future use).
    #[allow(dead_code)]
    manager: Addr<crate::network_manager::PingerNetworkManager>,
}

impl PingerTranslatorActor {
    /// Creates a new PingerTranslatorActor for a specific peer.
    ///
    /// # Arguments
    /// * `peer_id` - The peer ID this actor manages
    /// * `main_actor` - Address of the PingerActor (business logic)
    /// * `manager` - Address of the NetworkManager (parent)
    pub fn new(
        peer_id: PeerId,
        main_actor: Addr<crate::actor::PingerActor>,
        manager: Addr<crate::network_manager::PingerNetworkManager>,
    ) -> Self {
        Self {
            peer_id,
            main_actor,
            manager,
        }
    }
}

impl Actor for PingerTranslatorActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        debug!("PingerTranslatorActor started for peer: {:?}", self.peer_id);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        debug!("PingerTranslatorActor stopped for peer: {:?}", self.peer_id);
    }
}

impl Handler<PingerMessage> for PingerTranslatorActor {
    type Result = ();

    fn handle(&mut self, msg: PingerMessage, _ctx: &mut Context<Self>) -> Self::Result {
        debug!(
            "Received network message from peer {:?}: {:?}",
            self.peer_id, msg
        );

        // Translate network message to internal message and forward to MainActor
        match msg {
            PingerMessage::UpdateTargets { targets } => {
                self.main_actor.do_send(InternalUpdateTargets { targets });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests will be added in Phase 3 integration testing
}
