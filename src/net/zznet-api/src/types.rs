//! Shared types used across the transport layer.

// NOTE: The concrete role enum was removed from zznet-api to keep the API
// auth-agnostic. Applications provide their own role types by implementing
// zznet_auth::ApplicationRole. Transport and session layers refer to roles
// via trait bounds or strings (e.g., certificate CN).

/// Represents the verified identity of a peer in the ZZPing network.
///
/// Identity is extracted from certificates in TLS mode, or synthesized from
/// HELLO messages in raw TCP mode. The identity model uses:
/// - Common Name (CN): represents the role (collector, database, client-ro, etc.)
/// - Subject Alternative Name (SAN): first DNS entry represents username ("root" for services, actual username for users)
/// - Peer Address: network address for logging and debugging
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    /// The common name from the certificate (represents role).
    pub common_name: String,
    /// The first DNS entry from SAN (username or "root" for services).
    pub san_username: String,
    /// Network address of the peer for logging.
    pub peer_addr: String,
}

/// Canonical Role representation used at the zznet boundary.
///
/// The network core should remain auth-agnostic and only carry a compact
/// identifier for a peer's role. Application code (auth layer / components)
/// perform mapping from `Role` -> component-specific permission enums.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Role(pub String);

impl Role {
    /// Create a Role from &str
    pub fn new(s: &str) -> Self {
        Self(s.to_string())
    }

    /// Borrow the role as &str
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Role {
    fn from(s: &str) -> Self {
        Role::new(s)
    }
}

impl From<String> for Role {
    fn from(s: String) -> Self {
        Role(s)
    }
}

impl PeerIdentity {
    /// Returns true if this identity represents a service (not a user).
    ///
    /// Services have "root" as their SAN username.
    pub fn is_service(&self) -> bool {
        self.san_username == "root"
    }

    /// Returns the full identity string for logging and authorization.
    ///
    /// Format: "username@role" for users, "role" for services.
    pub fn full_identity(&self) -> String {
        if self.is_service() {
            self.common_name.clone()
        } else {
            format!("{}@{}", self.san_username, self.common_name)
        }
    }
}

// ---------------------------------------------------------------------------
// Canonical network primitives (migrated from zznet-session::types)
// These are intentionally placed in `zznet-api` so all crates depend on
// a stable, minimal set of shared types instead of the old `zznet-session` crate.
// ---------------------------------------------------------------------------

use async_trait::async_trait;
use std::fmt;
use thiserror::Error;
use tokio::sync::{broadcast, mpsc};

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

/// Errors that can occur in session/peer management
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("Peer not found: {0}")]
    /// No session exists for the requested peer id.
    PeerNotFound(PeerId),

    #[error("Peer already exists: {0}")]
    /// A peer with the same id was already registered.
    PeerAlreadyExists(PeerId),

    #[error("Peer limit exceeded: max {max}")]
    /// The configured maximum number of peers has been reached.
    PeerLimitExceeded {
        /// Maximum allowed peers.
        max: usize,
    },

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

    #[error("Too many rooms for peer {peer_id}: max {max}")]
    /// The peer has more rooms than the configured per-peer limit.
    RoomLimitExceeded {
        /// The peer with too many rooms.
        peer_id: PeerId,
        /// Maximum allowed rooms per peer.
        max: usize,
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

/// Authentication context passed to the authorizer.
///
/// This contains both the HELLO protocol role (primary source) and optional
/// TLS identity (for validation). The authorizer should:
/// 1. Parse the HELLO role (always required - this is the source of truth)
/// 2. If TLS identity exists, validate HELLO role matches certificate CN
/// 3. If no TLS, check insecure mode flag before trusting HELLO
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Role string from HELLO message - PRIMARY source of identity
    pub hello_role_str: String,
    /// Optional TLS peer identity for validation (None for plain TCP)
    pub peer_identity: Option<PeerIdentity>,
}

