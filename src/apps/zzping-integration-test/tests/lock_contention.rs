//! Integration test for the Lock contention ("Highlander") scenario.
//!
//! This test ensures that a collector will not start pinging if another process
//! holds the lock, and that it will start pinging once the lock is released.
//!
//! Uses Memory locks for fully deterministic, hermetic testing.

use std::net::IpAddr;
use tokio::time::{Duration, advance, pause};
use zzcollector_state::messages::GetCollectorState;
use zzping_integration_test::harness::{HarnessConfig, SystemHarness};
use zztcp_lock::backend::MemoryLockGuard;
use zztcp_lock::config::LockStrategy;

/// Test Scenario: The "Highlander Rule" - There can be only one.
/// 1. Process A holds the lock (simulated via MemoryLockGuard).
/// 2. Process B (harness) starts and tries to get the same lock.
/// 3. Process B should NOT become master while A holds the lock.
/// 4. Process A releases the lock.
/// 5. Process B should acquire the lock and become master.
#[actix_rt::test]
async fn test_lock_contention_highlander_rule() {
    pause(); // Enable simulated time for determinism

    let lock_id = "highlander-test-lock".to_string();

    // 1. Process A acquires the lock first (simulating another process)
    let process_a_lock =
        MemoryLockGuard::try_acquire(lock_id.clone()).expect("Process A should acquire the lock");

    // 2. Start Harness: "Process B" starts up and tries to get the same lock
    let harness = SystemHarness::new(HarnessConfig {
        lock_strategy: Some(LockStrategy::Memory(lock_id.clone())),
        collector_id: "process-b".to_string(),
        existing_db_server: None,
    })
    .await
    .expect("Failed to create SystemHarness");

    // 3. Configure Intent: Give the pinger a task.
    let target: IpAddr = "8.8.8.8".parse().unwrap();
    harness.configure_intent(vec![target], 1).await;

    // Let the actor system process messages
    advance(Duration::from_millis(1)).await;
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }

    // 4. Expectation: Harness should NOT be master (lock held by A)
    let cstate = harness.cstate.as_ref().expect("CState should exist");
    let state = cstate.send(GetCollectorState).await.unwrap().unwrap();
    assert!(
        !state.has_local_lock,
        "Process B should NOT have the lock while A holds it"
    );

    // 5. Process A releases the lock
    drop(process_a_lock);

    // 6. Advance time to trigger lock retry and let actors process
    advance(Duration::from_millis(200)).await; // Past the 100ms retry interval
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }

    // 7. Expectation: Harness should now have the lock
    let state = cstate.send(GetCollectorState).await.unwrap().unwrap();
    assert!(
        state.has_local_lock,
        "Process B should have acquired the lock after A released it"
    );
}
