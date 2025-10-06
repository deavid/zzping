//! Moved documentation tests for ZZPing-specific AuthRole.
//!
use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zzping_auth::AclManagerDefault;

/// Basic ACL example moved from generic crate.
#[test]
fn example_basic_acl_moved() {
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    allowed.insert("collector".to_string());

    let acl: AclManagerDefault = AclManagerDefault::with_allowed_peers(allowed);

    let admin_user = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    let collector = PeerIdentity {
        common_name: "collector".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };

    let unauthorized = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "bob".to_string(),
        peer_addr: "127.0.0.1:8082".to_string(),
    };

    assert!(acl.authorize_peer(&admin_user).is_ok());
    assert!(acl.authorize_peer(&collector).is_ok());
    assert!(acl.authorize_peer(&unauthorized).is_err());
}
