//! ZZPing-specific authentication roles and authorization.
//!
//! This crate provides ZZPing's implementation of the `ApplicationRole` trait
//! from `zznet-auth`, along with role-specific authorization logic.

use serde::{Deserialize, Serialize};
use std::fmt;
use zznet_auth::ApplicationRole;

/// Authentication role for a peer in the ZZPing network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl AuthRole {
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
}

// Re-export common types for convenience
pub use zznet_auth::{AclManager, GenericAuthorizer};

// Re-export zznet-auth modules for convenience in application tests and examples
pub use zznet_auth::config;
pub use zznet_auth::error;

// Type alias for the authorizer closure used by ConnectionManager (ZZPing-specific)
/// Type alias for the authorizer specialized to this application's `AuthRole`.
///
/// The generic `GenericAuthorizer` from `zznet-auth` is bound to the concrete
/// `AuthRole` defined in this crate for convenience in application code and
/// tests.
pub type Authorizer = GenericAuthorizer<AuthRole>;

/// Type alias for the default ACL manager using this crate's `AuthRole`.
///
/// This exposes `AclManager<AuthRole>` under a short name for callers that
/// don't need to reference the generic form.
pub type AclManagerDefault = AclManager<AuthRole>;
