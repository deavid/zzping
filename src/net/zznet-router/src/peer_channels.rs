use actix::Recipient;
use std::collections::HashMap;
use tokio::sync::mpsc;
use zznet_api::protocol::{Frame, RoomFrame};
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

    /// Build the PeerChannels by connecting directly to transport.
    ///
    /// Spawns a task that reads from transport_rx, deserializes frames,
    /// and routes them to the appropriate rooms.
    pub(crate) fn build_and_spawn_transport_demux(
        self,
        transport_rx: mpsc::Receiver<Result<bytes::Bytes, zznet_api::error::TransportError>>,
    ) {
        let rooms = self.rooms;
        let peer_id = self.peer_id.clone();
        tokio::spawn(transport_demux_task(rooms, peer_id, transport_rx));
    }
}

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

/// Task that reads from transport, deserializes frames, and routes to rooms.
///
/// This is the "zero-copy" demux point - it reads directly from the transport
/// and routes messages to rooms without intermediate forwarding actors.
async fn transport_demux_task(
    rooms: SessionRooms,
    peer_id: PeerId,
    mut transport_rx: mpsc::Receiver<Result<bytes::Bytes, zznet_api::error::TransportError>>,
) {
    while let Some(result) = transport_rx.recv().await {
        match result {
            Ok(frame_bytes) => {
                // Deserialize the frame
                match Frame::deserialize(&frame_bytes) {
                    Ok(Frame::Room(room_frame)) => match room_frame {
                        RoomFrame::Message {
                            to_room, payload, ..
                        } => {
                            let room_id = RoomId::from(to_room.as_str());
                            route_inbound_message(&rooms, &peer_id, room_id, payload).await;
                        }
                        RoomFrame::Disconnect => {
                            tracing::info!("Peer {} sent disconnect", peer_id);
                            break;
                        }
                    },
                    Ok(Frame::Handshake(_)) => {
                        tracing::warn!(
                            "Received handshake frame after session established for peer {}",
                            peer_id
                        );
                    }
                    Err(e) => {
                        tracing::error!("Failed to deserialize frame from peer {}: {}", peer_id, e);
                        break;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Transport error from peer {}: {}", peer_id, e);
                break;
            }
        }
    }
    tracing::debug!("Peer {} transport demux task stopped", peer_id);
}
