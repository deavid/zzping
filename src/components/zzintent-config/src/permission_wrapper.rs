use serde::{Deserialize, Serialize};
use zznet_auth::role::ApplicationRole;

/// A wrapper around an ApplicationRole to be used by the IntentConfig component.
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
