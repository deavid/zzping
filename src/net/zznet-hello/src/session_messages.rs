//! Messages for HelloActor to communicate with SessionManager.

use actix::prelude::*;

use crate::error::HelloError;

/// Message sent TO HelloActor FROM SessionManager to send a room message.
///
/// After the HELLO handshake completes, SessionManager uses this message
/// to route application messages through the transport.
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), HelloError>")]
pub struct SendRoomMessage {
    /// Source room name.
    pub from_room: String,
    /// Destination room name.
    pub to_room: String,
    /// Serialized message payload.
    pub payload: Vec<u8>,
}

/// Message sent FROM HelloActor TO SessionManager after successful handshake.
///
/// This notifies SessionManager that a new authenticated peer is ready
/// and provides the HelloActor address for sending messages back.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct HandshakeComplete {
    /// Unique identifier for the peer.
    pub peer_id: String,
    /// Peer's authentication role as a string (from HELLO handshake).
    pub peer_role_str: String,
    /// Rooms negotiated during handshake (intersection of offered rooms).
    pub active_rooms: Vec<String>,
    /// Address of this HelloActor for sending messages.
    pub hello_actor: Addr<super::actor::HelloActor>,
}

/// Message sent FROM HelloActor TO SessionManager when connection is lost.
///
/// This notifies SessionManager to clean up state for this peer.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct ConnectionLost {
    /// Reason for disconnection.
    pub reason: String,
}

/// Message received BY HelloActor FROM SessionManager with room message.
///
/// This is the inbound direction: application → SessionManager → HelloActor → transport.
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), HelloError>")]
pub struct InboundRoomMessage {
    /// Source room name.
    pub from_room: String,
    /// Destination room name.
    pub to_room: String,
    /// Serialized payload.
    pub payload: Vec<u8>,
}
