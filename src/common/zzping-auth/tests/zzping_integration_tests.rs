//! Integration tests for zzping-auth helpers used by application-level code.
//!
//! These are small, focused tests that exercise the ACL manager and
//! configuration helpers.
use zznet_api::types::PeerIdentity;
use zznet_auth::{acl::AclManager, config::AclConfig};
use zzping_auth::AuthRole;

type AclManagerDefault = AclManager<AuthRole>;

#[test]
fn moved_test_acl_config_to_manager() {
    let allowed_peers = vec![
        "alice@collector".to_string(),
        "database".to_string(),
        "collector".to_string(),
    ];

    let config = AclConfig::new(allowed_peers, false);
    let manager: AclManagerDefault =
        AclManagerDefault::with_allowed_peers(config.allowed_peers.into_iter().collect());

    let collector_identity = PeerIdentity {
        common_name: "collector".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    assert!(manager.authorize_peer(&collector_identity).is_ok());
}
