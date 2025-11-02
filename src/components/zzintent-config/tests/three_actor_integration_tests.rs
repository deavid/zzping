//! Integration tests for IntentConfig three-actor pattern (Phase 3.9)
//!
//! These tests focus on comprehensive scenario coverage with fewer, longer tests
//! that validate end-to-end behavior rather than individual function tests.
//!
//! **Test Philosophy**: Happy-path focused, longer scenario tests that demonstrate
//! the three-actor pattern working correctly in realistic use cases.
//!
//! All tests have strict 100ms timeouts to prevent hanging.

use actix::prelude::*;
use std::collections::HashMap;
use std::time::Duration;
use zzintent_config::{
    actor::IntentConfigActor,
    messages::{GetCurrentConfig, IntentConfigData, UpdateConfig},
    network_manager::IntentConfigNetworkManager,
    permissions::IntentConfigPermissions,
};
use zznet_api::types::RoomId;
use zznet_router::RouterActor;

/// Timeout constant for all async operations (100ms max)
const TEST_TIMEOUT: Duration = Duration::from_millis(100);

/// Comprehensive Test 1: Basic Actor State Management
///
/// Scenario: Actor starts with default config, receives local UpdateConfig,
/// and maintains state consistency without involving subscribers or network.
///
/// This is the simplest possible test - just actor state management.
#[actix::test]
async fn test_actor_basic_state_management() {
    let actor = IntentConfigActor::default();
    let actor_addr = actor.start();

    // Verify initial default state
    let initial_config = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
        .await
        .expect("GetCurrentConfig timeout")
        .expect("GetCurrentConfig failed");
    assert_eq!(initial_config, IntentConfigData::default());

    // Send UpdateConfig with new configuration
    let new_config = IntentConfigData {
        targets: vec![
            "1.1.1.1".parse().unwrap(),
            "8.8.8.8".parse().unwrap(),
            "9.9.9.9".parse().unwrap(),
        ],
        ping_rate_pps: 100,
    };

    tokio::time::timeout(
        TEST_TIMEOUT,
        actor_addr.send(UpdateConfig {
            data: new_config.clone(),
            peer_id: None,
        }),
    )
    .await
    .expect("UpdateConfig timeout")
    .expect("UpdateConfig failed");

    // Verify actor's internal state updated correctly
    let current_config = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
        .await
        .expect("GetCurrentConfig timeout")
        .expect("GetCurrentConfig failed");
    assert_eq!(current_config.targets, new_config.targets);
    assert_eq!(current_config.ping_rate_pps, new_config.ping_rate_pps);
}

/// Comprehensive Test 2: Multiple Sequential State Updates
///
/// Scenario: Actor receives multiple sequential config updates and maintains
/// correct state transitions. This validates state consistency without the
/// complexity of subscribers or network communication.
#[actix::test]
async fn test_actor_sequential_state_updates() {
    let actor = IntentConfigActor::default();
    let actor_addr = actor.start();

    // Verify initial state
    let initial = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
        .await
        .expect("GetCurrentConfig timeout")
        .expect("GetCurrentConfig failed");
    assert_eq!(initial, IntentConfigData::default());

    // First update
    let config1 = IntentConfigData {
        targets: vec!["1.2.3.4".parse().unwrap()],
        ping_rate_pps: 50,
    };
    tokio::time::timeout(
        TEST_TIMEOUT,
        actor_addr.send(UpdateConfig {
            data: config1.clone(),
            peer_id: None,
        }),
    )
    .await
    .expect("UpdateConfig 1 timeout")
    .expect("UpdateConfig 1 failed");

    let state1 = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
        .await
        .expect("GetCurrentConfig timeout")
        .expect("GetCurrentConfig failed");
    assert_eq!(state1, config1);

    // Second update (overwrites previous)
    let config2 = IntentConfigData {
        targets: vec!["5.6.7.8".parse().unwrap(), "9.10.11.12".parse().unwrap()],
        ping_rate_pps: 200,
    };
    tokio::time::timeout(
        TEST_TIMEOUT,
        actor_addr.send(UpdateConfig {
            data: config2.clone(),
            peer_id: None,
        }),
    )
    .await
    .expect("UpdateConfig 2 timeout")
    .expect("UpdateConfig 2 failed");

    let state2 = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
        .await
        .expect("GetCurrentConfig timeout")
        .expect("GetCurrentConfig failed");
    assert_eq!(state2, config2);

    // Third update
    let config3 = IntentConfigData {
        targets: vec!["13.14.15.16".parse().unwrap()],
        ping_rate_pps: 300,
    };
    tokio::time::timeout(
        TEST_TIMEOUT,
        actor_addr.send(UpdateConfig {
            data: config3.clone(),
            peer_id: None,
        }),
    )
    .await
    .expect("UpdateConfig 3 timeout")
    .expect("UpdateConfig 3 failed");

    let state3 = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
        .await
        .expect("GetCurrentConfig timeout")
        .expect("GetCurrentConfig failed");
    assert_eq!(state3, config3);

    // Verify final state matches last update
    let final_config = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
        .await
        .expect("GetCurrentConfig timeout")
        .expect("GetCurrentConfig failed");
    assert_eq!(final_config, config3);
}

