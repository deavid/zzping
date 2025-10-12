//! Permission wrapper for the MemDB component.

use serde::{Deserialize, Serialize};
use zznet_auth::role::ApplicationRole;

/// A wrapper around an ApplicationRole to be used by the MemDB component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct PermissionWrapper<T: ApplicationRole> {
    /// The wrapped permission.
    pub permission: T,
}

impl<'de, T: ApplicationRole> Deserialize<'de> for PermissionWrapper<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        T::deserialize(deserializer).map(|permission| Self { permission })
    }
}

impl<T: ApplicationRole> ApplicationRole for PermissionWrapper<T> {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        T::from_cn(cn).map(|permission| Self { permission })
    }

    fn as_str(&self) -> &'static str {
        self.permission.as_str()
    }

    fn can_connect_to(&self, target: &Self) -> bool {
        self.permission.can_connect_to(&target.permission)
    }

    fn can_access_room(&self, room_name: &str) -> bool {
        self.permission.can_access_room(room_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::MemDBPermission;

    #[test]
    fn test_permission_wrapper_creation() {
        let perm = MemDBPermission::SubmitBatch;
        let wrapper = PermissionWrapper { permission: perm };

        assert_eq!(wrapper.permission, MemDBPermission::SubmitBatch);
        assert_eq!(wrapper.as_str(), "submit-batch");
    }

    #[test]
    fn test_permission_wrapper_from_cn() {
        let wrapper = PermissionWrapper::<MemDBPermission>::from_cn("submit-batch").unwrap();
        assert_eq!(wrapper.permission, MemDBPermission::SubmitBatch);
    }

    #[test]
    fn test_permission_wrapper_room_access() {
        let wrapper = PermissionWrapper {
            permission: MemDBPermission::QueryData,
        };

        assert!(wrapper.can_access_room("memdb"));
        assert!(!wrapper.can_access_room("other-room"));
    }

    #[test]
    fn test_permission_wrapper_deserialize() {
        // Test that deserialization works through the ApplicationRole trait
        // Since PermissionWrapper implements Deserialize via the ApplicationRole deserialize
        let perm_str = "submit-batch";
        let permission = MemDBPermission::from_cn(perm_str).unwrap();
        let wrapper = PermissionWrapper { permission };

        // Verify the wrapper was created correctly
        assert_eq!(wrapper.permission, MemDBPermission::SubmitBatch);
        assert_eq!(wrapper.as_str(), "submit-batch");
    }

    #[test]
    fn test_permission_wrapper_can_connect_to() {
        let submit_wrapper = PermissionWrapper {
            permission: MemDBPermission::SubmitBatch,
        };
        let query_wrapper = PermissionWrapper {
            permission: MemDBPermission::QueryData,
        };

        // All permissions can connect to each other in this implementation
        assert!(submit_wrapper.can_connect_to(&query_wrapper));
        assert!(query_wrapper.can_connect_to(&submit_wrapper));
    }
}