/// Framework-level peer lifecycle events published by the control plane.
///
/// These events are emitted by `zznet-peer-manager` whenever peers are added,
/// connected, disconnected, removed, or when their identity information is
/// refreshed after authentication. Components subscribe to this event stream
/// to drive their network managers without depending on the concrete
/// implementation crate.
#[derive(Debug, Clone)]
pub enum PeerLifecycleEvent {
    /// A peer was registered with the manager (not connected yet).
    PeerAdded {
        /// Identifier of the peer that was registered.
        peer_id: PeerId,
    },

    /// A peer transitioned to the connected state.
    PeerConnected {
        /// Identifier of the peer that connected.
        peer_id: PeerId,
    },

    /// A peer transitioned out of the connected state.
    PeerDisconnected {
        /// Identifier of the peer that disconnected.
        peer_id: PeerId,
    },

    /// A peer was completely removed from the manager.
    PeerRemoved {
        /// Identifier of the peer that was removed.
        peer_id: PeerId,
    },

    /// A peer's authenticated identity information was updated.
    PeerIdentityUpdated {
        /// Identifier of the peer whose identity changed.
        peer_id: PeerId,
        /// The refreshed identity information for the peer.
        identity: PeerIdentity,
    },
}

// ---------------------------------------------------------------------------
// Control-plane and data-plane abstraction traits
// ---------------------------------------------------------------------------

/// Read-only view of control-plane state associated with a peer.
pub trait PeerStateView: Send + Sync {
    /// Unique identifier for the peer.
    fn peer_id(&self) -> &PeerId;

    /// Connection lifecycle state for the peer.
    fn connection_state(&self) -> ConnectionState;

    /// Authenticated role, if authorization completed.
    fn role(&self) -> Option<&Role>;

    /// Authenticated identity, if TLS validation completed.
    fn identity(&self) -> Option<&PeerIdentity>;
}

/// Mutable access to control-plane state for a peer.
pub trait PeerStateMut: PeerStateView {
    /// Update connection lifecycle state.
    fn set_connection_state(&mut self, state: ConnectionState);

    /// Update the authenticated role (or clear when unknown).
    fn set_role(&mut self, role: Option<Role>);

    /// Update the authenticated identity (or clear when unknown).
    fn set_identity(&mut self, identity: Option<PeerIdentity>);
}

/// Data-plane capabilities required for routing bytes to a peer.
#[async_trait]
pub trait PeerChannels: Send + Sync {
    /// Identifier for this peer.
    fn peer_id(&self) -> &PeerId;

    /// Rooms successfully negotiated with this peer.
    fn joined_rooms(&self) -> &[RoomId];

    /// Returns true when the given room is joined with the peer.
    fn is_room_joined(&self, room_id: &RoomId) -> bool;

    /// Clone of the outbound sender for direct byte transmission.
    fn outbound_sender(&self) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>>;

    /// Subscribe to raw inbound messages from the peer.
    fn subscribe_inbound(&self) -> Option<broadcast::Receiver<(RoomId, Vec<u8>)>>;

    /// Send raw bytes to a specific room for this peer.
    async fn send_to_room(&self, room_id: &RoomId, bytes: Vec<u8>) -> Result<(), SessionError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Role-related tests removed; role type moved to application layer.

    #[test]
    fn test_peer_identity_is_service() {
        let service = PeerIdentity {
            common_name: "collector".to_string(),
            san_username: "root".to_string(),
            peer_addr: "192.168.1.100:5555".to_string(),
        };
        assert!(service.is_service());

        let user = PeerIdentity {
            common_name: "client-admin".to_string(),
            san_username: "alice".to_string(),
            peer_addr: "192.168.1.101:5556".to_string(),
        };
        assert!(!user.is_service());
    }

    #[test]
    fn test_peer_identity_full_identity() {
        let service = PeerIdentity {
            common_name: "collector".to_string(),
            san_username: "root".to_string(),
            peer_addr: "192.168.1.100:5555".to_string(),
        };
        assert_eq!(service.full_identity(), "collector");

        let user = PeerIdentity {
            common_name: "client-admin".to_string(),
            san_username: "alice".to_string(),
            peer_addr: "192.168.1.101:5556".to_string(),
        };
        assert_eq!(user.full_identity(), "alice@client-admin");
    }
}
