//! Edge-case tests moved to application crate.
//!
use std::collections::HashSet;
use zznet_api::types::PeerTLSIdentity;
use zzping_auth::AclManagerDefault;

#[test]
fn unicode_username_and_long_username_moved() {
    let mut allowed = std::collections::HashSet::new();
    allowed.insert("josé@client-ro".to_string());

    let manager = AclManagerDefault::with_allowed_peers(allowed);

    let id = PeerTLSIdentity {
        common_name: "client-ro".to_string(),
        san_username: "josé".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id).is_ok());

    let long = "a".repeat(300);
    let mut mgr2 = AclManagerDefault::new();
    mgr2.allow_user(&format!("{}@client-ro", &long));
    let id2 = PeerTLSIdentity {
        common_name: "client-ro".to_string(),
        san_username: long.clone(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(mgr2.authorize_peer(&id2).is_ok());
}

#[test]
fn special_characters_in_usernames_moved() {
    let mut allowed = HashSet::new();
    allowed.insert("user@#$%^&*()@client-ro".to_string());
    allowed.insert("user-with.dots@client-ro".to_string());
    allowed.insert("user_with_underscores@client-ro".to_string());
    allowed.insert("user-with-dashes@client-ro".to_string());

    let manager = AclManagerDefault::with_allowed_peers(allowed);

    let test_cases = vec![
        ("user@#$%^&*()", "client-ro"),
        ("user-with.dots", "client-ro"),
        ("user_with_underscores", "client-ro"),
        ("user-with-dashes", "client-ro"),
    ];

    for (username, cn) in test_cases {
        let id = PeerTLSIdentity {
            common_name: cn.to_string(),
            san_username: username.to_string(),
            peer_addr: "127.0.0.1:0".to_string(),
        };
        assert!(
            manager.authorize_peer(&id).is_ok(),
            "Failed for {}",
            username
        );
    }
}
