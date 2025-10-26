//! Actix message types for SessionManager actor pattern
//!
//! This module defines all message types that can be sent to a SessionManager
//! when used as an Actix actor via `Addr<SessionManager>`.
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! // Start SessionManager as an actor
//! let session_manager = SessionManager::new(offered_rooms).start();
//!
//! // Send messages to it
//! let result = session_manager.send(AddPeer {
//!     peer_id,
//!     peer_session,
//! }).await??;
//! ```

#![allow(missing_docs)]

use crate::peer_session::{PeerSession, RoomHandle};
use crate::types::{ConnectionState, PeerId, RoomId, SessionError};
use actix::prelude::*;
use tokio::sync::mpsc;
use zznet_api::types::PeerIdentity;
use zznet_api::types::Role;

// ============================================================================
// Peer Management Messages
// ============================================================================

/// Add a new peer to the session manager
#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct AddPeer {
    pub peer_id: PeerId,
    pub peer_session: PeerSession,
}

/// Connects a peer to enable message routing.
/// This sets up the necessary channels and tasks for bidirectional communication.
///
/// # Note
/// This message is NOT implemented as an actor handler due to Rust lifetime constraints.
/// Use the `connect_peer()` method directly via Arc<Mutex<SessionManager>> instead.
#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct ConnectPeer {
    pub peer_id: PeerId,
    pub outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    pub inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
}

/// Disconnect a peer (but keep peer session)
#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct DisconnectPeer {
    pub peer_id: PeerId,
}

/// Remove a peer completely
#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct RemovePeer {
    pub peer_id: PeerId,
}

/// Add a room to a peer's joined rooms
#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct AddRoomToPeer {
    pub peer_id: PeerId,
    pub room_id: RoomId,
    pub room_handle: Box<dyn RoomHandle>,
}

// ============================================================================
// Room Management Messages
// ============================================================================

/// Set the rooms offered by this SessionManager
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetOfferedRooms {
    pub rooms: Vec<RoomId>,
}

/// Handle PublishRooms negotiation
#[derive(Message)]
#[rtype(result = "Result<Vec<RoomId>, SessionError>")]
pub struct HandlePublishRooms {
    pub peer_id: PeerId,
    pub requested_rooms: Vec<RoomId>,
}

// ============================================================================
// Query Messages
// ============================================================================

/// Get the connection state of a peer
#[derive(Message)]
#[rtype(result = "Option<ConnectionState>")]
pub struct GetPeerState {
    pub peer_id: PeerId,
}

/// Check if a peer is connected
#[derive(Message)]
#[rtype(result = "bool")]
pub struct IsPeerConnected {
    pub peer_id: PeerId,
}

/// Get all peer IDs
#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeerIds;

/// Get the role of a specific peer (returns canonical `Role`)
#[derive(Message)]
#[rtype(result = "Option<Role>")]
pub struct GetPeerRole {
    pub peer_id: PeerId,
}

impl GetPeerRole {
    pub fn new(peer_id: PeerId) -> Self {
        Self { peer_id }
    }
}

/// Get the identity of a specific peer
#[derive(Message)]
#[rtype(result = "Option<PeerIdentity>")]
pub struct GetPeerIdentity {
    pub peer_id: PeerId,
}

/// Get all peers with a specific canonical `Role`
#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeersWithRole {
    pub role: Role,
}

/// Get count of connected peers
#[derive(Message)]
#[rtype(result = "usize")]
pub struct GetConnectedPeerCount;

/// Get the rooms offered by this SessionManager
#[derive(Message)]
#[rtype(result = "Vec<RoomId>")]
pub struct GetOfferedRooms;

/// Get the sender channel for a peer
#[derive(Message)]
#[rtype(result = "Option<mpsc::Sender<(RoomId, Vec<u8>)>>")]
pub struct GetPeerSender {
    pub peer_id: PeerId,
}

/// Subscribe to a peer's inbound messages
#[derive(Message)]
#[rtype(result = "Option<tokio::sync::broadcast::Receiver<(RoomId, Vec<u8>)>>")]
pub struct SubscribePeerInbound {
    pub peer_id: PeerId,
}

/// Get all rooms a peer has joined
#[derive(Message)]
#[rtype(result = "Result<Vec<RoomId>, SessionError>")]
pub struct GetPeerJoinedRooms {
    pub peer_id: PeerId,
}

/// Check if a room is joined with a specific peer
#[derive(Message)]
#[rtype(result = "Result<bool, SessionError>")]
pub struct IsRoomJoinedWithPeer {
    pub peer_id: PeerId,
    pub room_id: RoomId,
}

// ============================================================================
// Message Sending
// ============================================================================

/// Send a message to a room for a specific peer
#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct SendToRoom {
    pub peer_id: PeerId,
    pub room_id: RoomId,
    pub bytes: Vec<u8>,
}

// ============================================================================
// Optimized Batch Messages (Future Extension)
// ============================================================================

/// Broadcast a message to all peers with a specific canonical `Role`
/// This is an optimization that avoids multiple round-trips
#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct BroadcastToRole {
    pub role: Role,
    pub room_id: RoomId,
    pub bytes: Vec<u8>,
}
