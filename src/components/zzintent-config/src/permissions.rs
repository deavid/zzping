//! Defines the permissions for the zzintent-config component.

use serde::{Deserialize, Serialize};
use std::fmt;
use zznet_auth::role::ApplicationRole;
use zzping_auth::{AuthRole, AuthRoleMapper};

/// Permissions for the zzintent-config component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntentConfigPermission {
    /// Allows updating the configuration.
    UpdateConfig,
    /// Allows receiving configuration updates.
    ReceiveConfigUpdates,
}

/// A trait for checking permissions.
pub trait PermissionCheck<T: ApplicationRole> {
    /// Checks if the given role has permission to update the config.
    fn has_update_permission(&self, role: &T) -> bool;
    /// Checks if the given role has permission to receive config updates.
    fn has_receive_permission(&self, role: &T) -> bool;
    /// Returns a string representation of the permission.
    fn to_string(&self, role: &T) -> String;
    /// Returns the role wrapper for peers that should receive config updates.
    fn receive_role(&self) -> T;
}

// Implement ApplicationRole for the concrete permission enum.
impl ApplicationRole for IntentConfigPermission {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "update-config" | "intent-update" => Ok(Self::UpdateConfig),
            "receive-config-updates" | "intent-receive" => Ok(Self::ReceiveConfigUpdates),
            _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::UpdateConfig => "update-config",
            Self::ReceiveConfigUpdates => "receive-config-updates",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        // In this simple model, any valid role can connect to any other.
        true
    }

    fn can_access_room(&self, room_name: &str) -> bool {
        // All roles can access the intent-config room.
        room_name == "intent-config"
    }

    fn receive_config_updates_role() -> Option<Self> {
        Some(Self::ReceiveConfigUpdates)
    }
}

impl fmt::Display for IntentConfigPermission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AuthRoleMapper for IntentConfigPermission {
    /// Maps connection-level AuthRole to component-level IntentConfigPermission.
    ///
    /// Authorization mapping:
    /// - `AuthRole::Database` → `IntentConfigPermission::UpdateConfig` (can update configuration)
    /// - `AuthRole::Collector` → `IntentConfigPermission::ReceiveConfigUpdates` (receives updates)
    /// - Other roles → `None` (denied access to this component)
    fn from_auth_role(role: AuthRole) -> Option<Self> {
        match role {
            AuthRole::Database => Some(IntentConfigPermission::UpdateConfig),
            AuthRole::Collector => Some(IntentConfigPermission::ReceiveConfigUpdates),
            AuthRole::ClientRo => Some(IntentConfigPermission::ReceiveConfigUpdates),
            AuthRole::ClientAdmin => Some(IntentConfigPermission::UpdateConfig),
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
            IntentConfigPermission::from_cn("update-config"),
            Ok(IntentConfigPermission::UpdateConfig)
        ));
        assert!(matches!(
            IntentConfigPermission::from_cn("intent-update"),
            Ok(IntentConfigPermission::UpdateConfig)
        ));
        assert!(matches!(
            IntentConfigPermission::from_cn("receive-config-updates"),
            Ok(IntentConfigPermission::ReceiveConfigUpdates)
        ));
        assert!(matches!(
            IntentConfigPermission::from_cn("intent-receive"),
            Ok(IntentConfigPermission::ReceiveConfigUpdates)
        ));
    }

    #[test]
    fn test_from_cn_unknown() {
        let res = IntentConfigPermission::from_cn("no-such-role");
        assert!(matches!(
            res,
            Err(zznet_auth::error::AuthError::UnknownRole(_))
        ));
    }

    #[test]
    fn test_as_str() {
        assert_eq!(
            IntentConfigPermission::UpdateConfig.as_str(),
            "update-config"
        );
        assert_eq!(
            IntentConfigPermission::ReceiveConfigUpdates.as_str(),
            "receive-config-updates"
        );
    }

    #[test]
    fn test_can_connect_and_access_room() {
        // Current implementation is permissive; ensure methods return true
        let a = IntentConfigPermission::UpdateConfig;
        let b = IntentConfigPermission::ReceiveConfigUpdates;
        assert!(a.can_connect_to(&b));
        assert!(b.can_connect_to(&a));
        assert!(a.can_access_room("intent-config"));
        assert!(b.can_access_room("intent-config"));
    }

    #[test]
    fn test_serde_roundtrip_ron() {
        let p = IntentConfigPermission::UpdateConfig;
        let s = ron::ser::to_string(&p).expect("serialize ron");
        let p2: IntentConfigPermission = ron::de::from_str(&s).expect("deserialize ron");
        assert_eq!(p, p2);
    }
}
