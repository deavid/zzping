use crate::database_client::DatabaseClient;
use zzping_proto::zzping::{CollectorRole, HeartbeatRequest};

// Import the common test utilities
use ntest::timeout;

use super::common;
use common::{MockIngestionService, spawn_mock_server};

#[tokio::test]
#[timeout(1000)]
async fn test_database_client_connect_and_heartbeat() {
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();
    let addr = spawn_mock_server(MockIngestionService::with_ping_rate(10)).await;
    let client_addr = format!("http://{addr}");

    // Test successful connection and heartbeat.
    let client = DatabaseClient::connect(client_addr.clone(), "test-token".to_string())
        .await
        .unwrap();

    let request = HeartbeatRequest {
        collector_uuid: "test-uuid".to_string(),
        pid: 1234,
        current_role: CollectorRole::Standby as i32,
        buffer_record_count: 0,
        last_fatal_error: "".to_string(),
        last_processed_command_id: 0,
    };
    let response = client.heartbeat(request).await;
    assert!(response.is_ok());
    let response = response.unwrap().into_inner();
    assert_eq!(response.ping_rate_pps, 10);
    assert_eq!(response.role, CollectorRole::Primary as i32);

    // Test with a bad token.
    let bad_client = DatabaseClient::connect(client_addr, "bad-token".to_string())
        .await
        .unwrap();
    let request = HeartbeatRequest {
        collector_uuid: "test-uuid".to_string(),
        pid: 1234,
        current_role: CollectorRole::Standby as i32,
        buffer_record_count: 0,
        last_fatal_error: "".to_string(),
        last_processed_command_id: 0,
    };
    let response = bad_client.heartbeat(request).await;
    assert!(response.is_err());
    let err = response.unwrap_err();
    let status = err.downcast_ref::<tonic::Status>().unwrap();
    assert_eq!(status.code(), tonic::Code::Unauthenticated);
}
