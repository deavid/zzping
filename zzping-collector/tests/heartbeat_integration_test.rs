use std::io::Write;
use tempfile::tempdir;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use zzping_database::spawn_test_server;
use zzping_proto::zzping::{ingestion_client::IngestionClient, HeartbeatRequest, HeartbeatResponse};

#[tokio::test]
async fn test_collector_gets_config_via_heartbeat() {
    // 1. Setup: Create a temporary database with a known intent.ron config.
    let temp_dir = tempdir().unwrap();
    let intent_path = temp_dir.path().join("intent.ron");
    let mut file = std::fs::File::create(intent_path).unwrap();
    let expected_config = r#"
(
    ping_rate_pps: 100,
    targets: [ "1.1.1.1", "8.8.8.8" ],
)
"#;
    write!(file, "{expected_config}").unwrap();

    let (server_addr, _server_handle) =
        spawn_test_server(temp_dir.path().to_str().unwrap().to_string()).await;

    // 2. Mock Collector: Spawn a task that acts like a simplified collector.
    let (tx, mut rx) = mpsc::channel::<HeartbeatResponse>(1);

    tokio::spawn(async move {
        // Connect to the test server
        let mut client = IngestionClient::connect(format!("http://{server_addr}"))
            .await
            .unwrap();

        // Perform one heartbeat
        let request = tonic::Request::new(HeartbeatRequest {
            collector_uuid: "test-collector".to_string(),
        });

        let response = client.heartbeat(request).await.unwrap();

        // Send the received config back to the main test thread
        tx.send(response.into_inner()).await.unwrap();
    });

    // 3. Assert: The main thread waits for the config and verifies it.
    let received_config = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("Test timed out waiting for heartbeat response")
        .unwrap();

    assert_eq!(received_config.ping_rate_pps, 100);
    assert_eq!(received_config.targets, vec!["1.1.1.1", "8.8.8.8"]);
}
