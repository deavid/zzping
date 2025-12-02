//! Integration test for the TCP Lock contention ("Highlander") scenario.
//!
//! This test ensures that a collector will not start pinging if another process
//! holds the designated TCP lock port, and that it will start pinging once the
//! lock is released.

use std::net::{IpAddr, TcpListener};
use std::time::Duration;
use tracing::info;
use zzping_integration_test::harness::{HarnessConfig, SystemHarness};

#[ctor::ctor]
fn init() {
    tracing_subscriber::fmt()
        .with_env_filter(
            "info,zzping_integration_test=debug,zztcp_lock=debug,zzcollector_state=debug",
        )
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .init();
}

#[actix_rt::test]
async fn test_lock_contention_highlander_rule() {
    tokio::time::pause();

    info!("Setting up Highlander test: there can be only one.");

    // 1. Setup Lock: Manually bind the lock port to simulate "Process A"
    let lock_port = 9000;
    let bind_addr = format!("127.0.0.1:{}", lock_port);
    info!(
        "Manually binding TCP listener to {} to simulate an existing process.",
        bind_addr
    );
    let _process_a_lock = TcpListener::bind(&bind_addr).expect("Failed to bind manual TCP lock");

    // 2. Start Harness: "Process B" starts up and tries to get the same lock
    info!("Starting SystemHarness (Process B), which will contend for the same lock.");
    let harness_config = HarnessConfig {
        lock_port: Some(lock_port),
        collector_id: "process-b".to_string(),
    };
    let harness = SystemHarness::new(harness_config)
        .await
        .expect("Failed to create SystemHarness");

    // 3. Configure Intent: Give the pinger a task.
    let target: IpAddr = "8.8.8.8".parse().unwrap();
    harness.configure_intent(vec![target], 1).await;

    // Give actors plenty of time to start and for the lock to fail.
    tokio::time::advance(Duration::from_secs(5)).await;

    // 4. Expectation: Pinger should NOT have generated data
    let health = harness.collector_health().await.unwrap();
    assert_eq!(
        health.buffer_size, 0,
        "Pinger should NOT have stored any pings while the lock was held by another process."
    );
    info!("Verified: Pinger is correctly disabled while lock is contended.");

    // 5. Transition: Drop the manual listener
    info!("Releasing manual TCP lock (Process A dies).");
    drop(_process_a_lock);

    // 6. Advance time & Verify
    info!("Advancing time to allow Process B to acquire the lock and start pinging.");
    // Wait for pings. This helper advances time internally, so we don't need a separate advance call.
    harness
        .wait_for_pings(1)
        .await
        .expect("Pinger should have generated results after acquiring lock.");

    info!("Verified: Pinger started generating data after acquiring the lock. Test passed.");
}
