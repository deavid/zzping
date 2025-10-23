//! Generic documentation tests using a local MockRole implementing ApplicationRole.
//!
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zznet_auth::acl::AclManager;
use zznet_auth::role::ApplicationRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum MockRole {
    Service,
    User,
}

impl ApplicationRole for MockRole {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "service" => Ok(MockRole::Service),
            "user" => Ok(MockRole::User),
            _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            MockRole::Service => "service",
            MockRole::User => "user",
        }
    }

    fn can_connect_to(&self, target: &Self) -> bool {
        match self {
            MockRole::Service => matches!(target, MockRole::Service),
            MockRole::User => matches!(target, MockRole::Service),
        }
    }

    fn can_access_room(&self, _room_name: &str) -> bool {
        true
    }
}

#[test]
fn generic_doc_example() {
    let mut allowed = HashSet::new();
    allowed.insert("service".to_string());

    let acl: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    let svc_identity = PeerIdentity {
        common_name: "service".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:9000".to_string(),
    };

    assert!(acl.authorize_peer(&svc_identity).is_ok());
}
