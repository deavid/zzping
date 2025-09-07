// zzping-collector/tests/pinger_test.rs

use std::net::IpAddr;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use zzping_collector::database_client::DatabaseClient;
use zzping_collector::pinger::Pinger;
use zzping_collector::ping_mock_client::PingMockClient;

// Include the test utilities
mod common;
use common::MockIngestionService;

#[tokio::test]
async fn test_pinger_loop() {
    let target: IpAddr = "127.0.0.1".parse().unwrap();
    let ping_rate_pps = 100;
    let test_duration = Duration::from_millis(50);

    let mock_ping_client = Arc::new(PingMockClient::new());
    let (results_tx, results_rx) = mpsc::channel(100);

    // Spawn a mock gRPC server to handle the AnnouncePings RPC.
    let server_addr = common::spawn_mock_server(MockIngestionService::default()).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();

    let pinger = Pinger::new(
        target,
        ping_rate_pps,
        Duration::from_secs(60),
        Duration::from_secs(5),
        mock_ping_client.clone(),
        results_tx,
        db_client,
    );

    let pinger_handle = tokio::spawn(pinger.run());

    // Let the pinger run for a short duration
    sleep(test_duration);

    // Stop the pinger by dropping the receiver
    drop(results_rx);

    // Wait for the pinger to finish, panicking if it times out or panics itself.
    timeout(Duration::from_secs(1), pinger_handle)
        .await
        .expect("Pinger task timed out")
        .expect("Pinger task panicked")
        .expect("Pinger run method returned an error");

    // Assertions
    let num_pings_sent = mock_ping_client.pings.lock().unwrap().len();
    let expected_pings = (ping_rate_pps as f64 * test_duration.as_secs_f64()).round() as usize;

    // Check if the number of pings is within a reasonable tolerance
    let tolerance = 5;
    assert!(
        (num_pings_sent as i32 - expected_pings as i32).abs() <= tolerance,
        "Expected around {expected_pings} pings, but got {num_pings_sent}"
    );
}

#[tokio::test]
async fn test_pinger_handles_lost_packets() {
    let target: IpAddr = "127.0.0.1".parse().unwrap();
    let grace_period = Duration::from_millis(50);

    // Use a mock client that never sends replies
    let mut mock_ping_client = PingMockClient::new();
    mock_ping_client.rtt_to_send = None;
    let mock_ping_client = Arc::new(mock_ping_client);

    let (results_tx, mut results_rx) = mpsc::channel(100);

    let server_addr = common::spawn_mock_server(MockIngestionService::default()).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();

    let pinger = Pinger::new(
        target,
        10, // Ping rate doesn't matter much for this test
        grace_period,
        Duration::from_millis(10), // Fast timeout check for test
        mock_ping_client.clone(),
        results_tx,
        db_client,
    );

    let _pinger_handle = tokio::spawn(pinger.run());

    // Wait for the grace period to elapse, plus a buffer
    tokio::time::sleep(grace_period + Duration::from_millis(50)).await;

    // The pinger should have detected the lost ping and sent a result
    let result = timeout(Duration::from_millis(10), results_rx.recv())
        .await
        .expect("Test timed out waiting for lost packet result")
        .expect("Channel should not be empty");

    assert!(result.rtt.is_none(), "RTT should be None for a lost packet");
    assert_eq!(result.sequence_idx, 0, "Sequence index should be correct");
}
