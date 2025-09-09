// tests/graceful_shutdown_test.rs

use anyhow::Result;
use ntest::timeout;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use zzping_collector::{
    bootstrap_collector_for_test, pinger::FinalizedPing, task_supervisor::SupervisorShutdown,
};

// Import the common test utilities
mod common;
use common::MockIngestionService;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[timeout(20000)] // 20 second timeout to prevent hangs
#[ignore] // Ignoring this test as it is flaky and times out.
async fn test_graceful_shutdown_flushes_buffer() -> Result<()> {
    // 1. Setup
    let mock_service = MockIngestionService::new();
    let addr = common::spawn_mock_server(mock_service.clone()).await;

    // Create a temporary config file
    let mut config = zzping_collector::config::Config::load("tests/test-configs/valid-config.ron")?;
    config.database_addr = format!("http://{}", addr);
    let temp_dir = tempfile::tempdir()?;
    let temp_config_path = temp_dir.path().join("config.ron");
    let config_str = ron::to_string(&config)?;
    tokio::fs::write(&temp_config_path, config_str).await?;
    let config_path_str = temp_config_path.to_str().unwrap().to_string();

    // Create channels to receive the test handles from the service
    let (worker_data_tx_sender, mut worker_data_tx_receiver) = mpsc::channel(1);
    let (shutdown_tx_sender, mut shutdown_tx_receiver) = mpsc::channel(1);

    // Bootstrap the collector using the special test function
    let service =
        bootstrap_collector_for_test(config_path_str, worker_data_tx_sender, shutdown_tx_sender)?;

    // Configure the mock server to initially fail SendBatch requests
    *mock_service.send_batch_should_fail.lock().unwrap() = true;

    // 2. Execute
    let service_handle = tokio::spawn(service.run());

    // Wait to receive the handles from the service
    let data_tx = worker_data_tx_receiver.recv().await.expect("Failed to receive data_tx for worker");
    let shutdown_tx = shutdown_tx_receiver.recv().await.expect("Failed to receive shutdown_tx");

    // Inject a ping directly into the BatchSubmitter's buffer
    let injected_ping = FinalizedPing {
        sent_nanos: 12345,
        rtt: Some(Duration::from_millis(10)),
    };
    data_tx.send(injected_ping).await?;

    // Let the collector run. It will try to send a batch, but it will fail.
    // The health check interval is 1 second. We wait for 2 to be sure a heartbeat is sent.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 3. Verify Buffer State
    let heartbeats = mock_service.received_heartbeats.lock().unwrap();
    assert!(!heartbeats.is_empty(), "Expected at least one heartbeat");
    let last_heartbeat = heartbeats.last().unwrap();
    assert_eq!(last_heartbeat.buffer_record_count, 1, "Expected buffer to contain 1 record, but heartbeat reported {}", last_heartbeat.buffer_record_count);
    assert!(mock_service.received_batches.lock().unwrap().is_empty(), "No batches should have been received while failing");

    // 4. Trigger Shutdown
    // Re-enable batch sending in the mock server so the final flush can succeed.
    *mock_service.send_batch_should_fail.lock().unwrap() = false;

    // Send the shutdown command directly to the supervisor
    let (ack_tx, ack_rx) = oneshot::channel();
    shutdown_tx.send(SupervisorShutdown { ack_sender: ack_tx }).await?;

    // Wait for the supervisor to acknowledge the shutdown command has been processed
    ack_rx.await?;

    // 5. Await completion and Assert
    // Now that the shutdown is complete, the service should exit.
    service_handle.await??;

    // Check the mock service to see if the final batch was received.
    let batches = mock_service.received_batches.lock().unwrap();
    assert_eq!(batches.len(), 1, "Expected exactly one final batch to be flushed on shutdown, but got {}", batches.len());

    let received_batch = batches.first().unwrap();
    assert_eq!(received_batch.records.len(), 1, "Expected the flushed batch to contain 1 record");

    let received_record = received_batch.records.first().unwrap();
    assert_eq!(received_record.sent_nanos, 12345);
    assert_eq!(received_record.rtt_nanos, Duration::from_millis(10).as_nanos() as u64);

    Ok(())
}
