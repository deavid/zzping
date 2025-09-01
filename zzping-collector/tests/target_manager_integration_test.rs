use std::net::IpAddr;
use std::time::Duration;
use tokio::sync::mpsc;
use zzping_collector::ping_client::PingResult;
use zzping_collector::target_manager::run_target_manager;
use zzping_database::spawn_test_server;
use zzping_proto::zzping::ingestion_client::IngestionClient;

#[tokio::test]
async fn test_target_manager_happy_path() {
    let server_addr = spawn_test_server().await;
    let client = IngestionClient::connect(format!("http://{}", server_addr))
        .await
        .unwrap();

    let (ping_tx, ping_rx) = mpsc::channel(100);

    let manager_handle = tokio::spawn(run_target_manager(
        client,
        "test-host".to_string(),
        "1.2.3.4".parse::<IpAddr>().unwrap(),
        ping_rx,
    ));

    // Simulate the ping source
    for i in 0..250 {
        ping_tx
            .send(PingResult {
                sent_nanos: i + 1,
                rtt: Some(Duration::from_millis(100)),
            })
            .await
            .unwrap();
    }

    // Close the ping channel, which should cause the target manager to terminate gracefully.
    drop(ping_tx);

    // The test will pass if the manager runs to completion without panicking.
    // We can make this more robust by adding a timeout.
    let result = tokio::time::timeout(Duration::from_secs(5), manager_handle).await;
    assert!(result.is_ok(), "Target manager timed out");
    assert!(result.unwrap().is_ok(), "Target manager panicked");
}
