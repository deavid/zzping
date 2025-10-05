//! Documentation examples as runnable tests.
//!
//! These tests demonstrate common usage patterns for the zzping-auth crate.

use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zzping_auth::acl::AclManager;
use zzping_auth::config::AclConfig;

/// Example: Basic ACL setup and authorization
#[test]
fn example_basic_acl() {
    // Create an ACL manager with allowed peers
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    allowed.insert("collector".to_string()); // Allow all users in collector role

    let acl = AclManager::with_allowed_peers(allowed);

    // Test authorization for different identities
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

/// Example: Loading ACL from TOML configuration
#[test]
fn example_config_from_toml() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Create a temporary TOML config file
    let mut tf = NamedTempFile::new().unwrap();
    write!(
        tf,
        r#"
allowed_peers = ["user@client-ro", "collector"]
"#
    )
    .unwrap();

    // Load configuration
    let config = AclConfig::from_file(tf.path()).unwrap();
    let acl = AclManager::with_allowed_peers(config.allowed_peers.into_iter().collect());

    // Test the loaded ACL
    let allowed_user = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "user".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    assert!(acl.authorize_peer(&allowed_user).is_ok());
}

/// Example: Dynamic ACL modification
#[test]
fn example_dynamic_acl_modification() {
    let mut acl = AclManager::new();

    // Initially empty, should deny all
    let user = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "user".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };
    assert!(acl.authorize_peer(&user).is_err());

    // Add user to ACL
    acl.allow_user("user@client-ro");
    assert!(acl.authorize_peer(&user).is_ok());

    // Remove user from ACL
    acl.deny_user("user@client-ro");
    assert!(acl.authorize_peer(&user).is_err());
}

/// Example: Role-based access control
#[test]
fn example_role_based_access() {
    let mut allowed = HashSet::new();
    allowed.insert("client-ro".to_string()); // Allow entire role
    allowed.insert("admin@client-admin".to_string()); // Allow specific user

    let acl = AclManager::with_allowed_peers(allowed);

    // Any user in client-ro role is allowed
    let user1 = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    let user2 = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "bob".to_string(),
        peer_addr: "127.0.0.1:8081".to_string(),
    };

    // Only specific admin user is allowed
    let admin_allowed = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "admin".to_string(),
        peer_addr: "127.0.0.1:8082".to_string(),
    };

    let admin_denied = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "other".to_string(),
        peer_addr: "127.0.0.1:8083".to_string(),
    };

    assert!(acl.authorize_peer(&user1).is_ok());
    assert!(acl.authorize_peer(&user2).is_ok());
    assert!(acl.authorize_peer(&admin_allowed).is_ok());
    assert!(acl.authorize_peer(&admin_denied).is_err());
}

/// Example: Error handling
#[test]
fn example_error_handling() {
    let acl = AclManager::new();

    let invalid_identity = PeerIdentity {
        common_name: "invalid-role".to_string(),
        san_username: "user".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    // Authorization should fail for unauthorized identity
    let result = acl.authorize_peer(&invalid_identity);
    assert!(result.is_err());

    // Check the error type
    if let Err(zzping_auth::error::AuthError::IdentityNotAllowed(identity)) = result {
        assert_eq!(identity, "user@invalid-role");
    } else {
        panic!("Expected IdentityNotAllowed error");
    }
}

/// Example: Using the authorizer closure
#[test]
fn example_authorizer_closure() {
    let mut allowed = HashSet::new();
    allowed.insert("user@client-ro".to_string());
    let acl = AclManager::with_allowed_peers(allowed);

    // Get the authorizer function
    let authorizer = acl.to_authorizer();

    let identity = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "user".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    // Use the closure for authorization
    let role = authorizer(&identity);
    assert!(role.is_some());
    // The role should be ClientRo
    assert_eq!(format!("{:?}", role.unwrap()), "ClientRo");
}