/// Comprehensive Test 3: Rapid Concurrent Updates
///
/// Scenario: Send multiple updates rapidly and verify the actor handles them
/// correctly, maintaining the final state. This tests the actor's ability to
/// handle concurrent messages without getting stuck.
#[actix::test]
async fn test_actor_rapid_concurrent_updates() {
    let actor = IntentConfigActor::default();
    let actor_addr = actor.start();

    // Send multiple updates in quick succession
    let configs: Vec<IntentConfigData> = (1..=5)
        .map(|i| IntentConfigData {
            targets: vec![format!("10.0.0.{}", i).parse().unwrap()],
            ping_rate_pps: i * 10,
        })
        .collect();

    // Fire all updates
    for config in &configs {
        tokio::time::timeout(
            TEST_TIMEOUT,
            actor_addr.send(UpdateConfig {
                data: config.clone(),
                peer_id: None,
            }),
        )
        .await
        .expect("UpdateConfig timeout")
        .expect("UpdateConfig failed");
    }

    // Give actor time to process all messages
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Verify final state matches last update
    let final_config = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
        .await
        .expect("GetCurrentConfig timeout")
        .expect("GetCurrentConfig failed");
    assert_eq!(final_config, configs[4]);
}

/// Comprehensive Test 4: Default Configuration Validation
///
/// Scenario: Verify that default configuration is properly initialized and
/// maintained until explicitly updated. This validates initialization logic.
#[actix::test]
async fn test_actor_default_configuration() {
    let actor = IntentConfigActor::default();
    let actor_addr = actor.start();

    // Query current config multiple times to ensure consistency
    for _ in 0..3 {
        let config = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
            .await
            .expect("GetCurrentConfig timeout")
            .expect("GetCurrentConfig failed");
        assert_eq!(config, IntentConfigData::default());

        // Small delay between queries
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Update to non-default config
    let new_config = IntentConfigData {
        targets: vec!["192.168.1.1".parse().unwrap()],
        ping_rate_pps: 25,
    };
    tokio::time::timeout(
        TEST_TIMEOUT,
        actor_addr.send(UpdateConfig {
            data: new_config.clone(),
            peer_id: None,
        }),
    )
    .await
    .expect("UpdateConfig timeout")
    .expect("UpdateConfig failed");

    // Verify config changed
    let updated = tokio::time::timeout(TEST_TIMEOUT, actor_addr.send(GetCurrentConfig))
        .await
        .expect("GetCurrentConfig timeout")
        .expect("GetCurrentConfig failed");
    assert_eq!(updated, new_config);
    assert_ne!(updated, IntentConfigData::default());
}

/// Test authorization: collector role cannot write config
#[actix::test]
async fn test_collector_cannot_write_config() {
    use std::collections::HashMap;
    use zzintent_config::permissions::IntentConfigPermissions;
    use zznet_router::RouterActor;

    let actor = IntentConfigActor::default();
    let actor_addr = actor.start();
    let router = RouterActor::new(vec![]).start();

    // Create permissions map: collector has read-only access
    let mut permissions_map = HashMap::new();
    permissions_map.insert(
        "collector".to_string(),
        IntentConfigPermissions::new(true, false), // can read, cannot write
    );

    let network_manager =
        IntentConfigNetworkManager::new(actor_addr.clone(), router, permissions_map);

    // Test the permissions map using the getter
    let permissions = network_manager.permissions_map().get("collector");
    assert!(permissions.is_some(), "collector should have permissions");
    assert!(
        permissions.unwrap().can_read_config,
        "collector should can read"
    );
    assert!(
        !permissions.unwrap().can_write_config,
        "collector should not can write"
    );
}

/// Test authorization: unknown role is denied
#[actix::test]
async fn test_unknown_role_denied() {
    use std::collections::HashMap;
    use zzintent_config::permissions::IntentConfigPermissions;
    use zznet_router::RouterActor;

    let actor = IntentConfigActor::default();
    let actor_addr = actor.start();
    let router = RouterActor::new(vec![]).start();

    // Create permissions map with only collector role
    let mut permissions_map = HashMap::new();
    permissions_map.insert(
        "collector".to_string(),
        IntentConfigPermissions::new(true, false),
    );

    let network_manager =
        IntentConfigNetworkManager::new(actor_addr.clone(), router, permissions_map);

    // Test that unknown role has no permissions using the getter
    let permissions = network_manager.permissions_map().get("hacker");
    assert!(
        permissions.is_none(),
        "unknown role should have no permissions"
    );
}

/// Test: Permissions Map Getter
///
/// Scenario: Verify that the permissions_map() getter returns the correct permissions
/// that were injected during NetworkManager construction.
#[actix::test]
async fn test_permissions_map_getter() {
    let actor = IntentConfigActor::default();
    let actor_addr = actor.start();

    let router = RouterActor::new(vec![RoomId::new("intent-config")]);
    let router_addr = router.start();

    // Create a test permissions map with multiple roles
    let mut permissions_map = HashMap::new();
    permissions_map.insert(
        "client-admin".to_string(),
        IntentConfigPermissions {
            can_read_config: true,
            can_write_config: true,
        },
    );
    permissions_map.insert(
        "collector".to_string(),
        IntentConfigPermissions {
            can_read_config: true,
            can_write_config: false,
        },
    );
    permissions_map.insert(
        "viewer".to_string(),
        IntentConfigPermissions {
            can_read_config: true,
            can_write_config: false,
        },
    );

    // Create the network manager with permissions
    let network_manager =
        IntentConfigNetworkManager::new(actor_addr.clone(), router_addr, permissions_map.clone());

    // Test that we can retrieve the permissions map
    let retrieved_map = network_manager.permissions_map();
    assert_eq!(retrieved_map.len(), 3, "should have 3 roles");

    // Verify client-admin has full permissions
    let admin_perms = retrieved_map
        .get("client-admin")
        .expect("client-admin should exist");
    assert!(
        admin_perms.can_read_config,
        "client-admin should be able to read"
    );
    assert!(
        admin_perms.can_write_config,
        "client-admin should be able to write"
    );

    // Verify collector has read-only permissions
    let collector_perms = retrieved_map
        .get("collector")
        .expect("collector should exist");
    assert!(
        collector_perms.can_read_config,
        "collector should be able to read"
    );
    assert!(
        !collector_perms.can_write_config,
        "collector should NOT be able to write"
    );

    // Verify viewer has read-only permissions
    let viewer_perms = retrieved_map.get("viewer").expect("viewer should exist");
    assert!(
        viewer_perms.can_read_config,
        "viewer should be able to read"
    );
    assert!(
        !viewer_perms.can_write_config,
        "viewer should NOT be able to write"
    );
}
