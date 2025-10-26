// Permissions for the `zzcollector-state` component.
// Minimal concrete ApplicationRole implementation used for examples/tests.

use serde::{Deserialize, Serialize};
use std::fmt;
use zznet_auth::error::AuthError;
use zznet_auth::role::ApplicationRole;
use zzping_auth::{AuthRole, AuthRoleMapper};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// Concrete permission role used for examples and tests.
#[deprecated(note = "components CANNOT define roles")]
pub enum CStatePermission {
    /// Administrative role with full access.
    Admin,
    /// Database role that tracks collectors.
    Database,
    /// Collector role that sends heartbeats.
    Collector,
}

impl ApplicationRole for CStatePermission {
    fn from_cn(cn: &str) -> Result<Self, AuthError> {
        match cn {
            "admin" => Ok(CStatePermission::Admin),
            "database" => Ok(CStatePermission::Database),
            "collector" => Ok(CStatePermission::Collector),
            other => Err(AuthError::UnknownRole(other.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            CStatePermission::Admin => "admin",
            CStatePermission::Database => "database",
            CStatePermission::Collector => "collector",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        true
    }

    fn can_access_room(&self, room_name: &str) -> bool {
        room_name == "cstate"
    }
}

impl fmt::Display for CStatePermission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AuthRoleMapper for CStatePermission {
    /// Maps connection-level AuthRole to component-level CStatePermission.
    ///
    /// Authorization mapping:
    /// - `AuthRole::Database` → `CStatePermission::Database`
    /// - `AuthRole::Collector` → `CStatePermission::Collector`
    /// - `AuthRole::ClientRo` → `CStatePermission::Collector` (read-only access to collector data)
    /// - `AuthRole::ClientAdmin` → `CStatePermission::Admin` (full access)
    fn from_auth_role(role: AuthRole) -> Option<Self> {
        match role {
            AuthRole::Database => Some(CStatePermission::Database),
            AuthRole::Collector => Some(CStatePermission::Collector),
            AuthRole::ClientRo => Some(CStatePermission::Collector),
            AuthRole::ClientAdmin => Some(CStatePermission::Admin),
        }
    }
}
