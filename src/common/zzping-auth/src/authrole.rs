//! Specific roles for zzping applications.

use serde::{Deserialize, Serialize};
use std::fmt;
use zznet_auth::role::ApplicationRole;

/// Authentication role for a peer in the ZZPing network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthRole {
    /// Role representing a collector service.
    Collector,
    /// Role representing the database service.
    Database,
    /// Read-only client role.
    ClientRo,
    /// Administrator client role with full privileges.
    ClientAdmin,
}

impl ApplicationRole for AuthRole {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "collector" => Ok(AuthRole::Collector),
            "database" => Ok(AuthRole::Database),
            "client-ro" => Ok(AuthRole::ClientRo),
            "client-admin" => Ok(AuthRole::ClientAdmin),
            _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            AuthRole::Collector => "collector",
            AuthRole::Database => "database",
            AuthRole::ClientRo => "client-ro",
            AuthRole::ClientAdmin => "client-admin",
        }
    }

    fn can_connect_to(&self, target: &AuthRole) -> bool {
        match self {
            AuthRole::ClientAdmin => true,
            AuthRole::Collector | AuthRole::ClientRo => matches!(target, AuthRole::Database),
            AuthRole::Database => matches!(
                target,
                AuthRole::Collector | AuthRole::ClientRo | AuthRole::ClientAdmin
            ),
        }
    }

    fn can_access_room(&self, room_name: &str) -> bool {
        if matches!(self, AuthRole::ClientAdmin) {
            return true;
        }

        match self {
            AuthRole::Collector => room_name == "memdb",
            AuthRole::Database => room_name == "memdb" || room_name == "query",
            AuthRole::ClientRo => room_name == "query",
            AuthRole::ClientAdmin => true, // handled above but keep exhaustive match
        }
    }
}

impl fmt::Display for AuthRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
