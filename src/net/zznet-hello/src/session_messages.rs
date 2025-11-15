//! Defines messages for communication between the `HelloActor` and the `ConnectionManager`.

use actix::prelude::*;

/// Sent by `HelloActor` to `ConnectionManager` after a successful handshake.
///
/// This message signals that a new peer has been authenticated and is ready to be
/// integrated into the session layer.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub(crate) struct HandshakeComplete {
    /// The unique identifier for the peer.
    pub peer_id: String,
    /// The peer's role, as determined by the HELLO handshake.
    pub peer_role_str: String,
    /// The list of rooms negotiated for this session.
    pub active_rooms: Vec<String>,
    /// The address of the `HelloActor` managing this connection.
    pub hello_actor: Addr<super::actor::HelloActor>,
}
