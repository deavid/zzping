//! Shared types used across the transport layer.

use serde::{Deserialize, Serialize};

/// Represents the role of a participant in the ZZPing network protocol.
///
/// Roles are used for:
/// - Certificate selection (each role has its own cert/key pair)
/// - Authorization (determining which roles can connect to which)
/// - Logging and metrics (identifying connection types)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Role {
    /// Collector role: gathers ping data from targets.
    Collector,

    /// Database role: stores and serves ping data.
    Database,

    /// Read-only client role: can query data but not modify.
    ClientRo,

    /// Administrative client role: full access to all operations.
    ClientAdmin,
}

impl Role {
    /// Returns the conventional certificate filename for this role.
    pub fn cert_name(&self) -> &'static str {
        match self {
            Role::Collector => "collector",
            Role::Database => "database",
            Role::ClientRo => "client-ro",
            Role::ClientAdmin => "client-admin",
        }
    }

    /// Checks if this role is authorized to connect to another role.
    pub fn can_connect_to(&self, target: Role) -> bool {
        match self {
            Role::ClientAdmin => true,
            Role::Collector | Role::ClientRo => matches!(target, Role::Database),
            Role::Database => false,
        }
    }
}

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

    #[test]
    fn test_cert_name() {
        assert_eq!(Role::Collector.cert_name(), "collector");
        assert_eq!(Role::Database.cert_name(), "database");
        assert_eq!(Role::ClientRo.cert_name(), "client-ro");
        assert_eq!(Role::ClientAdmin.cert_name(), "client-admin");
    }

    #[test]
    fn test_authorization_collector_to_database() {
        assert!(Role::Collector.can_connect_to(Role::Database));
        assert!(!Role::Collector.can_connect_to(Role::Collector));
        assert!(!Role::Collector.can_connect_to(Role::ClientRo));
        assert!(!Role::Collector.can_connect_to(Role::ClientAdmin));
    }

    #[test]
    fn test_authorization_clients_to_database() {
        assert!(Role::ClientRo.can_connect_to(Role::Database));
        assert!(Role::ClientAdmin.can_connect_to(Role::Database));
    }

    #[test]
    fn test_authorization_admin_to_all() {
        assert!(Role::ClientAdmin.can_connect_to(Role::Collector));
        assert!(Role::ClientAdmin.can_connect_to(Role::Database));
        assert!(Role::ClientAdmin.can_connect_to(Role::ClientRo));
        assert!(Role::ClientAdmin.can_connect_to(Role::ClientAdmin));
    }

    #[test]
    fn test_role_equality() {
        assert_eq!(Role::Collector, Role::Collector);
        assert_ne!(Role::Collector, Role::Database);
    }

    #[test]
    fn test_role_clone() {
        let role = Role::Collector;
        let cloned = role;
        assert_eq!(role, cloned);
    }

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
