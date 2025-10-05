//! Application-specific AuthRole for zzping
//!
//! This enum represents the application-level roles (Collector, Database, ClientRo, ClientAdmin)
//! and provides utilities to map from certificate CN values.

use serde::{Deserialize, Serialize};

/// Authentication role for a peer in the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthRole {
    Collector,
    Database,
    ClientRo,
    ClientAdmin,
}

impl AuthRole {
    /// Creates an AuthRole from a certificate Common Name (CN).
    pub fn from_cn(cn: &str) -> Result<Self, crate::error::AuthError> {
        match cn {
            "collector" => Ok(AuthRole::Collector),
            "database" => Ok(AuthRole::Database),
            "client-ro" => Ok(AuthRole::ClientRo),
            "client-admin" => Ok(AuthRole::ClientAdmin),
            _ => Err(crate::error::AuthError::UnknownRole(cn.to_string())),
        }
    }

    /// Convert to string name used in certificates.
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthRole::Collector => "collector",
            AuthRole::Database => "database",
            AuthRole::ClientRo => "client-ro",
            AuthRole::ClientAdmin => "client-admin",
        }
    }

    /// Converts from zznet-api Role to AuthRole (compat shim during migration).
    pub fn from_api_role(role: zznet_api::types::Role) -> Self {
        match role {
            zznet_api::types::Role::Collector => AuthRole::Collector,
            zznet_api::types::Role::Database => AuthRole::Database,
            zznet_api::types::Role::ClientRo => AuthRole::ClientRo,
            zznet_api::types::Role::ClientAdmin => AuthRole::ClientAdmin,
        }
    }

    /// Converts to zznet-api Role (compat shim during migration).
    pub fn to_api_role(&self) -> zznet_api::types::Role {
        match self {
            AuthRole::Collector => zznet_api::types::Role::Collector,
            AuthRole::Database => zznet_api::types::Role::Database,
            AuthRole::ClientRo => zznet_api::types::Role::ClientRo,
            AuthRole::ClientAdmin => zznet_api::types::Role::ClientAdmin,
        }
    }

    /// Check if this role is authorized to connect to a peer with the target role.
    pub fn can_connect_to(&self, target: &AuthRole) -> bool {
        match self {
            AuthRole::ClientAdmin => true,
            AuthRole::Collector | AuthRole::ClientRo => matches!(target, AuthRole::Database),
            AuthRole::Database => matches!(
                target,
                AuthRole::Collector | AuthRole::ClientRo | AuthRole::ClientAdmin
            ),
        }
    }

    /// Check if this role can access a specific room.
    pub fn can_access_room(&self, room_name: &str) -> bool {
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
