//! Shared types used across the transport layer.
use std::fmt;
use thiserror::Error;

/// Represents the verified identity of a peer in the ZZPing network.
///
/// Identity is extracted from certificates in TLS mode, or synthesized from
/// HELLO messages in raw TCP mode. The identity model uses:
/// - Common Name (CN): represents the role (collector, database, client-ro, etc.)
/// - Subject Alternative Name (SAN): first DNS entry represents username ("root" for services, actual username for users)
/// - Peer Address: network address for logging and debugging
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerTLSIdentity {
    /// The common name from the certificate (represents role).
    pub common_name: String,
    /// The first DNS entry from SAN (username or "root" for services).
    pub san_username: String,
    /// Network address of the peer for logging.
    pub peer_addr: String,
}

impl PeerTLSIdentity {
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

/// Canonical Role representation used at the zznet boundary.
///
/// The network core should remain auth-agnostic and only carry a compact
/// identifier for a peer's role. Application code (auth layer / components)
/// perform mapping from `Role` -> component-specific permission enums.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Role(String);

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

/// Errors that can occur in session/peer management
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("Peer not found: {0}")]
    /// No session exists for the requested peer id.
    PeerNotFound(PeerId),

    #[error("Peer already exists: {0}")]
    /// A peer with the same id was already registered.
    PeerAlreadyExists(PeerId),

    #[error("Room already exists: {room_id} for peer {peer_id}")]
    /// A room with the same id already exists for the peer.
    RoomAlreadyExists {
        /// The peer id where the room already exists.
        peer_id: PeerId,
        /// The conflicting room id.
        room_id: RoomId,
    },
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
    pub peer_identity: Option<PeerTLSIdentity>,
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
        identity: PeerTLSIdentity,
    },
}

// ---------------------------------------------------------------------------
// Control-plane and data-plane abstraction traits
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Role-related tests removed; role type moved to application layer.

    #[test]
    fn test_peer_identity_is_service() {
        let service = PeerTLSIdentity {
            common_name: "collector".to_string(),
            san_username: "root".to_string(),
            peer_addr: "192.168.1.100:5555".to_string(),
        };
        assert!(service.is_service());

        let user = PeerTLSIdentity {
            common_name: "client-admin".to_string(),
            san_username: "alice".to_string(),
            peer_addr: "192.168.1.101:5556".to_string(),
        };
        assert!(!user.is_service());
    }

    #[test]
    fn test_peer_identity_full_identity() {
        let service = PeerTLSIdentity {
            common_name: "collector".to_string(),
            san_username: "root".to_string(),
            peer_addr: "192.168.1.100:5555".to_string(),
        };
        assert_eq!(service.full_identity(), "collector");

        let user = PeerTLSIdentity {
            common_name: "client-admin".to_string(),
            san_username: "alice".to_string(),
            peer_addr: "192.168.1.101:5556".to_string(),
        };
        assert_eq!(user.full_identity(), "alice@client-admin");
    }
}
