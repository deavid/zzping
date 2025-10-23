//! Generic integration tests for zznet-auth demonstrating usage with a MockRole.
//!
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zznet_auth::acl::AclManager;
use zznet_auth::error::AuthError;
use zznet_auth::role::ApplicationRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum MockRole {
    Foo,
    Bar,
}

impl ApplicationRole for MockRole {
    fn from_cn(cn: &str) -> Result<Self, AuthError> {
        match cn {
            "foo" => Ok(MockRole::Foo),
            "bar" => Ok(MockRole::Bar),
            other => Err(AuthError::UnknownRole(other.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            MockRole::Foo => "foo",
            MockRole::Bar => "bar",
        }
    }

    fn can_connect_to(&self, target: &Self) -> bool {
        matches!((self, target), (MockRole::Foo, MockRole::Bar))
    }

    fn can_access_room(&self, _room_name: &str) -> bool {
        true
    }
}

#[test]
fn generic_acl_allows_role_only_entries() {
    let mut allowed = HashSet::new();
    allowed.insert("foo".to_string());

    let acl: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    let identity = PeerIdentity {
        common_name: "foo".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };

    assert!(acl.authorize_peer(&identity).is_ok());
}
