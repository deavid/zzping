//! Actix mesages for zznet

use crate::error::TransportError;
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

/// Carries a raw, authenticated connection from Transport to Session layer.
#[derive(Message)]
#[rtype(result = "()")]
pub struct AcceptTransport {
    /// Sender for outbound frames to the peer.
    pub tx: mpsc::Sender<TransportFrame>,
    /// Receiver for inbound frames from the peer.
    pub rx: mpsc::Receiver<Result<TransportFrame, TransportError>>,
    /// Peer address for logging/metrics.
    pub peer_addr: String,
    /// Optional TLS-verified peer identity.
    pub peer_identity: Option<crate::types::PeerTLSIdentity>,
}
