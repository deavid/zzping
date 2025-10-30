//! Focused tests for ZZPing-specific AuthRole business logic.
//!
//! These tests verify the specific connection and room access rules
//! defined in zzping_auth::AuthRole, without re-testing the generic
//! AclManager framework.

use zznet_auth::role::ApplicationRole;
use zzping_auth::AuthRole;

#[test]
fn test_role_serialization() {
    assert_eq!(AuthRole::Collector.as_str(), "collector");
    assert_eq!(AuthRole::Database.as_str(), "database");
    assert_eq!(AuthRole::ClientRo.as_str(), "client-ro");
    assert_eq!(AuthRole::ClientAdmin.as_str(), "client-admin");

    assert_eq!(AuthRole::from_cn("collector").unwrap(), AuthRole::Collector);
    assert_eq!(AuthRole::from_cn("database").unwrap(), AuthRole::Database);
    assert_eq!(AuthRole::from_cn("client-ro").unwrap(), AuthRole::ClientRo);
    assert_eq!(
        AuthRole::from_cn("client-admin").unwrap(),
        AuthRole::ClientAdmin
    );
    assert!(AuthRole::from_cn("unknown").is_err());
}

#[test]
fn test_connection_rules() {
    // ClientAdmin can connect to everything
    assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::Collector));
    assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::Database));
    assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::ClientRo));
    assert!(AuthRole::ClientAdmin.can_connect_to(&AuthRole::ClientAdmin));

    // Collector can only connect to Database
    assert!(AuthRole::Collector.can_connect_to(&AuthRole::Database));
    assert!(!AuthRole::Collector.can_connect_to(&AuthRole::Collector));
    assert!(!AuthRole::Collector.can_connect_to(&AuthRole::ClientRo));
    assert!(!AuthRole::Collector.can_connect_to(&AuthRole::ClientAdmin));

    // ClientRo can only connect to Database
    assert!(AuthRole::ClientRo.can_connect_to(&AuthRole::Database));
    assert!(!AuthRole::ClientRo.can_connect_to(&AuthRole::Collector));
    assert!(!AuthRole::ClientRo.can_connect_to(&AuthRole::ClientRo));
    assert!(!AuthRole::ClientRo.can_connect_to(&AuthRole::ClientAdmin));

    // Database can connect to Collector, ClientRo, ClientAdmin
    assert!(AuthRole::Database.can_connect_to(&AuthRole::Collector));
    assert!(AuthRole::Database.can_connect_to(&AuthRole::ClientRo));
    assert!(AuthRole::Database.can_connect_to(&AuthRole::ClientAdmin));
    assert!(!AuthRole::Database.can_connect_to(&AuthRole::Database));
}

#[test]
fn test_room_access_rules() {
    // ClientAdmin has access to everything
    assert!(AuthRole::ClientAdmin.can_access_room("memdb"));
    assert!(AuthRole::ClientAdmin.can_access_room("query"));
    assert!(AuthRole::ClientAdmin.can_access_room("any-room"));

    // Collector only has access to memdb
    assert!(AuthRole::Collector.can_access_room("memdb"));
    assert!(!AuthRole::Collector.can_access_room("query"));
    assert!(!AuthRole::Collector.can_access_room("other"));

    // Database has access to memdb and query
    assert!(AuthRole::Database.can_access_room("memdb"));
    assert!(AuthRole::Database.can_access_room("query"));
    assert!(!AuthRole::Database.can_access_room("other"));

    // ClientRo only has access to query
    assert!(!AuthRole::ClientRo.can_access_room("memdb"));
    assert!(AuthRole::ClientRo.can_access_room("query"));
    assert!(!AuthRole::ClientRo.can_access_room("other"));
}
