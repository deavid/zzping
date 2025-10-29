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
use std::time::Duration;
use zzintent_config::{
    actor::IntentConfigActor,
    messages::{GetCurrentConfig, IntentConfigData, UpdateConfig},
};

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
