use ntest::timeout;
use std::{io::Write, sync::{Arc, Mutex}, time::Duration};
use tempfile::NamedTempFile;
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use zzping_collector::run_with_config_path;
use zzping_proto::zzping::ingestion_server::IngestionServer;

// Import the common test utilities
mod common;
use common::{MockIngestionService, VectorLogger};

/// This is the primary integration test for the collector's resilience.
/// It verifies that the collector can:
/// 1. Start up and connect to the database.
/// 2. Survive a database connection failure.
/// 3. Actively attempt to reconnect after the failure.
#[tokio::test]
#[timeout(11000)]
async fn test_collector_survives_disconnect_and_reconnects() {
    // 1. Setup a logger to capture output.
    let log_messages = Arc::new(Mutex::new(Vec::new()));
    VectorLogger::init(log_messages.clone()).unwrap();

    // 2. Setup a mock server
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(IngestionServer::new(MockIngestionService::default()))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });

    // 3. Setup a mock config file
    let config_content = format!(
        r#"
(
    collector_uuid: "integ-test-uuid",
    database_addr: "http://{addr}",
    auth_token: "test-token",
)
"#
    );
    let mut config_file = NamedTempFile::new().unwrap();
    config_file.write_all(config_content.as_bytes()).unwrap();
    let config_path = config_file.path().to_str().unwrap().to_string();

    // 4. Run the collector
    let collector_handle = tokio::spawn(run_with_config_path(config_path));

    // 5. Wait for the initial connection.
    let check_logs = || log_messages.lock().unwrap().clone();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if check_logs().iter().any(|s| s.contains("Attempting to connect")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.expect("Collector did not connect initially.");

    // 6. Shutdown the server to test reconnection
    shutdown_tx.send(()).unwrap();
    server_handle.await.unwrap();

    // 7. Wait for the reconnection attempt.
    // The collector should notice the session died and try again.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            // We expect to see "Session ended" followed by another "Attempting to connect".
            let logs = check_logs();
            let session_ended = logs.iter().any(|s| s.contains("Session ended"));
            let reconnect_attempt = logs.iter().filter(|s| s.contains("Attempting to connect")).count() >= 2;
            if session_ended && reconnect_attempt {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.expect("Collector did not attempt to reconnect after server shutdown.");


    // 8. Abort the collector task to clean up the test.
    collector_handle.abort();
}
