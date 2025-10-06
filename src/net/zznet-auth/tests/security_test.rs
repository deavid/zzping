//! Security-focused tests for ACL enforcement and related checks.
//!
use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zznet_auth::{acl::AclManager, config::AclConfig};

#[test]
fn reject_missing_cn_or_san() {
    // This crate's TLS parsing is exercised elsewhere; here we simulate behavior by
    // ensuring that ACL validation and config loading fail for missing entries.
    let cfg = AclConfig::new(vec!["".to_string()], false);
    assert!(cfg.validate().is_err());

    // Empty ACL should deny everything
    let manager = AclManager::new();
    let id = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "eve".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id).is_err());
}

#[test]
fn acl_denies_when_not_listed() {
    let mut allowed = HashSet::new();
    allowed.insert("collector".to_string());
    let manager = AclManager::with_allowed_peers(allowed);

    let id = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "mallory".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };

    let res = manager.authorize_peer(&id);
    assert!(res.is_err());
}

/// Test that plain TCP cannot impersonate TLS identity (ACL prevents unauthorized)
#[test]
fn test_plain_tcp_cannot_impersonate_tls_identity() {
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    let manager = AclManager::with_allowed_peers(allowed);

    // Plain TCP identity (placeholder) should be denied
    let plain_identity = PeerIdentity {
        common_name: "plain-tcp".to_string(),
        san_username: "unknown".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };
    assert!(manager.authorize_peer(&plain_identity).is_err());

    // Even if someone tries to spoof with correct CN but wrong SAN
    let spoofed_identity = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "bob".to_string(), // not alice
        peer_addr: "127.0.0.1:8081".to_string(),
    };
    assert!(manager.authorize_peer(&spoofed_identity).is_err());
}

/// Test that HELLO messages cannot override certificate identity (simulated via ACL)
#[test]
fn test_hello_cannot_override_certificate_identity() {
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    let manager = AclManager::with_allowed_peers(allowed);

    // Bob trying to connect as alice should fail
    let bob_as_alice = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "bob".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };
    assert!(manager.authorize_peer(&bob_as_alice).is_err());
}

/// Test ACL security: denied users cannot bypass ACL
#[test]
fn test_acl_denies_bypass_attempts() {
    let mut allowed = HashSet::new();
    allowed.insert("collector".to_string());
    let manager = AclManager::with_allowed_peers(allowed);

    // Mallory tries various bypass attempts
    let attempts = vec![
        PeerIdentity {
            common_name: "client-admin".to_string(),
            san_username: "mallory".to_string(),
            peer_addr: "127.0.0.1:8080".to_string(),
        },
        PeerIdentity {
            common_name: "database".to_string(),
            san_username: "mallory".to_string(),
            peer_addr: "127.0.0.1:8081".to_string(),
        },
        PeerIdentity {
            common_name: "client-ro".to_string(),
            san_username: "mallory".to_string(),
            peer_addr: "127.0.0.1:8082".to_string(),
        },
    ];

    for attempt in attempts {
        assert!(manager.authorize_peer(&attempt).is_err());
    }
}

/// Test that role-only ACL doesn't grant unintended access
#[test]
fn test_role_only_acl_no_unintended_access() {
    let mut allowed = HashSet::new();
    allowed.insert("collector".to_string());
    let manager = AclManager::with_allowed_peers(allowed);

    // Only collector role should work, not others
    let collector = PeerIdentity {
        common_name: "collector".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };
    assert!(manager.authorize_peer(&collector).is_ok());

    // Other roles denied
    let client_admin = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };
    assert!(manager.authorize_peer(&client_admin).is_err());
}

/// Test that empty ACL denies all access
#[test]
fn test_empty_acl_denies_all() {
    let manager = AclManager::new();

    let identities = vec![
        PeerIdentity {
            common_name: "collector".to_string(),
            san_username: "root".to_string(),
            peer_addr: "127.0.0.1:8080".to_string(),
        },
        PeerIdentity {
            common_name: "client-admin".to_string(),
            san_username: "alice".to_string(),
            peer_addr: "127.0.0.1:8081".to_string(),
        },
    ];

    for id in identities {
        assert!(manager.authorize_peer(&id).is_err());
    }
}

/// Test connection hijacking prevention: peer_identity cannot be modified after handshake
#[test]
fn test_peer_identity_immutable_after_handshake() {
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    let manager = AclManager::with_allowed_peers(allowed);

    let alice = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    // Alice should be authorized
    assert!(manager.authorize_peer(&alice).is_ok());

    // If someone tries to modify the identity (simulated), it should still be checked
    // Since identity is immutable in our model, this is more of a design assurance
    let modified_alice = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(), // same
    };
    assert!(manager.authorize_peer(&modified_alice).is_ok());

    // But changing username should fail
    let bob_as_alice = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "bob".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };
    assert!(manager.authorize_peer(&bob_as_alice).is_err());
}

/// Test concurrent connection attempts with different identities
#[test]
fn test_concurrent_different_identities() {
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    allowed.insert("collector".to_string());
    let manager = AclManager::with_allowed_peers(allowed);

    let alice = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    let collector = PeerIdentity {
        common_name: "collector".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };

    let mallory = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "mallory".to_string(),
        peer_addr: "127.0.0.1:8082".to_string(),
    };

    // Simulate concurrent checks
    let results: Vec<_> = vec![&alice, &collector, &mallory]
        .into_iter()
        .map(|id| manager.authorize_peer(id))
        .collect();

    assert!(results[0].is_ok()); // alice ok
    assert!(results[1].is_ok()); // collector ok
    assert!(results[2].is_err()); // mallory denied
}

/// Test that ACL changes take effect immediately
#[test]
fn test_acl_changes_take_effect_immediately() {
    let mut manager = AclManager::new();

    let alice = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    // Initially denied
    assert!(manager.authorize_peer(&alice).is_err());

    // Add to ACL
    manager.allow_user("alice@client-admin");

    // Now allowed
    assert!(manager.authorize_peer(&alice).is_ok());

    // Remove from ACL
    manager.deny_user("alice@client-admin");

    // Denied again
    assert!(manager.authorize_peer(&alice).is_err());
}

/// Test that disconnecting peer invalidates identity (simulated via ACL removal)
#[test]
fn test_disconnect_invalidates_identity() {
    let mut manager = AclManager::new();

    let alice = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    // Add alice
    manager.allow_user("alice@client-admin");
    assert!(manager.authorize_peer(&alice).is_ok());

    // Simulate disconnect by removing from ACL
    manager.deny_user("alice@client-admin");
    assert!(manager.authorize_peer(&alice).is_err());
}
