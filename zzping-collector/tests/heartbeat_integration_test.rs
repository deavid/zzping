use ntest::timeout;
use std::io::Write;
use tempfile::tempdir;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};
use zzping_database::{auth::generate_test_token, spawn_test_server};
use zzping_proto::zzping::{
    HeartbeatRequest, HeartbeatResponse, ingestion_client::IngestionClient,
};

const TEST_TIMEOUT_DURATION: Duration = Duration::from_millis(500);

#[tokio::test]
#[timeout(1000)]
async fn test_collector_gets_config_via_heartbeat() {
    let _ = env_logger::builder().is_test(true).try_init();
    let temp_dir = tempdir().unwrap();
    let intent_path = temp_dir.path().join("intent.ron");
    let mut file = std::fs::File::create(intent_path).unwrap();
    let expected_config = r#"(ping_rate_pps: 100, targets: [ "1.1.1.1", "8.8.8.8" ])"#;
    write!(file, "{expected_config}").unwrap();

    let (server_addr, server_handle) = timeout(
        TEST_TIMEOUT_DURATION,
        spawn_test_server(temp_dir.path().to_str().unwrap().to_string()),
    )
    .await
    .unwrap();

    let (tx, mut rx) = mpsc::channel::<HeartbeatResponse>(1);

    tokio::spawn(async move {
        let mut client = timeout(
            TEST_TIMEOUT_DURATION,
            IngestionClient::connect(format!("http://{server_addr}")),
        )
        .await
        .unwrap()
        .unwrap();

        let token = generate_test_token("test-collector", &["collector"]);
        let mut request = tonic::Request::new(HeartbeatRequest {
            collector_uuid: "test-collector".to_string(),
            pid: 1234,
            current_role: 0,
            buffer_record_count: 0,
            last_fatal_error: "".to_string(),
            last_processed_command_id: 0,
        });
        request
            .metadata_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());

        let response = timeout(TEST_TIMEOUT_DURATION, client.heartbeat(request))
            .await
            .unwrap()
            .unwrap();

        timeout(TEST_TIMEOUT_DURATION, tx.send(response.into_inner()))
            .await
            .unwrap()
            .unwrap();
    });

    let received_config = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("Test timed out waiting for heartbeat response")
        .unwrap();

    assert_eq!(received_config.ping_rate_pps, 100);
    assert_eq!(received_config.targets, vec!["1.1.1.1", "8.8.8.8"]);

    server_handle.abort();
}