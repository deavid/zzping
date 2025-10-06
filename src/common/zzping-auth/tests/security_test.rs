//! Security tests moved into application crate.
//!
use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zzping_auth::{config::AclConfig, AclManagerDefault};

#[test]
fn reject_missing_cn_or_san_moved() {
    let cfg = AclConfig::new(vec!["".to_string()], false);
    assert!(cfg.validate().is_err());

    let manager = AclManagerDefault::new();
    let id = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "eve".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id).is_err());
}

#[test]
fn acl_denies_when_not_listed_moved() {
    let mut allowed = HashSet::new();
    allowed.insert("collector".to_string());
    let manager = AclManagerDefault::with_allowed_peers(allowed);

    let id = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "mallory".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };

    let res = manager.authorize_peer(&id);
    assert!(res.is_err());
}
