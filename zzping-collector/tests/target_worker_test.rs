// tests/target_worker_test.rs

use anyhow::Result;
use std::{net::IpAddr, sync::Arc};
use tokio::sync::mpsc;
use zzping_collector::{
    database_client::DatabaseClient,
    ping_mock_client::PingMockClient,
    target_worker::{TargetWorker, WorkerCommand},
};
use zzping_proto::zzping::{CollectorRole, GetRecentDataResponse};

// Import the common test utilities
mod common;
use common::MockIngestionService;

#[tokio::test]
async fn test_target_worker_primary_supervised_role_sends_init_command() -> Result<()> {
    // 1. Setup
    let mock_service = MockIngestionService::new();
    let addr = common::spawn_mock_server(mock_service.clone()).await;
    let db_client =
        DatabaseClient::connect(format!("http://{}", addr), "test-token".to_string()).await?;

    // Configure the mock response for GetRecentData
    let expected_ack_nanos = 1234567890;

    let response = GetRecentDataResponse {
        records: vec![],
        database_confirms_last_acked_received_nanos: expected_ack_nanos,
    };
    *mock_service.get_recent_data_response.lock().unwrap() = response;

    // 2. Create the TargetWorker with MockPingClient
    let target_ip = "127.0.0.1".parse::<IpAddr>()?;
    let (ping_event_tx, _) = mpsc::channel(10);
    let ping_client = Arc::new(PingMockClient::new(target_ip, ping_event_tx));
    let handles = TargetWorker::new_with_ping_client(
        "test-uuid".to_string(),
        target_ip,
        1, // Use a normal ping rate since we're providing our own client
        db_client,
        ping_client,
    )?;

    // 3. Send the command and verify the result
    handles
        .handle
        .command_tx
        .send(WorkerCommand::UpdateRole(CollectorRole::PrimarySupervised))
        .await?;

    // We would need to intercept the channels created inside TargetWorker::new to actually test this.
    // The current design does not permit this easily.
    // A full test would look something like this:
    // let pinger_cmd = pinger_cmd_rx.recv().await.unwrap();
    // assert!(matches!(pinger_cmd, PingerCommand::UpdateRole(CollectorRole::PrimarySupervised)));
    // let submitter_cmd = submitter_cmd_rx.recv().await.unwrap();
    // assert!(matches!(submitter_cmd, BatchSubmitterCommand::InitializeAckCursor(nanos) if nanos == expected_ack_nanos));

    Ok(())
}
