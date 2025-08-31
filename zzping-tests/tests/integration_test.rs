use anyhow::Result;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use zzping_collector::{
    ping_client::{PingClient, PingResult},
    ping_mock_client::PingMockClient,
    ping_target_loop, Cli,
};
use zzping_database::{ingestion::handle_ingestion_connection, IngestionItem};

#[tokio::test]
async fn test_collector_database_communication() -> Result<()> {
    // 1. Setup: Create a mock client that will send a known ping result.
    let expected_rtt = Duration::from_millis(50);
    let expected_sent_nanos = 12345;
    let mock_ping_client = Arc::new(PingMockClient {
        pings: Arc::new(Default::default()),
        result_to_send: Some(PingResult {
            sent_nanos: expected_sent_nanos,
            rtt: Some(expected_rtt),
        }),
    });

    // 2. Setup: Create a TCP listener on a random port to act as the database server.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let db_addr = listener.local_addr()?;
    println!("Test database listening on {}", db_addr);

    // 3. Setup: Create a channel to receive records on the database side.
    let (tx, mut rx) = mpsc::channel::<IngestionItem>(32);

    // 4. Run: Spawn the database server task.
    // It will accept one connection and pass it to the ingestion handler.
    let server_task = tokio::spawn(async move {
        let (stream, addr) = listener.accept().await.unwrap();
        handle_ingestion_connection(stream, addr, tx).await.unwrap();
    });

    // 5. Run: Spawn the collector task.
    let cli = Arc::new(Cli {
        targets: vec!["127.0.0.1".parse::<IpAddr>().unwrap()],
        source_hostname: "test-collector".to_string(),
        rate: 100, // High rate to ensure a ping is sent quickly
        database_addr: db_addr.to_string(),
        max_in_flight: 1,
    });

    let client_for_loop: Arc<dyn PingClient> = mock_ping_client.clone();
    let collector_task = tokio::spawn(ping_target_loop(cli, client_for_loop));

    // 6. Verification: Wait for a record to be received on the database side.
    // We'll use a timeout to prevent the test from hanging indefinitely.
    let received_item = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await?
        .expect("Failed to receive an item from the database side");

    // 7. Verification: Check that the received item has the expected data.
    assert_eq!(received_item.source_hostname, "test-collector");
    assert_eq!(
        received_item.target,
        "127.0.0.1".parse::<IpAddr>().unwrap()
    );
    assert_eq!(received_item.record.sent_nanos, expected_sent_nanos);
    assert_eq!(
        received_item.record.rtt_nanos,
        expected_rtt.as_nanos() as u64
    );

    // 8. Teardown: Abort the tasks to clean up.
    server_task.abort();
    collector_task.abort();

    Ok(())
}
