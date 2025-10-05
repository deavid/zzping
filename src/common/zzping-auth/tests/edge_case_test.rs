use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use zznet_api::types::PeerIdentity;
use zzping_auth::acl::AclManager;

#[test]
fn unicode_username_and_long_username() {
    let mut allowed = std::collections::HashSet::new();
    allowed.insert("josé@client-ro".to_string());

    let manager = AclManager::with_allowed_peers(allowed);

    let id = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "josé".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id).is_ok());

    // Very long username
    let long = "a".repeat(300);
    let mut mgr2 = AclManager::new();
    mgr2.allow_user(&format!("{}@client-ro", &long));
    let id2 = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: long.clone(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(mgr2.authorize_peer(&id2).is_ok());
}

#[test]
fn special_characters_in_usernames() {
    let mut allowed = HashSet::new();
    allowed.insert("user@#$%^&*()@client-ro".to_string());
    allowed.insert("user-with.dots@client-ro".to_string());
    allowed.insert("user_with_underscores@client-ro".to_string());
    allowed.insert("user-with-dashes@client-ro".to_string());

    let manager = AclManager::with_allowed_peers(allowed);

    let test_cases = vec![
        ("user@#$%^&*()", "client-ro"),
        ("user-with.dots", "client-ro"),
        ("user_with_underscores", "client-ro"),
        ("user-with-dashes", "client-ro"),
    ];

    for (username, cn) in test_cases {
        let id = PeerIdentity {
            common_name: cn.to_string(),
            san_username: username.to_string(),
            peer_addr: "127.0.0.1:0".to_string(),
        };
        assert!(
            manager.authorize_peer(&id).is_ok(),
            "Failed for username: {}",
            username
        );
    }
}

#[test]
fn case_sensitivity_in_identities() {
    let mut allowed = HashSet::new();
    allowed.insert("Alice@client-ro".to_string());

    let manager = AclManager::with_allowed_peers(allowed);

    // Case mismatch should fail
    let id_upper = PeerIdentity {
        common_name: "CLIENT-RO".to_string(),
        san_username: "ALICE".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_upper).is_err());

    // Exact case should work
    let id_exact = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "Alice".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_exact).is_ok());
}

#[test]
fn whitespace_handling() {
    let mut allowed = HashSet::new();
    allowed.insert("user@client-ro".to_string());

    let manager = AclManager::with_allowed_peers(allowed);

    // Leading/trailing whitespace in identity should not match
    let id_with_space = PeerIdentity {
        common_name: " client-ro ".to_string(),
        san_username: " user ".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_with_space).is_err());

    // Exact match should work
    let id_exact = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "user".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_exact).is_ok());
}

#[test]
fn empty_and_boundary_strings() {
    let manager = AclManager::new();

    // Empty username should be denied
    let id_empty_user = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_empty_user).is_err());

    // Empty CN should be denied
    let id_empty_cn = PeerIdentity {
        common_name: "".to_string(),
        san_username: "user".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_empty_cn).is_err());

    // Allow empty ACL to deny all
    let empty_manager = AclManager::new();
    assert!(empty_manager.authorize_peer(&id_empty_user).is_err());
}

#[test]
fn network_address_variations() {
    let mut allowed = HashSet::new();
    allowed.insert("user@client-ro".to_string());

    let manager = AclManager::with_allowed_peers(allowed);

    let addresses = vec![
        "127.0.0.1:8080",
        "192.168.1.1:443",
        "[::1]:8080",
        "localhost:8080",
        "example.com:80",
    ];

    for addr in addresses {
        let id = PeerIdentity {
            common_name: "client-ro".to_string(),
            san_username: "user".to_string(),
            peer_addr: addr.to_string(),
        };
        // ACL doesn't check address, so should pass
        assert!(
            manager.authorize_peer(&id).is_ok(),
            "Failed for address: {}",
            addr
        );
    }
}

#[test]
fn role_boundary_cases() {
    let mut allowed = HashSet::new();
    allowed.insert("client-ro".to_string()); // role only
    allowed.insert("user@client-admin".to_string()); // user@role

    let manager = AclManager::with_allowed_peers(allowed);

    // Role only should work for any user in that role
    let id_role = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "anyuser".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_role).is_ok());

    // Specific user@role should work
    let id_specific = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "user".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_specific).is_ok());

    // Wrong user for specific role should fail
    let id_wrong = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "wronguser".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_wrong).is_err());
}

#[test]
fn concurrent_authorization_during_modification() {
    let manager = Arc::new(Mutex::new(AclManager::new()));
    let mut handles = vec![];

    // Start authorization threads
    for _ in 0..5 {
        let mgr_clone = Arc::clone(&manager);
        let handle = thread::spawn(move || {
            for j in 0..100 {
                let id = PeerIdentity {
                    common_name: "client-ro".to_string(),
                    san_username: format!("user{}", j),
                    peer_addr: "127.0.0.1:0".to_string(),
                };
                let _ = mgr_clone.lock().unwrap().authorize_peer(&id);
            }
        });
        handles.push(handle);
    }

    // Concurrently modify ACL
    let mgr_clone = Arc::clone(&manager);
    let modify_handle = thread::spawn(move || {
        for i in 0..100 {
            let mut mgr = mgr_clone.lock().unwrap();
            mgr.allow_user(&format!("user{}@client-ro", i));
            mgr.deny_user(&format!("user{}@client-ro", i));
        }
    });
    handles.push(modify_handle);

    // Wait for all
    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn very_long_common_names() {
    let long_username = "a".repeat(500);
    let mut allowed = HashSet::new();
    allowed.insert(format!("{}@client-ro", long_username));

    let manager = AclManager::with_allowed_peers(allowed);

    let id = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: long_username.clone(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id).is_ok());
}

#[test]
fn mixed_case_role_definitions() {
    let mut allowed = HashSet::new();
    allowed.insert("User@client-ro".to_string());

    let manager = AclManager::with_allowed_peers(allowed);

    // Exact case match
    let id = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "User".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id).is_ok());
}
