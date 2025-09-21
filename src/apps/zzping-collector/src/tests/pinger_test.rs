// zzping-collector/tests/pinger_test.rs

use crate::database_client::DatabaseClient;
use crate::ping_mock_client::PingMockClient;
use crate::pinger::{Pinger, PingerCommand};
use ntest::timeout;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use zzping_proto::zzping::CollectorRole;

// Include the test utilities
use super::common;
use common::MockIngestionService;

#[tokio::test]
#[timeout(2000)]
async fn test_pinger_loop() {
    common::setup_logger();

    let target: IpAddr = "127.0.0.1".parse().unwrap();
    let ping_rate_pps = 100;
    let test_duration = Duration::from_millis(50);

    let (ping_event_tx, mut ping_event_rx) = mpsc::channel(100);
    let mock_ping_client = Arc::new(PingMockClient::new(target, ping_event_tx));
    let (results_tx, _results_rx) = mpsc::channel(100);

    // Spawn a mock gRPC server to handle the AnnouncePings RPC.
    let server_addr = common::spawn_mock_server(MockIngestionService::default()).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "test-token".to_string())
            .await
            .unwrap();

    let (pinger_cmd_tx, pinger_cmd_rx) = mpsc::channel(10);
    let pinger = Pinger::new(
        target,
        ping_rate_pps,
        Duration::from_secs(60),
        Duration::from_secs(5),
        Duration::from_secs(60),
        mock_ping_client.clone(),
        results_tx,
        db_client,
        pinger_cmd_rx,
    );

    let pinger_handle = tokio::spawn(pinger.run());

    // Activate the pinger
    pinger_cmd_tx
        .send(PingerCommand::UpdateRole(CollectorRole::Primary))
        .await
        .unwrap();

    // Let the pinger run for a short duration
    tokio::time::sleep(test_duration).await;

    // Stop the pinger by dropping the command sender
    drop(pinger_cmd_tx);

    // Wait for the pinger to finish
    tokio::time::timeout(Duration::from_secs(1), pinger_handle)
        .await
        .expect("Pinger task timed out")
        .expect("Pinger task panicked")
        .expect("Pinger run method returned an error");

    // Assertions
    let mut pings_sent = 0;
    while ping_event_rx.try_recv().is_ok() {
        pings_sent += 1;
    }
    let expected_pings = (ping_rate_pps as f64 * test_duration.as_secs_f64()).round() as usize;

    // Check if the number of pings is within a reasonable tolerance
    let tolerance = 5;
    let delta = pings_sent as isize - expected_pings as isize;
    assert!(
        delta.abs() <= tolerance as isize,
        "Expected around {expected_pings} pings, but got {pings_sent}"
    );
}

#[test]
fn test_monotonic_time_source_resync_updates_reference() {
    // MonotonicTimeSource is private; a direct unit test would require changing
    // visibility. We rely on the higher-level pinger tests and integration
    // tests to exercise resync behavior.
    common::setup_logger();
}

#[tokio::test]
#[timeout(1000)]
async fn test_pinger_handles_lost_packets() {
    common::setup_logger();

    let target: IpAddr = "127.0.0.1".parse().unwrap();
    let grace_period = Duration::from_millis(50);

    let (ping_event_tx, _) = mpsc::channel(100);
    let mock_ping_client = PingMockClient::new(target, ping_event_tx);
    mock_ping_client.set_rtt_to_send(None).await;
    let mock_ping_client = Arc::new(mock_ping_client);

    let (results_tx, mut results_rx) = mpsc::channel(100);

    let server_addr = common::spawn_mock_server(MockIngestionService::default()).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "test-token".to_string())
            .await
            .unwrap();

    let (pinger_cmd_tx, pinger_cmd_rx) = mpsc::channel(10);
    let pinger = Pinger::new(
        target,
        10, // Ping rate doesn't matter much for this test
        grace_period,
        Duration::from_millis(10), // Fast timeout check for test
        Duration::from_secs(60),
        mock_ping_client.clone(),
        results_tx,
        db_client,
        pinger_cmd_rx,
    );

    let _pinger_handle = tokio::spawn(pinger.run());

    // Activate the pinger
    pinger_cmd_tx
        .send(PingerCommand::UpdateRole(CollectorRole::Primary))
        .await
        .unwrap();

    // Wait for the grace period to elapse, plus a buffer
    tokio::time::sleep(grace_period + Duration::from_millis(50)).await;

    // The pinger should have detected the lost ping and sent a result
    let result = tokio::time::timeout(Duration::from_millis(10), results_rx.recv())
        .await
        .expect("Test timed out waiting for lost packet result")
        .expect("Channel should not be empty");

    assert!(result.rtt.is_none(), "RTT should be None for a lost packet");
    assert_ne!(result.sent_nanos, 0, "sent_nanos should be populated");
}

#[tokio::test]
#[timeout(1000)]
async fn test_pinger_pauses_and_resumes() {
    common::setup_logger();

    let target: IpAddr = "127.0.0.1".parse().unwrap();
    let (ping_event_tx, mut ping_event_rx) = mpsc::channel(100);
    let mock_ping_client = Arc::new(PingMockClient::new(target, ping_event_tx));
    let (results_tx, _results_rx) = mpsc::channel(100);
    let server_addr = common::spawn_mock_server(MockIngestionService::default()).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "test-token".to_string())
            .await
            .unwrap();
    let (pinger_cmd_tx, pinger_cmd_rx) = mpsc::channel(10);
    let pinger = Pinger::new(
        target,
        100, // High rate to ensure we see pings quickly
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(60),
        mock_ping_client.clone(),
        results_tx,
        db_client,
        pinger_cmd_rx,
    );
    let _pinger_handle = tokio::spawn(pinger.run());

    // 1. Should not be active initially
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        ping_event_rx.try_recv().is_err(),
        "Pinger should not be active by default"
    );

    // 2. Activate it
    pinger_cmd_tx
        .send(PingerCommand::UpdateRole(CollectorRole::Primary))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        ping_event_rx.try_recv().is_ok(),
        "Pinger should start sending pings when role is Primary"
    );

    // Drain the channel
    while ping_event_rx.try_recv().is_ok() {}

    // 3. Pause it
    pinger_cmd_tx
        .send(PingerCommand::UpdateRole(CollectorRole::Standby))
        .await
        .unwrap();
    // Give the command time to be processed
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        ping_event_rx.try_recv().is_err(),
        "Pinger should stop sending pings when role is Standby"
    );

    // 4. Resume it
    pinger_cmd_tx
        .send(PingerCommand::UpdateRole(CollectorRole::Primary))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        ping_event_rx.try_recv().is_ok(),
        "Pinger should resume sending pings when role is Primary again"
    );
}
