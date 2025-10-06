//! Integration tests for zzping-auth
//!
//! These tests verify the complete authorization flow from certificate identity
//! extraction through ACL checking.

use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zznet_auth::{acl::AclManager, config::AclConfig, error::AuthError, role::AuthRole};

/// Test that AclConfig properly configures an AclManager
#[test]
fn test_acl_config_to_manager() {
    let allowed_peers = vec!["collector".to_string(), "alice@client-admin".to_string()];

    let config = AclConfig::new(allowed_peers, false);

    let manager = config.into_acl_manager().unwrap();

    // Service role should be allowed
    let collector_identity = PeerIdentity {
        common_name: "collector".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };
    assert!(manager.authorize_peer(&collector_identity).is_ok());

    // Specific user@role should be allowed
    let alice_identity = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };
    assert!(manager.authorize_peer(&alice_identity).is_ok());

    // Random user with same role should be denied
    let bob_identity = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "bob".to_string(),
        peer_addr: "127.0.0.1:8082".to_string(),
    };
    assert!(manager.authorize_peer(&bob_identity).is_err());
}

/// Test role-only matching in ACL
#[test]
fn test_role_only_acl_matching() {
    let mut allowed_peers = HashSet::new();
    allowed_peers.insert("database".to_string());

    let manager = AclManager::with_allowed_peers(allowed_peers);

    // Any service with database role should be allowed
    let database_identity = PeerIdentity {
        common_name: "database".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:9000".to_string(),
    };
    let result = manager.authorize_peer(&database_identity);
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), AuthRole::Database));
}

/// Test user@role specific matching in ACL
#[test]
fn test_user_role_specific_acl_matching() {
    let mut allowed_peers = HashSet::new();
    allowed_peers.insert("alice@client-ro".to_string());
    allowed_peers.insert("bob@client-admin".to_string());

    let manager = AclManager::with_allowed_peers(allowed_peers);

    // Alice with client-ro should be allowed
    let alice_identity = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };
    let result = manager.authorize_peer(&alice_identity);
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), AuthRole::ClientRo));

    // Bob with client-admin should be allowed
    let bob_identity = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "bob".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };
    let result = manager.authorize_peer(&bob_identity);
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), AuthRole::ClientAdmin));

    // Alice with wrong role should be denied
    let alice_wrong_role = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8082".to_string(),
    };
    assert!(manager.authorize_peer(&alice_wrong_role).is_err());

    // Charlie (not in ACL) should be denied
    let charlie_identity = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "charlie".to_string(),
        peer_addr: "127.0.0.1:8083".to_string(),
    };
    assert!(manager.authorize_peer(&charlie_identity).is_err());
}

/// Test authorization error types
#[test]
fn test_authorization_error_types() {
    let manager = AclManager::new(); // Empty ACL

    let identity = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "eve".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    let result = manager.authorize_peer(&identity);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        AuthError::IdentityNotAllowed(_)
    ));

    // Test unknown role
    let bad_identity = PeerIdentity {
        common_name: "hacker".to_string(),
        san_username: "eve".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    let mut allowed = HashSet::new();
    allowed.insert("hacker".to_string());
    let manager2 = AclManager::with_allowed_peers(allowed);

    let result = manager2.authorize_peer(&bad_identity);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AuthError::UnknownRole(_)));
}

/// Test the authorize_peer_option bridge function
#[test]
fn test_authorize_peer_option_bridge() {
    let mut allowed_peers = HashSet::new();
    allowed_peers.insert("collector".to_string());

    let manager = AclManager::with_allowed_peers(allowed_peers);

    let good_identity = PeerIdentity {
        common_name: "collector".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    let bad_identity = PeerIdentity {
        common_name: "database".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };

    // Good identity should return Some(role)
    let result = manager.authorize_peer_option(&good_identity);
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), AuthRole::Collector));

    // Bad identity should return None
    let result = manager.authorize_peer_option(&bad_identity);
    assert!(result.is_none());
}

/// Test the to_authorizer function that creates a closure
#[test]
fn test_to_authorizer_closure() {
    let mut allowed_peers = HashSet::new();
    allowed_peers.insert("database".to_string());
    allowed_peers.insert("alice@client-admin".to_string());

    let manager = AclManager::with_allowed_peers(allowed_peers);
    let authorizer = manager.to_authorizer();

    let database_identity = PeerIdentity {
        common_name: "database".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:9000".to_string(),
    };

    let alice_identity = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    let eve_identity = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "eve".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };

    // Authorized peers should return Some(role)
    assert!(authorizer(&database_identity).is_some());
    assert!(authorizer(&alice_identity).is_some());

    // Unauthorized peer should return None
    assert!(authorizer(&eve_identity).is_none());
}

