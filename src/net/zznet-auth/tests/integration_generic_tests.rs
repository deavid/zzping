//! Generic integration tests for zznet-auth (crate-local mock roles)

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zznet_auth::acl::AclManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum MockRole {
    Foo,
    Bar,
}

impl zznet_auth::ApplicationRole for MockRole {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "foo" => Ok(MockRole::Foo),
            "bar" => Ok(MockRole::Bar),
            other => Err(zznet_auth::error::AuthError::UnknownRole(other.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            MockRole::Foo => "foo",
            MockRole::Bar => "bar",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        true
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
