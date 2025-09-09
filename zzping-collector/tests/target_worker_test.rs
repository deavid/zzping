// tests/target_worker_test.rs

use anyhow::Result;
use std::net::IpAddr;
use tokio::sync::mpsc;
use zzping_collector::{
    database_client::DatabaseClient,
    target_worker::{TargetWorker, WorkerCommand},
};
use zzping_proto::zzping::{CollectorRole, GetRecentDataResponse};

// Import the common test utilities
mod common;
use common::MockIngestionService;

#[tokio::test]
#[ignore] // Ignoring this test because it's not fully implemented and relies on a complex setup.
async fn test_target_worker_primary_supervised_role_sends_init_command() -> Result<()> {
    // 1. Setup
    let mock_service = MockIngestionService::new();
    let addr = common::spawn_mock_server(mock_service.clone()).await;
    let db_client = DatabaseClient::connect(format!("http://{}", addr), "test-token".to_string()).await?;

    // Configure the mock response for GetRecentData
    let expected_ack_nanos = 1234567890;

    let response = GetRecentDataResponse {
        records: vec![],
        database_confirms_last_acked_received_nanos: expected_ack_nanos,
    };
    *mock_service.get_recent_data_response.lock().unwrap() = response;

    // 2. Create the TargetWorker
    let handles = TargetWorker::new(
        "test-uuid".to_string(),
        "127.0.0.1".parse::<IpAddr>()?,
        10,
        db_client,
    )?;

    // 3. Send the command and verify the result
    handles.handle.command_tx.send(WorkerCommand::UpdateRole(CollectorRole::PrimarySupervised)).await?;

    // We would need to intercept the channels created inside TargetWorker::new to actually test this.
    // The current design does not permit this easily.
    // A full test would look something like this:
    // let pinger_cmd = pinger_cmd_rx.recv().await.unwrap();
    // assert!(matches!(pinger_cmd, PingerCommand::UpdateRole(CollectorRole::PrimarySupervised)));
    // let submitter_cmd = submitter_cmd_rx.recv().await.unwrap();
    // assert!(matches!(submitter_cmd, BatchSubmitterCommand::InitializeAckCursor(nanos) if nanos == expected_ack_nanos));

    Ok(())
}
