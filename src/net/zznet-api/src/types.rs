//! Shared types used across the transport layer.
use bytes::Bytes;
use std::fmt;

/// A serialized protocol frame ready for transport.
///
/// This represents a discrete, atomic unit of data. The transport layer
/// guarantees that this entire block will be framed (e.g. with a length prefix)
/// and delivered as a single unit to the peer.
///
/// Unlike `Bytes`, this type semantically represents a complete protocol frame,
/// not a stream or arbitrary blob. This clarity helps prevent misuse and
/// enables future extensions (e.g., priority, compression flags) without
/// breaking component signatures.
#[derive(Clone, Debug)]
pub struct TransportFrame(Bytes);

impl TransportFrame {
    /// Wrap raw bytes into a transport frame.
    pub fn new(data: Vec<u8>) -> Self {
        Self(Bytes::from(data))
    }

    /// Access the inner bytes (used by the Transport layer only).
    pub fn get_bytes(&self) -> &Bytes {
        &self.0
    }

    /// Consume the frame into bytes (used by the Transport layer).
    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

// Allow cheap conversion from Bytes if needed internally
impl From<Bytes> for TransportFrame {
    fn from(b: Bytes) -> Self {
        Self(b)
    }
}

/// Represents the verified identity of a peer
///
/// Identity is extracted from X.509 certificates using the Directory Model:
/// - OU (OrganizationalUnit): role
/// - CN (CommonName): username ("root" for services, actual username for users)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerTLSIdentity {
    /// The role from the certificate's OU field (OrganizationalUnit).
    pub role: String,
    /// The username from the certificate's CN field (CommonName).
    pub username: String,
}

impl PeerTLSIdentity {
    /// Returns true if this identity represents a service (not a user).
    ///
    /// Services have "root" as their username.
    pub fn is_service(&self) -> bool {
        self.username == "root"
    }

    /// Returns the full identity string for logging and authorization.
    ///
    /// Format: "username@role" for users, "role" for services.
    pub fn full_identity(&self) -> String {
        if self.is_service() {
            self.role.clone()
        } else {
            format!("{}@{}", self.username, self.role)
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
            role: "collector".to_string(),
            username: "root".to_string(),
        };
        assert!(service.is_service());

        let user = PeerTLSIdentity {
            role: "client-admin".to_string(),
            username: "alice".to_string(),
        };
        assert!(!user.is_service());
    }

    #[test]
    fn test_peer_identity_full_identity() {
        let service = PeerTLSIdentity {
            role: "collector".to_string(),
            username: "root".to_string(),
        };
        assert_eq!(service.full_identity(), "collector");

        let user = PeerTLSIdentity {
            role: "client-admin".to_string(),
            username: "alice".to_string(),
        };
        assert_eq!(user.full_identity(), "alice@client-admin");
    }
}
