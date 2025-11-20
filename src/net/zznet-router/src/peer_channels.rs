use actix::Recipient;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use zznet_api::types::{PeerId, RoomId};
use zznet_room::room_manager::InboundRoomPayload;

use crate::error::SessionError;

/// Shared room storage for a peer.
type SessionRooms = HashMap<RoomId, Recipient<InboundRoomPayload>>;

/// Builder for PeerChannels with immutable construction.
///
/// Collects rooms before connecting transport channels.
pub(crate) struct PeerChannelsBuilder {
    peer_id: PeerId,
    rooms: HashMap<RoomId, Recipient<InboundRoomPayload>>,
}

impl PeerChannelsBuilder {
    /// Create a new builder for a peer.
    pub(crate) fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            rooms: HashMap::new(),
        }
    }

    /// Add a room to this peer.
    pub(crate) fn add_room(
        &mut self,
        room_id: RoomId,
        room: Recipient<InboundRoomPayload>,
    ) -> Result<(), SessionError> {
        if self.rooms.contains_key(&room_id) {
            return Err(SessionError::RoomAlreadyExists {
                peer_id: self.peer_id.clone(),
                room_id,
            });
        }
        self.rooms.insert(room_id, room);
        Ok(())
    }

    /// Build the PeerChannels by connecting transport channels.
    ///
    /// Spawns the inbound routing task and sets up channels.
    pub(crate) fn build(self, inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>) -> PeerChannels {
        let rooms_map = self.rooms;

        let rooms = rooms_map;
        let peer_id = self.peer_id.clone();
        let task = tokio::spawn(PeerChannels::inbound_task_loop(rooms, peer_id, inbound_rx));

        PeerChannels {
            peer_id: self.peer_id,
            inbound_task: task,
        }
    }
}

/// Data-plane channel set for a peer.
///
/// Immutable after construction; owns transport channels and routing task.
pub(crate) struct PeerChannels {
    pub(crate) peer_id: PeerId,
    inbound_task: JoinHandle<()>,
}

impl PeerChannels {
    /// Helper for routing inbound bytes to rooms.
    async fn route_inbound_message(
        rooms: &SessionRooms,
        peer_id: &PeerId,
        room_id: RoomId,
        bytes: Vec<u8>,
    ) {
        if let Some(room_recipient) = rooms.get(&room_id) {
            let message = InboundRoomPayload { payload: bytes };
            room_recipient.do_send(message);
        } else {
            tracing::warn!(
                "Received message for unknown/unjoined room {} on peer {}",
                room_id,
                peer_id
            );
        }
    }

    async fn inbound_task_loop(
        rooms: SessionRooms,
        peer_id: PeerId,
        mut inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
    ) {
        while let Some((room_id, bytes)) = inbound_rx.recv().await {
            Self::route_inbound_message(&rooms, &peer_id, room_id, bytes).await;
        }
        tracing::debug!("Peer {} inbound task stopped", peer_id);
    }

    /// Disconnect transport wiring and stop routing tasks.
    // TODO: This is called by RouterActor::OnPeerDisconnected. An integration test is needed.
    pub(crate) fn disconnect(&self) {
        self.inbound_task.abort();
    }
}
