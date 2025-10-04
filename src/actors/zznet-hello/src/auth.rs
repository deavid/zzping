//! Authentication and authorization for HELLO protocol.
//!
//! This module defines authentication roles and implements authorization rules
//! for determining which roles can connect to which services and access which rooms.

use serde::{Deserialize, Serialize};
use zznet_api::types::Role;

/// Authentication role for a peer in the network.
///
/// This determines what connections are allowed and what rooms can be accessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthRole {
    /// Collector role: gathers ping data from targets.
    ///
    /// Can connect to: Database
    /// Can access rooms: "memdb" (to submit ping results)
    Collector,

    /// Database role: stores and serves ping data.
    ///
    /// Can connect to: None (database is server-only)
    /// Can access rooms: "memdb" (to receive ping results), "query" (to serve queries)
    Database,

    /// Read-only client role: can query data but not modify.
    ///
    /// Can connect to: Database
    /// Can access rooms: "query" (to read data)
    ClientRo,

    /// Administrative client role: full access to all operations.
    ///
    /// Can connect to: Any service (for maintenance/debugging)
    /// Can access rooms: All rooms
    ClientAdmin,
}

impl AuthRole {
    /// Converts from zznet-api Role to AuthRole.
    ///
    /// The mapping is 1:1 for compatibility during migration.
    pub fn from_api_role(role: Role) -> Self {
        match role {
            Role::Collector => AuthRole::Collector,
            Role::Database => AuthRole::Database,
            Role::ClientRo => AuthRole::ClientRo,
            Role::ClientAdmin => AuthRole::ClientAdmin,
        }
    }

    /// Converts to zznet-api Role.
    ///
    /// The mapping is 1:1 for compatibility during migration.
    pub fn to_api_role(&self) -> Role {
        match self {
            AuthRole::Collector => Role::Collector,
            AuthRole::Database => Role::Database,
            AuthRole::ClientRo => Role::ClientRo,
            AuthRole::ClientAdmin => Role::ClientAdmin,
        }
    }

    /// Checks if this role is authorized to connect to a peer with the target role.
    ///
    /// This implements the connection authorization matrix:
    /// - Collectors connect to Database
    /// - Clients (RO and Admin) connect to Database
    /// - Admin can connect to anyone (for maintenance)
    /// - Database accepts but doesn't initiate connections
    ///
    /// Note: This is symmetric - if A can connect to B, then B can accept from A.
    pub fn can_connect_to(&self, target: &AuthRole) -> bool {
        match (self, target) {
            // Collectors connect to Database
            (AuthRole::Collector, AuthRole::Database) => true,
            // Clients connect to Database
            (AuthRole::ClientRo, AuthRole::Database) => true,
            (AuthRole::ClientAdmin, AuthRole::Database) => true,
            // Admin can connect to anyone (for maintenance/debugging)
            (AuthRole::ClientAdmin, _) => true,
            // Database accepts connections but doesn't initiate
            (AuthRole::Database, AuthRole::Collector) => true,
            (AuthRole::Database, AuthRole::ClientRo) => true,
            (AuthRole::Database, AuthRole::ClientAdmin) => true,
            // Everything else is denied
            _ => false,
        }
    }

    /// Checks if this role can access a specific room.
    ///
    /// Room access control:
    /// - Collectors can access: "memdb" (to submit data)
    /// - Database can access: "memdb", "query" (both sides of data flow)
    /// - ClientRo can access: "query" (to read data)
    /// - ClientAdmin can access: all rooms
    pub fn can_access_room(&self, room_name: &str) -> bool {
        match (self, room_name) {
            // Admin has access to all rooms
            (AuthRole::ClientAdmin, _) => true,
            // Collector can submit to memdb
            (AuthRole::Collector, "memdb") => true,
            // Database can handle both memdb and query
            (AuthRole::Database, "memdb") => true,
            (AuthRole::Database, "query") => true,
            // Read-only clients can query
            (AuthRole::ClientRo, "query") => true,
            // Everything else is denied
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_conversion_roundtrip() {
        let api_role = Role::Collector;
        let auth_role = AuthRole::from_api_role(api_role);
        let converted_back = auth_role.to_api_role();
        assert_eq!(api_role, converted_back);
    }

    #[test]
    fn test_can_connect_to_collector_to_database() {
        assert!(AuthRole::Collector.can_connect_to(&AuthRole::Database));
        assert!(!AuthRole::Collector.can_connect_to(&AuthRole::Collector));
        assert!(!AuthRole::Collector.can_connect_to(&AuthRole::ClientRo));
    }

    #[test]
    fn test_can_connect_to_database_accepts() {
        assert!(AuthRole::Database.can_connect_to(&AuthRole::Collector));
        assert!(AuthRole::Database.can_connect_to(&AuthRole::ClientRo));
        assert!(AuthRole::Database.can_connect_to(&AuthRole::ClientAdmin));
        assert!(!AuthRole::Database.can_connect_to(&AuthRole::Database));
    }

    #[test]
    fn test_can_connect_to_admin_to_all() {
        assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::Collector));
        assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::Database));
        assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::ClientRo));
        assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::ClientAdmin));
    }

    #[test]
    fn test_can_access_room_collector() {
        assert!(AuthRole::Collector.can_access_room("memdb"));
        assert!(!AuthRole::Collector.can_access_room("query"));
        assert!(!AuthRole::Collector.can_access_room("unknown"));
    }

    #[test]
    fn test_can_access_room_database() {
        assert!(AuthRole::Database.can_access_room("memdb"));
        assert!(AuthRole::Database.can_access_room("query"));
        assert!(!AuthRole::Database.can_access_room("unknown"));
    }

    #[test]
    fn test_can_access_room_client_ro() {
        assert!(!AuthRole::ClientRo.can_access_room("memdb"));
        assert!(AuthRole::ClientRo.can_access_room("query"));
        assert!(!AuthRole::ClientRo.can_access_room("unknown"));
    }

    #[test]
    fn test_can_access_room_admin() {
        assert!(AuthRole::ClientAdmin.can_access_room("memdb"));
        assert!(AuthRole::ClientAdmin.can_access_room("query"));
        assert!(AuthRole::ClientAdmin.can_access_room("unknown"));
        assert!(AuthRole::ClientAdmin.can_access_room("anything"));
    }

    #[test]
    fn test_auth_role_equality() {
        assert_eq!(AuthRole::Collector, AuthRole::Collector);
        assert_ne!(AuthRole::Collector, AuthRole::Database);
    }

    #[test]
    fn test_auth_role_copy() {
        let role = AuthRole::Collector;
        let copied = role;
        assert_eq!(role, copied);
    }
}
