//! Shared types used across the transport layer.
use std::fmt;

/// Represents the verified identity of a peer in the ZZPing network.
///
/// Identity is extracted from certificates in TLS mode, or synthesized from
/// HELLO messages in raw TCP mode. The identity model uses:
/// - Common Name (CN): represents the role (collector, database, client-ro, etc.)
/// - Subject Alternative Name (SAN): first DNS entry represents username ("root" for services, actual username for users)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerTLSIdentity {
    /// The common name from the certificate (represents role).
    pub common_name: String,
    /// The first DNS entry from SAN (username or "root" for services).
    pub san_username: String,
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
        };
        assert!(service.is_service());

        let user = PeerTLSIdentity {
            common_name: "client-admin".to_string(),
            san_username: "alice".to_string(),
        };
        assert!(!user.is_service());
    }

    #[test]
    fn test_peer_identity_full_identity() {
        let service = PeerTLSIdentity {
            common_name: "collector".to_string(),
            san_username: "root".to_string(),
        };
        assert_eq!(service.full_identity(), "collector");

        let user = PeerTLSIdentity {
            common_name: "client-admin".to_string(),
            san_username: "alice".to_string(),
        };
        assert_eq!(user.full_identity(), "alice@client-admin");
    }
}
