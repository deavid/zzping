//! Integration test for the zero-downtime handoff protocol.

use anyhow::Result;
use tokio::time::{Duration, advance, pause};
use zzcollector_state::messages::GetCollectorState;
use zzping_integration_test::harness::{HarnessConfig, SystemHarness};

/// Test Scenario: The "Smooth Operator"
/// 1. Collector A connects and becomes the primary.
/// 2. Collector B connects with the same ID, triggering a handoff.
/// 3. Database orchestrates the swap.
/// 4. Collector A becomes standby, Collector B becomes primary.
#[actix_rt::test]
async fn test_smooth_operator_handoff() -> Result<()> {
    pause(); // Enable simulated time

    let collector_id = "collector-handoff-test".to_string();
    let lock_port = 18080; // A unique port for this test

    // --- Act 1: Harness A (Old Collector) becomes Primary ---
    let harness_a = SystemHarness::new(HarnessConfig {
        lock_port: Some(lock_port),
        collector_id: collector_id.clone(),
        existing_db_server: None, // Create the DB
    })
    .await?;

    // TCP lock actor has a 1-second retry interval, so we need to advance at least that much
    // for the first lock acquisition attempt.
    advance(Duration::from_millis(1100)).await; // Let startup and first lock acquisition settle
    assert_mastership(&harness_a, true).await?;
    println!("Act 1 Complete: Harness A is Primary.");

    // --- Act 2: Harness B (New Collector) connects, triggering handoff timer ---
    let harness_b = SystemHarness::new(HarnessConfig {
        lock_port: Some(lock_port),
        collector_id: collector_id.clone(),
        existing_db_server: Some(harness_a.server_sender.clone()),
    })
    .await?;

    // B's heartbeat interval is 1 second. We need to advance enough for:
    // 1. B's first heartbeat to be sent (after 1s)
    // 2. The network message to be processed
    advance(Duration::from_millis(1100)).await; // Let handshake and heartbeat from B complete

    // After B's first heartbeat, the 5s handoff timer has started, but A is still primary.
    assert_mastership(&harness_a, true).await?;
    assert_mastership(&harness_b, false).await?;
    println!("Act 2 Complete: Harness B connected, handoff initiated.");

    // --- Act 3: The Jump.
    advance(Duration::from_secs(6)).await; // Jump past the 5s timer

    // Allow the actor system to process the run_later callback and network messages.
    // In paused time mode, we need to yield several times to let all the actors
    // process their mailboxes and propagate the SetMastership message.
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    // A small time advance helps process any pending timers
    advance(Duration::from_millis(10)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }

    // The `run_later` block in the DB actor has now executed. A is now standby.
    assert_mastership(&harness_a, false).await?;

    // B is not yet master, because its TcpLockActor has not retried yet.
    assert_mastership(&harness_b, false).await?;

    // Advance time enough for the 1s lock retry timer to fire.
    advance(Duration::from_millis(1100)).await;

    // NOW B should have the lock.
    assert_mastership(&harness_b, true).await?;
    println!("Act 3 Complete: Handoff successful. Harness A is Standby, Harness B is Primary.");

    Ok(())
}

/// Helper function to assert the mastership state of a harness's CState actor.
async fn assert_mastership(harness: &SystemHarness, should_be_master: bool) -> Result<()> {
    let cstate = harness
        .cstate
        .as_ref()
        .expect("CState not configured for this harness");
    // In a paused-time test, the actor's mailbox is processed on the next tick,
    // so we don't need to loop/wait. A simple yield is enough.
    tokio::task::yield_now().await;
    let state = cstate.send(GetCollectorState).await??;
    let is_master = state.has_local_lock && state.database_authorized;
    assert_eq!(
        is_master, should_be_master,
        "Mastership assertion failed. Expected: {}, Actual: {}",
        should_be_master, is_master
    );
    Ok(())
}
