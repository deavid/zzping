use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;
use zzping_collector::batch_submitter::BatchSubmitter;
use zzping_collector::database_client::DatabaseClient;
use zzping_collector::pinger::FinalizedPing;

use common::MockIngestionService;
use zzping_proto::zzping::{send_batch_response, SendBatchResponse};

mod common;

#[tokio::test]
async fn test_ingestion_logic() {
    let mock_service = MockIngestionService::new();
    let server_addr = common::spawn_mock_server(mock_service).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();
    let mut submitter = BatchSubmitter::new(
        "test-collector".to_string(),
        IpAddr::from_str("1.1.1.1").unwrap(),
        Duration::from_secs(60),
        1_000_000,
        Duration::from_secs(24 * 3600),
        db_client,
    );
    let grace_period_ns = submitter.grace_period.as_nanos() as u64;

    // 1. Ingest a successful ping
    let successful_ping = FinalizedPing {
        sent_nanos: 1000,
        rtt: Some(Duration::from_millis(50)),
    };
    let rtt_ns = successful_ping.rtt.unwrap().as_nanos() as u64;
    submitter.ingest_ping_result(successful_ping);

    assert_eq!(submitter.buffer.len(), 1);
    let (key, record) = submitter.buffer.iter().next().unwrap();
    assert_eq!(*key, 1000 + rtt_ns);
    assert_eq!(record.sent_nanos, 1000);
    assert_eq!(record.rtt_nanos, rtt_ns);

    // 2. Ingest a lost ping
    let lost_ping = FinalizedPing {
        sent_nanos: 2000,
        rtt: None,
    };
    submitter.ingest_ping_result(lost_ping);

    assert_eq!(submitter.buffer.len(), 2);
    let lost_key = 2000 + grace_period_ns;
    let lost_record = submitter.buffer.get(&lost_key).unwrap();
    assert_eq!(lost_record.sent_nanos, 2000);
    assert_eq!(lost_record.rtt_nanos, u64::MAX);
}

#[tokio::test]
async fn test_send_batch_ok_and_embargo() {
    // 1. Setup
    let mock_service = MockIngestionService::new();
    let server_addr = common::spawn_mock_server(mock_service.clone()).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();
    let mut submitter = BatchSubmitter::new(
        "test-collector".to_string(),
        IpAddr::from_str("1.1.1.1").unwrap(),
        Duration::from_secs(60),
        1_000_000,
        Duration::from_secs(24 * 3600),
        db_client,
    );

    // 2. Ingest a record that is in the past (should be sent)
    submitter.ingest_ping_result(FinalizedPing {
        sent_nanos: 1000,
        rtt: Some(Duration::from_millis(50)),
    });

    // 3. Ingest a "lost" record whose artificial received_nanos is in the future (should be embargoed)
    let future_sent_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    submitter.ingest_ping_result(FinalizedPing {
        sent_nanos: future_sent_nanos,
        rtt: None,
    });
    assert_eq!(submitter.buffer.len(), 2);

    // 4. Trigger send_batch. Only the first record should be sent.
    submitter.send_batch().await.unwrap();
    let received_batches = mock_service.received_batches.lock().unwrap();
    assert_eq!(received_batches.len(), 1);
    assert_eq!(received_batches[0].records.len(), 1);
    assert_eq!(received_batches[0].records[0].sent_nanos, 1000);
}

#[tokio::test]
async fn test_send_batch_desync() {
    // 1. Setup: Configure mock server to return DESYNC
    let mock_service = MockIngestionService::new();
    let new_acked_nanos = 5000;
    {
        let mut response = mock_service.send_batch_response.lock().unwrap();
        *response = SendBatchResponse {
            status: send_batch_response::Status::Desync as i32,
            database_confirms_last_acked_received_nanos: new_acked_nanos,
        };
    }
    let server_addr = common::spawn_mock_server(mock_service.clone()).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();
    let mut submitter = BatchSubmitter::new(
        "test-collector".to_string(),
        IpAddr::from_str("1.1.1.1").unwrap(),
        Duration::from_secs(60),
        1_000_000,
        Duration::from_secs(24 * 3600),
        db_client,
    );

    // 2. Ingest data that will be sent
    submitter.ingest_ping_result(FinalizedPing {
        sent_nanos: 10000,
        rtt: Some(Duration::from_millis(50)),
    });

    // 3. Trigger send and assert DESYNC handling
    submitter.send_batch().await.unwrap();
    assert_eq!(
        submitter.last_acked_received_nanos, new_acked_nanos,
        "Submitter should update its acked_nanos to the value from the DB on DESYNC"
    );

    // 4. Configure server to return OK now
    let final_acked_nanos = 10050;
    {
        let mut response = mock_service.send_batch_response.lock().unwrap();
        *response = SendBatchResponse {
            status: send_batch_response::Status::Ok as i32,
            database_confirms_last_acked_received_nanos: final_acked_nanos,
        };
    }

    // 5. Trigger send again. The previously sent record should be sent again because
    // the new acked_nanos (5000) is less than its received_nanos (10050).
    submitter.send_batch().await.unwrap();
    let received_batches = mock_service.received_batches.lock().unwrap();
    assert_eq!(received_batches.len(), 2, "Should have sent a second batch");
    assert_eq!(
        received_batches[1]
            .collector_believes_last_acked_received_nanos,
        new_acked_nanos,
        "The second batch should use the corrected acked_nanos value"
    );
    assert_eq!(
        submitter.last_acked_received_nanos, final_acked_nanos,
        "Submitter should update its acked_nanos after the successful batch"
    );
}


