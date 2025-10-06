use std::fmt;

/// Unique identifier for a peer
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeerId(String);

impl PeerId {
    /// Create a new PeerId from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for PeerId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a room
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RoomId(String);

impl RoomId {
    /// Create a new RoomId from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RoomId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for RoomId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Connection state for a peer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Peer exists but is not connected
    Disconnected,
    /// Peer is connected and active
    Connected,
}

/// Errors that can occur in session management
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Peer not found: {0}")]
    /// No session exists for the requested peer id.
    PeerNotFound(PeerId),

    #[error("Peer already exists: {0}")]
    /// A peer with the same id was already registered.
    PeerAlreadyExists(PeerId),

    #[error("Peer already connected: {0}")]
    /// Peer is already connected.
    PeerAlreadyConnected(PeerId),

    #[error("Peer not connected: {0}")]
    /// Peer exists but is not currently connected.
    PeerNotConnected(PeerId),

    #[error("Room not found: {room_id} for peer {peer_id}")]
    /// The requested room was not found for the peer.
    RoomNotFound {
        /// The peer id for which the room was looked up.
        peer_id: PeerId,
        /// The room id that was not found for the peer.
        room_id: RoomId,
    },

    #[error("Room already exists: {room_id} for peer {peer_id}")]
    /// A room with the same id already exists for the peer.
    RoomAlreadyExists {
        /// The peer id where the room already exists.
        peer_id: PeerId,
        /// The conflicting room id.
        room_id: RoomId,
    },

    #[error("Room handler not registered: {room_id}")]
    /// No handler was registered for the room.
    RoomHandlerNotRegistered {
        /// The room id lacking a registered handler.
        room_id: RoomId,
    },

    #[error("Room receiver already spawned for {room_id} on peer {peer_id}")]
    /// The receiver task for the room is already running.
    RoomReceiverAlreadySpawned {
        /// The peer id on which the receiver was spawned.
        peer_id: PeerId,
        /// The room id whose receiver is already running.
        room_id: RoomId,
    },

    #[error("Failed to send message")]
    /// Failed due to an underlying channel/send error.
    SendFailed,

    #[error("Wrong message type for room (failed conversion)")]
    /// The message could not be converted to the room's expected type.
    WrongMessageType,

    #[error("Room not joined: {0}")]
    /// The requested room is not joined by the peer.
    RoomNotJoined(RoomId),

    #[error("Empty room intersection: no common rooms between local and peer")]
    /// There are no common rooms between local and peer to communicate.
    EmptyIntersection,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_id_new() {
        let peer_id = PeerId::new("test_peer");
        assert_eq!(peer_id.as_str(), "test_peer");
    }

    #[test]
    fn test_peer_id_as_str() {
        let peer_id = PeerId::new("test_peer");
        assert_eq!(peer_id.as_str(), "test_peer");
    }

    #[test]
    fn test_peer_id_display() {
        let peer_id = PeerId::new("test_peer");
        assert_eq!(format!("{}", peer_id), "test_peer");
    }

    #[test]
    fn test_peer_id_from_str() {
        let peer_id: PeerId = "test_peer".into();
        assert_eq!(peer_id.as_str(), "test_peer");
    }

    #[test]
    fn test_peer_id_equality() {
        let peer_id1 = PeerId::new("test");
        let peer_id2 = PeerId::new("test");
        let peer_id3 = PeerId::new("different");

        assert_eq!(peer_id1, peer_id2);
        assert_ne!(peer_id1, peer_id3);
    }

    #[test]
    fn test_room_id_new() {
        let room_id = RoomId::new("test_room");
        assert_eq!(room_id.as_str(), "test_room");
    }

    #[test]
    fn test_room_id_as_str() {
        let room_id = RoomId::new("test_room");
        assert_eq!(room_id.as_str(), "test_room");
    }

    #[test]
    fn test_room_id_display() {
        let room_id = RoomId::new("test_room");
        assert_eq!(format!("{}", room_id), "test_room");
    }

    #[test]
    fn test_room_id_from_str() {
        let room_id: RoomId = "test_room".into();
        assert_eq!(room_id.as_str(), "test_room");
    }

    #[test]
    fn test_room_id_equality() {
        let room_id1 = RoomId::new("test");
        let room_id2 = RoomId::new("test");
        let room_id3 = RoomId::new("different");

        assert_eq!(room_id1, room_id2);
        assert_ne!(room_id1, room_id3);
    }

    #[test]
    fn test_connection_state() {
        assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
        assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
        assert_ne!(ConnectionState::Disconnected, ConnectionState::Connected);
    }
}
