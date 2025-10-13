// Permissions for the `zzcollector-state` component.
// Minimal concrete ApplicationRole implementation used for examples/tests.

use serde::{Deserialize, Serialize};
use zznet_auth::error::AuthError;
use zznet_auth::role::ApplicationRole;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// Concrete permission role used for examples and tests.
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
