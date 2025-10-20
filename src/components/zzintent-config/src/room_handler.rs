//! Room handler that forwards network messages to IntentConfigActor
//!
//! This module provides a bridge between the SessionManager's room system
//! and the IntentConfigActor, allowing the actor to receive messages from peers.

use actix::prelude::*;
use zznet_session::peer_session::RoomHandle;
use zznet_session::types::{RoomId, SessionError};

use crate::messages::NetworkMessageReceived;
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

impl RoomHandle<IntentConfigNetworkMsg> for IntentConfigRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, msg: IntentConfigNetworkMsg) -> Result<(), SessionError> {
        // Forward the network message to the actor using do_send (fire-and-forget)
        self.actor_addr.do_send(NetworkMessageReceived(msg));
        Ok(())
    }

    fn spawn_forwarder(
        &mut self,
        _tx: tokio::sync::mpsc::Sender<(RoomId, IntentConfigNetworkMsg)>,
    ) -> Result<(), SessionError> {
        // This handler is receive-only, no outbound forwarding needed
        Ok(())
    }
}