/// Test insecure mode flag
#[test]
fn test_insecure_mode_flag() {
    let allowed_peers = HashSet::new();

    let manager_secure = AclManager::with_allowed_peers_and_insecure(allowed_peers.clone(), false);
    assert!(!manager_secure.is_insecure_mode());

    let manager_insecure = AclManager::with_allowed_peers_and_insecure(allowed_peers, true);
    assert!(manager_insecure.is_insecure_mode());
}

/// Test role-based connection policies
#[test]
fn test_role_connection_policies() {
    // Collector can only connect to Database
    assert!(AuthRole::Collector.can_connect_to(&AuthRole::Database));
    assert!(!AuthRole::Collector.can_connect_to(&AuthRole::Collector));
    assert!(!AuthRole::Collector.can_connect_to(&AuthRole::ClientRo));
    assert!(!AuthRole::Collector.can_connect_to(&AuthRole::ClientAdmin));

    // Database accepts from Collector and Clients, but not from Database
    assert!(AuthRole::Database.can_connect_to(&AuthRole::Collector));
    assert!(AuthRole::Database.can_connect_to(&AuthRole::ClientRo));
    assert!(AuthRole::Database.can_connect_to(&AuthRole::ClientAdmin));
    assert!(!AuthRole::Database.can_connect_to(&AuthRole::Database));

    // ClientRo can only connect to Database
    assert!(!AuthRole::ClientRo.can_connect_to(&AuthRole::Collector));
    assert!(AuthRole::ClientRo.can_connect_to(&AuthRole::Database));
    assert!(!AuthRole::ClientRo.can_connect_to(&AuthRole::ClientRo));
    assert!(!AuthRole::ClientRo.can_connect_to(&AuthRole::ClientAdmin));

    // ClientAdmin can connect to all services
    assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::Collector));
    assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::Database));
    assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::ClientRo));
    assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::ClientAdmin));
}

/// Test room access policies
#[test]
fn test_role_room_access_policies() {
    // Collector can access memdb (write) but not query
    assert!(AuthRole::Collector.can_access_room("memdb"));
    assert!(!AuthRole::Collector.can_access_room("query"));
    assert!(!AuthRole::Collector.can_access_room("config"));

    // Database can access memdb and query but not config
    assert!(AuthRole::Database.can_access_room("memdb"));
    assert!(AuthRole::Database.can_access_room("query"));
    assert!(!AuthRole::Database.can_access_room("config"));

    // ClientRo can access query only
    assert!(!AuthRole::ClientRo.can_access_room("memdb"));
    assert!(AuthRole::ClientRo.can_access_room("query"));
    assert!(!AuthRole::ClientRo.can_access_room("config"));

    // ClientAdmin can access all rooms (admin privilege)
    assert!(AuthRole::ClientAdmin.can_access_room("memdb"));
    assert!(AuthRole::ClientAdmin.can_access_room("query"));
    assert!(AuthRole::ClientAdmin.can_access_room("config"));
    assert!(AuthRole::ClientAdmin.can_access_room("any_room"));
}

/// Test full identity string formatting
#[test]
fn test_peer_identity_full_identity() {
    // Service identity (san_username = "root")
    let service = PeerIdentity {
        common_name: "collector".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };
    assert_eq!(service.full_identity(), "collector");
    assert!(service.is_service());

    // User identity (san_username != "root")
    let user = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };
    assert_eq!(user.full_identity(), "alice@client-admin");
    assert!(!user.is_service());
}

/// Test dynamic ACL modification
#[test]
fn test_dynamic_acl_modification() {
    let mut manager = AclManager::new();

    let alice = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    // Initially denied
    assert!(manager.authorize_peer(&alice).is_err());

    // Add alice to allow-list
    manager.allow_user("alice@client-ro");
    assert!(manager.authorize_peer(&alice).is_ok());

    // Remove alice from allow-list
    manager.deny_user("alice@client-ro");
    assert!(manager.authorize_peer(&alice).is_err());
}

/// Test that the ACL manager correctly differentiates role-only vs user@role entries
#[test]
fn test_acl_precedence_and_specificity() {
    let mut allowed_peers = HashSet::new();
    // Allow all collectors, but specific user for client-admin
    allowed_peers.insert("collector".to_string());
    allowed_peers.insert("alice@client-admin".to_string());

    let manager = AclManager::with_allowed_peers(allowed_peers);

    // Any collector service should work
    let collector1 = PeerIdentity {
        common_name: "collector".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };
    assert!(manager.authorize_peer(&collector1).is_ok());

    // Alice as client-admin should work
    let alice = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };
    assert!(manager.authorize_peer(&alice).is_ok());

    // Bob as client-admin should NOT work (not in allow-list)
    let bob = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "bob".to_string(),
        peer_addr: "127.0.0.1:8082".to_string(),
    };
    assert!(manager.authorize_peer(&bob).is_err());
}