#[tokio::test]
async fn test_prune_by_buffer_limit() {
    let mock_service = MockIngestionService::new();
    let server_addr = common::spawn_mock_server(mock_service).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();
    let mut submitter = BatchSubmitter::new(
        "test-collector".to_string(),
        IpAddr::from_str("1.1.1.1").unwrap(),
        Duration::from_secs(60),
        5, // Set a small buffer limit for the test
        Duration::from_secs(24 * 3600),
        db_client,
    );

    // Ingest 6 records, exceeding the limit of 5
    for i in 0..6 {
        submitter.ingest_ping_result(FinalizedPing {
            sent_nanos: 1000 + (i * 100),
            rtt: Some(Duration::from_millis(50)),
        });
    }

    // The buffer should have pruned the oldest record and contain only 5
    assert_eq!(submitter.buffer.len(), 5);
    // The first record should now be the one with sent_nanos = 1100
    let (first_key, _) = submitter.buffer.iter().next().unwrap();
    assert_eq!(*first_key, 1100 + 50_000_000);
}

#[tokio::test]
async fn test_prune_by_time_retention() {
    let mock_service = MockIngestionService::new();
    let server_addr = common::spawn_mock_server(mock_service).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();
    let mut submitter = BatchSubmitter::new(
        "test-collector".to_string(),
        IpAddr::from_str("1.1.1.1").unwrap(),
        Duration::from_secs(60),
        1_000_000,
        Duration::from_secs(1), // Short retention for test
        db_client,
    );

    // Ingest a record now
    submitter.ingest_ping_result(FinalizedPing {
        sent_nanos: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64,
        rtt: Some(Duration::from_millis(50)),
    });
    assert_eq!(submitter.buffer.len(), 1);

    // Wait for longer than the retention period
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Trigger the pruning logic directly
    submitter.prune_by_time();

    // The buffer should now be empty
    assert_eq!(
        submitter.buffer.len(),
        0,
        "Buffer should be empty after pruning by time"
    );
}

#[tokio::test]
async fn test_prune_by_fsync() {
    let mock_service = MockIngestionService::new();
    let server_addr = common::spawn_mock_server(mock_service).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();
    let mut submitter = BatchSubmitter::new(
        "test-collector".to_string(),
        IpAddr::from_str("1.1.1.1").unwrap(),
        Duration::from_secs(60),
        100,
        Duration::from_secs(24 * 3600),
        db_client,
    );

    // Ingest 5 records
    for i in 0..5 {
        submitter.ingest_ping_result(FinalizedPing {
            sent_nanos: 1000 + (i * 100),
            rtt: Some(Duration::from_millis(50)),
        });
    }
    assert_eq!(submitter.buffer.len(), 5);

    // The keys will be 1050M, 1150M, 1250M, 1350M, 1450M (in nanos)
    // Prune everything up to and including 1250M
    let fsync_nanos = 1000 + (2 * 100) + 50_000_000;
    submitter.prune_by_fsync(fsync_nanos);

    // 2 records should remain
    assert_eq!(submitter.buffer.len(), 2);
    // The first remaining record should be the one with sent_nanos = 1300
    let (first_key, _) = submitter.buffer.iter().next().unwrap();
    assert_eq!(*first_key, 1300 + 50_000_000);
}

#[tokio::test]
async fn test_pruning_precedence() {
    let mock_service = MockIngestionService::new();
    let server_addr = common::spawn_mock_server(mock_service).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();
    let mut submitter = BatchSubmitter::new(
        "test-collector".to_string(),
        IpAddr::from_str("1.1.1.1").unwrap(),
        Duration::from_secs(60),
        5, // Set a small buffer limit for the test
        Duration::from_secs(10), // Time-based pruning is longer
        db_client,
    );

    // Ingest 6 records. All have recent timestamps.
    for i in 0..6 {
        submitter.ingest_ping_result(FinalizedPing {
            sent_nanos: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
                + (i * 100),
            rtt: Some(Duration::from_millis(50)),
        });
    }

    // Assert that the buffer limit was applied immediately on ingest,
    // before the time-based pruning had a chance to run.
    assert_eq!(
        submitter.buffer.len(),
        5,
        "Buffer should be pruned by count immediately upon insertion"
    );
}
