//! Actix mesages for zznet

use actix::prelude::*;
use tokio::sync::mpsc;
use crate::types::{PeerId, Role, RoomId};

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
    /// Sender for outbound messages to the peer
    pub outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    /// Receiver for inbound messages from the peer
    pub inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
}
