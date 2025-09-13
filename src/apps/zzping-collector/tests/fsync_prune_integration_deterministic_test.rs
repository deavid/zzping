use zzping_proto::zzping::{CollectorRole, HeartbeatResponse};

mod common;
use common::{MockIngestionService, spawn_mock_server};
use zzping_collector::database_client::DatabaseClient;

#[tokio::test]
async fn test_fsync_prune_deterministic_via_override_channel() {
    // Spawn the mock ingestion service and get its address.
    let mock = MockIngestionService::with_ping_rate(0);
    let addr = spawn_mock_server(mock.clone()).await;

    // Install the heartbeat override channel and obtain the sender.
    let sender = mock.install_heartbeat_override_channel().await;

    // Prepare a HeartbeatResponse that includes a last_fsynced_received_nanos value.
    let hb = HeartbeatResponse {
        targets: vec!["127.0.0.1".to_string()],
        ping_rate_pps: 0,
        role: CollectorRole::Primary as i32,
        swap_at_nanos: 0,
        last_fsynced_received_nanos: 123_456_789u64,
    };

    // Send the override response; the next heartbeat() call will return this.
    sender.send(hb.clone()).await.unwrap();

    // Create a DatabaseClient that points at the mock server.
    let db_client = DatabaseClient::connect(format!("http://{}", addr), "test-token".to_string())
        .await
        .expect("failed to create DatabaseClient");

    // Call heartbeat RPC; it should return the override response we sent.
    let req = zzping_proto::zzping::HeartbeatRequest {
        collector_uuid: "test-uuid".to_string(),
        pid: 0,
        current_role: zzping_proto::zzping::CollectorRole::Primary as i32,
        buffer_record_count: 0,
        last_fatal_error: String::new(),
        last_processed_command_id: 0,
    };
    let resp = db_client
        .heartbeat(req)
        .await
        .expect("heartbeat rpc failed");
    let body = resp.into_inner();
    assert_eq!(body.last_fsynced_received_nanos, 123_456_789u64);
}
