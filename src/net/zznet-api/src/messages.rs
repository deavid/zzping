//! Actix mesages for zznet

use crate::types::{PeerId, Role, RoomId, TransportFrame};
use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Carries raw bytes received from a room.
#[derive(Message)]
#[rtype(result = "()")]
pub struct InboundRoomPayload {
    /// The serialized message bytes.
    pub payload: Vec<u8>,
}

/// Alias for the recipient type used by Router ↔ RoomActor wiring.
pub type RoomInboundRecipient = Recipient<InboundRoomPayload>;

/// Signals a successful handshake and handover to the Router.
#[derive(Message)]
#[rtype(result = "Result<HashMap<RoomId, RoomInboundRecipient>, String>")]
pub struct OnPeerConnected {
    /// The ID of the connected peer
    pub peer_id: PeerId,
    /// The role of the peer
    pub role: Role,
    /// The list of rooms successfully negotiated with the peer
    pub negotiated_rooms: Vec<RoomId>,
    /// Direct transport write handle - writes raw frames to network
    pub transport_tx: mpsc::Sender<TransportFrame>,
}
