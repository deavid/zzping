use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

use log::{debug, info};
use ntest::timeout;
use tokio::sync::mpsc;

use crate::database_client::DatabaseClient;
use crate::pinger::FinalizedPing;

use super::common;
use common::{MockIngestionService, spawn_mock_server};

#[tokio::test]
#[timeout(1000)]
async fn test_batch_submitter_prune_by_fsync_via_command() {
    // Enable test logger so info!/debug! are captured in test output.
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();

    info!("Starting test_batch_submitter_prune_by_fsync_via_command");
    // Setup mock server and DatabaseClient
    let mock = MockIngestionService::with_ping_rate(0);
    let server_addr = spawn_mock_server(mock).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "test-token".to_string())
            .await
            .expect("failed to create DatabaseClient");

    // Create a BatchSubmitter directly (no async run loop) and exercise its
    // ingest/prune methods to avoid concurrent scheduling races in tests.
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let mut submitter = crate::batch_submitter::BatchSubmitter::new(
        "test-uuid".to_string(),
        IpAddr::from_str("127.0.0.1").unwrap(),
        Duration::from_secs(60),
        1000,
        Duration::from_secs(3600),
        db_client,
        cmd_rx,
    );

    // Insert two finalized pings directly
    let older = FinalizedPing {
        sent_nanos: 500,
        rtt: Some(Duration::from_millis(10)),
    };
    let newer = FinalizedPing {
        sent_nanos: 2000,
        rtt: Some(Duration::from_millis(10)),
    };
    submitter.ingest_ping_result(older);
    submitter.ingest_ping_result(newer);
    // Sanity check buffer size before pruning
    info!("Buffer length after ingest: {}", submitter.buffer_len());
    debug!("Buffer contents: {:#?}", submitter.buffer_contents());
    assert_eq!(
        submitter.buffer_len(),
        2,
        "expected 2 buffered records before pruning"
    );

    // Compute expected keys: sent_nanos + rtt.as_nanos()
    // older key = 500 + 10_000_000 = 10_000_500
    // newer key = 2000 + 10_000_000 = 10_002_000
    // Choose fsync between those values to prune only the older record.
    let fsync_cutoff = 10_000_800u64;
    info!("Pruning by fsync cutoff {fsync_cutoff}");
    submitter.prune_by_fsync(fsync_cutoff);
    info!("Buffer length after prune: {}", submitter.buffer_len());
    debug!(
        "Buffer contents after prune: {:#?}",
        submitter.buffer_contents()
    );
    assert_eq!(
        submitter.buffer_len(),
        1,
        "expected 1 buffered record after pruning"
    );
}
