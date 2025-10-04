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
    ///
    /// Collectors connect to the Database to submit results.
    Collector,

    /// Database role: stores and serves ping data.
    ///
    /// The Database accepts connections from Collectors and Clients.
    Database,

    /// Read-only client role: can query data but not modify.
    ///
    /// Read-only clients can connect to the Database to query historical data.
    ClientRo,

    /// Administrative client role: full access to all operations.
    ///
    /// Admin clients can connect to any service and perform all operations.
    ClientAdmin,
}

impl Role {
    /// Returns the conventional certificate filename for this role.
    ///
    /// Used to construct certificate paths like `certs/collector.pem`.
    pub fn cert_name(&self) -> &'static str {
        match self {
            Role::Collector => "collector",
            Role::Database => "database",
            Role::ClientRo => "client-ro",
            Role::ClientAdmin => "client-admin",
        }
    }

    /// Checks if this role is authorized to connect to another role.
    ///
    /// Authorization rules:
    /// - Collectors can connect to Database
    /// - Clients (both RO and Admin) can connect to Database
    /// - Admin clients can connect to anyone (for maintenance)
    /// - Database accepts connections from Collectors and Clients
    pub fn can_connect_to(&self, target: Role) -> bool {
        match (self, target) {
            // Collectors connect to Database
            (Role::Collector, Role::Database) => true,
            // Clients connect to Database
            (Role::ClientRo, Role::Database) => true,
            (Role::ClientAdmin, Role::Database) => true,
            // Admin can connect to anyone (for maintenance/debugging)
            (Role::ClientAdmin, _) => true,
            // Everything else is denied
            _ => false,
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
}
