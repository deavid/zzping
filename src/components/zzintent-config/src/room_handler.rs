//! Room handler that forwards network messages to IntentConfigActor
//!
//! This module provides a bridge between the SessionManager's room system
//! and the IntentConfigActor, allowing the actor to receive messages from peers.
//!
//! **NOTE**: This module is now obsolete as we've migrated to Room<T> which handles
//! serialization internally. IntentConfigActor now implements Handler<IntentConfigNetworkMsg>
//! and uses zznet-room::Room<IntentConfigNetworkMsg> for typed messaging.

use actix::prelude::*;
use zznet_session::peer_session::RoomHandle;
use zznet_session::types::{RoomId, SessionError};

use crate::network_messages::IntentConfigNetworkMsg;
use crate::permissions::IntentConfigPermission;

/// A room handler that forwards IntentConfigNetworkMsg to an IntentConfigActor
pub struct IntentConfigRoomHandler {
    /// Address of the IntentConfigActor to send messages to
    actor_addr: Addr<crate::actor::IntentConfigActor<IntentConfigPermission>>,
    /// The room ID this handler is for
    room_id: RoomId,
}

impl IntentConfigRoomHandler {
    /// Create a new room handler that forwards messages to the given actor
    pub fn new(
        actor_addr: Addr<crate::actor::IntentConfigActor<IntentConfigPermission>>,
        room_id: RoomId,
    ) -> Self {
        Self {
            actor_addr,
            room_id,
        }
    }
}

impl RoomHandle for IntentConfigRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, bytes: Vec<u8>) -> Result<(), SessionError> {
        // Deserialize the bytes to IntentConfigNetworkMsg
        let config = bincode::config::standard();
        match bincode::serde::decode_from_slice::<IntentConfigNetworkMsg, _>(&bytes, config) {
            Ok((msg, _)) => {
                // Forward the deserialized message to the actor using do_send (fire-and-forget)
                self.actor_addr.do_send(msg);
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to deserialize IntentConfigNetworkMsg: {:?}", e);
                Err(SessionError::RoomNotFound {
                    peer_id: zznet_session::types::PeerId::from("unknown"),
                    room_id: self.room_id.clone(),
                })
            }
        }
    }

    fn spawn_forwarder(
        &mut self,
        _tx: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>,
    ) -> Result<(), SessionError> {
        // This handler is receive-only, no outbound forwarding needed
        Ok(())
    }
}
