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
