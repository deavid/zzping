//! Minimal generic doc test for zznet-auth — uses a local MockRole to exercise the API.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zznet_auth::acl::AclManager;
use zznet_auth::role::ApplicationRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum MockRole {
    A,
    B,
}

impl ApplicationRole for MockRole {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "a" => Ok(MockRole::A),
            "b" => Ok(MockRole::B),
            other => Err(zznet_auth::error::AuthError::UnknownRole(other.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            MockRole::A => "a",
            MockRole::B => "b",
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
fn generic_acl_basic() {
    let mut allowed = HashSet::new();
    allowed.insert("a".to_string());

    let manager: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    let id = PeerIdentity {
        common_name: "a".to_string(),
        san_username: "user".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };

    assert!(manager.authorize_peer(&id).is_ok());
}
