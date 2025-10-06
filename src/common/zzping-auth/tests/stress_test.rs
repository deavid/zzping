//! Stress tests moved to application crate.
//!
use std::collections::HashSet;
use std::time::Instant;
use zznet_api::types::PeerIdentity;
use zzping_auth::AclManagerDefault;

#[test]
fn stress_large_acl_lookup_moved() {
    let mut allowed = HashSet::new();
    for i in 0..10_000u32 {
        allowed.insert(format!("user{}@client-ro", i));
    }

    let manager = AclManagerDefault::with_allowed_peers(allowed);

    let id_present = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "user9999".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };

    let start = Instant::now();
    let ok = manager.authorize_peer(&id_present).is_ok();
    let dur = start.elapsed();
    assert!(ok);
    assert!(dur.as_millis() < 50, "authorization too slow: {:?}", dur);
}
