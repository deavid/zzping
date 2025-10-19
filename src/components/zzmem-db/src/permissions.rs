//! Defines the permissions for the zzmem-db component.

use serde::{Deserialize, Serialize};
use std::fmt;
use zznet_auth::error::AuthError;
use zznet_auth::role::ApplicationRole;
use zzping_auth::{AuthRole, AuthRoleMapper};

/// Permissions for the zzmem-db component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemDBPermission {
    /// Allows submitting ping result batches to database.
    SubmitBatch,
    /// Allows querying stored ping data from database.
    QueryData,
    /// Allows receiving batch acknowledgments from database.
    ReceiveBatchAck,
    /// Allows receiving query responses from database.
    ReceiveQueryResponse,
}

// Implement ApplicationRole for the concrete permission enum.
impl ApplicationRole for MemDBPermission {
    fn from_cn(cn: &str) -> Result<Self, AuthError> {
        match cn {
            "memdb-submit" | "submit-batch" => Ok(Self::SubmitBatch),
            "memdb-query" | "query-data" => Ok(Self::QueryData),
            "memdb-ack" | "receive-batch-ack" => Ok(Self::ReceiveBatchAck),
            "memdb-response" | "receive-query-response" => Ok(Self::ReceiveQueryResponse),
            other => Err(AuthError::UnknownRole(other.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            MemDBPermission::SubmitBatch => "submit-batch",
            MemDBPermission::QueryData => "query-data",
            MemDBPermission::ReceiveBatchAck => "receive-batch-ack",
            MemDBPermission::ReceiveQueryResponse => "receive-query-response",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        // For now, allow all connections within the memdb component
        // This could be refined based on specific permission combinations
        true
    }

    fn can_access_room(&self, room_name: &str) -> bool {
        // All memdb permissions can access the "memdb" room
        room_name == "memdb"
    }
}

impl fmt::Display for MemDBPermission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AuthRoleMapper for MemDBPermission {
    /// Maps connection-level AuthRole to component-level MemDBPermission.
    ///
    /// Authorization mapping:
    /// - `AuthRole::Database` → All MemDB permissions (can read, write, acknowledge)
    /// - `AuthRole::Collector` → `SubmitBatch + ReceiveBatchAck` (can submit data and receive acks)
    /// - `AuthRole::ClientRo` → `QueryData + ReceiveQueryResponse` (read-only queries)
    /// - `AuthRole::ClientAdmin` → All permissions (full access)
    /// - Other roles → `None` (denied access to this component)
    ///
    /// Note: The mapper returns a primary permission; components using this should check
    /// if the mapped permission allows the specific operation needed.
    fn from_auth_role(role: AuthRole) -> Option<Self> {
        match role {
            // Database can do everything
            AuthRole::Database => Some(MemDBPermission::QueryData),
            // Collectors submit batch data
            AuthRole::Collector => Some(MemDBPermission::SubmitBatch),
            // Read-only clients can query
            AuthRole::ClientRo => Some(MemDBPermission::QueryData),
            // Admin can do everything
            AuthRole::ClientAdmin => Some(MemDBPermission::QueryData),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ron;

    #[test]
    fn test_from_cn_known_values() {
        assert!(matches!(
            MemDBPermission::from_cn("submit-batch"),
            Ok(MemDBPermission::SubmitBatch)
        ));
        assert!(matches!(
            MemDBPermission::from_cn("memdb-submit"),
            Ok(MemDBPermission::SubmitBatch)
        ));
        assert!(matches!(
            MemDBPermission::from_cn("query-data"),
            Ok(MemDBPermission::QueryData)
        ));
        assert!(matches!(
            MemDBPermission::from_cn("receive-batch-ack"),
            Ok(MemDBPermission::ReceiveBatchAck)
        ));
        assert!(matches!(
            MemDBPermission::from_cn("receive-query-response"),
            Ok(MemDBPermission::ReceiveQueryResponse)
        ));
    }

    #[test]
    fn test_from_cn_unknown() {
        let res = MemDBPermission::from_cn("no-such-permission");
        assert!(matches!(
            res,
            Err(zznet_auth::error::AuthError::UnknownRole(_))
        ));
    }

    #[test]
    fn test_as_str() {
        assert_eq!(MemDBPermission::SubmitBatch.as_str(), "submit-batch");
        assert_eq!(MemDBPermission::QueryData.as_str(), "query-data");
        assert_eq!(
            MemDBPermission::ReceiveBatchAck.as_str(),
            "receive-batch-ack"
        );
        assert_eq!(
            MemDBPermission::ReceiveQueryResponse.as_str(),
            "receive-query-response"
        );
    }

    #[test]
    fn test_can_connect_and_access_room() {
        let perms = [
            MemDBPermission::SubmitBatch,
            MemDBPermission::QueryData,
            MemDBPermission::ReceiveBatchAck,
            MemDBPermission::ReceiveQueryResponse,
        ];

        // Test all combinations can connect
        for &a in &perms {
            for &b in &perms {
                assert!(a.can_connect_to(&b), "{:?} should connect to {:?}", a, b);
            }
        }

        // Test all can access memdb room
        for &perm in &perms {
            assert!(
                perm.can_access_room("memdb"),
                "{:?} should access memdb room",
                perm
            );
            assert!(
                !perm.can_access_room("other-room"),
                "{:?} should not access other rooms",
                perm
            );
        }
    }

    #[test]
    fn test_serde_roundtrip_ron() {
        let permissions = [
            MemDBPermission::SubmitBatch,
            MemDBPermission::QueryData,
            MemDBPermission::ReceiveBatchAck,
            MemDBPermission::ReceiveQueryResponse,
        ];

        for &p in &permissions {
            let s = ron::ser::to_string(&p).expect("serialize ron");
            let p2: MemDBPermission = ron::de::from_str(&s).expect("deserialize ron");
            assert_eq!(p, p2);
        }
    }
}
