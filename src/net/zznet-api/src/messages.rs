//! Actix mesages for zznet

use crate::error::TransportError;
use crate::types::{PeerId, Role, RoomId};
use actix::prelude::*;
use bytes::Bytes;
use tokio::sync::mpsc;

/// Handle peer connected event from PeerManager
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct OnPeerConnected {
    /// The ID of the connected peer
    pub peer_id: PeerId,
    /// The role of the peer
    pub role: Role,
    /// The list of rooms successfully negotiated with the peer
    pub negotiated_rooms: Vec<RoomId>,
    /// Direct transport write handle - writes raw frames to network
    pub transport_tx: mpsc::Sender<Bytes>,
    /// Direct transport read handle - reads raw frames from network
    pub transport_rx: mpsc::Receiver<Result<Bytes, TransportError>>,
}
