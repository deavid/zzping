use anyhow::Result;
use log::info;
use ntest::timeout;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;
use zzping_proto::zzping::{RawDataRecord, SendBatchRequest, ingestion_client::IngestionClient};

const NANOS_PER_MINUTE: u64 = 60 * 1_000_000_000;

#[tokio::test]
#[timeout(5000)]
async fn test_full_ingestion_pipeline() -> Result<()> {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init();
    // 1. Setup: Spawn the full database server in the background.
    let temp_dir = tempdir()?;
    let data_dir = temp_dir.path().to_str().unwrap().to_string();
    let (addr, server_handle) = spawn_full_server_for_test(data_dir).await;
    tokio::time::sleep(Duration::from_millis(100)).await; // Give server time to start.

    // 2. Execution: Simulate a client sending data for two different minutes.
    let mut client = IngestionClient::connect(format!("http://{addr}")).await?;
    let token = zzping_database::auth::generate_test_token("test-collector", &["collector"]);

    // First batch: records for minute 1
    let records_minute_1 = vec![
        RawDataRecord {
            sent_nanos: NANOS_PER_MINUTE + 100,
            rtt_nanos: 50,
        },
        RawDataRecord {
            sent_nanos: NANOS_PER_MINUTE + 200,
            rtt_nanos: 60,
        },
    ];
    let mut request1 = tonic::Request::new(SendBatchRequest {
        collector_uuid: "test-collector".to_string(),
        target_ip: "1.1.1.1".to_string(),
        records: records_minute_1.clone(),
        collector_believes_last_acked_received_nanos: 0,
    });
    request1
        .metadata_mut()
        .insert("authorization", format!("Bearer {token}").parse()?);
    client.send_batch(request1).await?;

    // Second batch: a single record for minute 2. This should trigger the flush of minute 1.
    let records_minute_2 = vec![RawDataRecord {
        sent_nanos: 2 * NANOS_PER_MINUTE,
        rtt_nanos: 70,
    }];
    let mut request2 = tonic::Request::new(SendBatchRequest {
        collector_uuid: "test-collector".to_string(),
        target_ip: "1.1.1.1".to_string(),
        records: records_minute_2.clone(),
        collector_believes_last_acked_received_nanos: NANOS_PER_MINUTE + 200,
    });
    request2
        .metadata_mut()
        .insert("authorization", format!("Bearer {token}").parse()?);
    client.send_batch(request2).await?;

    // Give the storage task a moment to write the file.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 3. Verification: Check that the file for minute 1 was written correctly.
    let data_files: Vec<_> = std::fs::read_dir(temp_dir.path())?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zzp1"))
        .collect();
    assert_eq!(data_files.len(), 1, "Expected one data file to be created");

    let file_path = data_files.first().unwrap().path();
    let file_content = std::fs::read(&file_path)?;
    info!(
        "Read {} bytes from file {:?}",
        file_content.len(),
        file_path
    );
    let decompressed_records = zzping_lib::chunked_v1::decompress_chunked_v1(&file_content)?;

    // Convert the sent records to the lib type for comparison.
    let expected_records: Vec<zzping_lib::protocol::RawDataRecord> = records_minute_1
        .into_iter()
        .map(|r| zzping_lib::protocol::RawDataRecord {
            sent_nanos: r.sent_nanos,
            rtt_nanos: r.rtt_nanos,
        })
        .collect();

    assert_eq!(
        decompressed_records, expected_records,
        "The records in the file should match the records that were sent for minute 1"
    );

    // 4. Teardown
    server_handle.abort();

    Ok(())
}

/// A test-specific runner that spawns the server but returns the address and a flush channel.
async fn spawn_full_server_for_test(
    data_dir: String,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    // Create a default intent file for the test server to use.
    let intent_path = std::path::Path::new(&data_dir).join("intent.ron");
    std::fs::write(intent_path, "(ping_rate_pps: 1, targets: [])").unwrap();

    let (item_tx, item_rx) = mpsc::channel(1024);

    tokio::spawn(zzping_database::storage_engine::storage_task(
        item_rx,
        data_dir.clone(),
    ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service =
        zzping_database::grpc_server::IngestionServiceImpl::new(item_tx, data_dir.clone());
    let server = zzping_proto::zzping::ingestion_server::IngestionServer::with_interceptor(
        service,
        zzping_database::grpc_server::check_auth,
    );

    let handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(server)
            .serve(addr)
            .await
            .unwrap();
    });

    (addr, handle)
}
