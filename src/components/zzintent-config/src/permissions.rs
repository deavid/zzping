//! Defines the permissions for the zzintent-config component.

/// Permissions for the zzintent-config component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntentConfigPermission {
    /// Allows updating the configuration.
    UpdateConfig,
    /// Allows receiving configuration updates.
    ReceiveConfigUpdates,
}

/// A trait for checking permissions.
pub trait PermissionCheck<T> {
    /// Checks if the given role has permission to update the config.
    fn has_update_permission(&self, role: &T) -> bool;
    /// Checks if the given role has permission to receive config updates.
    fn has_receive_permission(&self, role: &T) -> bool;
    /// Returns a string representation of the permission.
    fn to_string(&self, role: &T) -> String;
}

use serde::{Deserialize, Serialize};
use zznet_auth::error::AuthError;
use zznet_auth::role::ApplicationRole;

// Implement ApplicationRole for the concrete permission enum.
impl ApplicationRole for IntentConfigPermission {
    fn from_cn(cn: &str) -> Result<Self, AuthError> {
        match cn {
            "intent-update" | "update-config" => Ok(Self::UpdateConfig),
            "intent-receive" | "receive-config-updates" => Ok(Self::ReceiveConfigUpdates),
            other => Err(AuthError::UnknownRole(other.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            IntentConfigPermission::UpdateConfig => "update-config",
            IntentConfigPermission::ReceiveConfigUpdates => "receive-config-updates",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        // These permissions are orthogonal to connection topology for now.
        true
    }

    fn can_access_room(&self, _room_name: &str) -> bool {
        // Both roles can access the intent-config room in our tests.
        true
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
