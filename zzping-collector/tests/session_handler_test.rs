use std::time::Duration;
use tokio::sync::watch;
use zzping_collector::{database_client::DatabaseClient, session_handler::SessionHandler};

mod common;
use common::{spawn_mock_server, MockIngestionService};

#[tokio::test]
async fn test_session_handler_sends_config_on_success() {
    // This test verifies that if the heartbeat call is successful, the
    // SessionHandler correctly translates the response and sends it
    // over the watch channel.

    // 1. Setup
    let (config_tx, mut config_rx) = watch::channel(None);
    let server_addr = spawn_mock_server().await;
    let client = DatabaseClient::connect(
        format!("http://{server_addr}"),
        "test-token".to_string(),
    )
    .await
    .unwrap();

    let handler = SessionHandler::new(client, config_tx, "test-uuid".to_string());

    // 2. Run the handler in a separate task
    tokio::spawn(handler.run());

    // 3. Assert
    // The handler's loop should run, call the mock server, and send the config.
    // We should receive the config on our end.
    let result = tokio::time::timeout(Duration::from_secs(2), config_rx.changed()).await;
    assert!(result.is_ok(), "Did not receive config within timeout");

    let received_config = config_rx.borrow().clone();
    assert!(received_config.is_some());
    let config = received_config.unwrap();

    // Check that the placeholder string contains data from the mock response
    assert!(config.placeholder.contains("Primary"));
    assert!(config.placeholder.contains("8.8.8.8"));
}

#[tokio::test]
async fn test_session_handler_exits_on_connection_failure() {
    // This test verifies that the handler's run loop terminates
    // when the database connection fails.

    // To test this, we start a server, let the handler connect, then stop the server.
    let (config_tx, _) = watch::channel(None);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                zzping_proto::zzping::ingestion_server::IngestionServer::new(
                    MockIngestionService::default(),
                ),
            )
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    shutdown_rx.await.ok();
                },
            )
            .await
            .unwrap();
    });

    let client = DatabaseClient::connect(format!("http://{addr}"), "test-token".to_string())
        .await
        .unwrap();

    let handler = SessionHandler::new(client, config_tx, "test-uuid".to_string());
    let handler_handle = tokio::spawn(handler.run());

    // Let it run once successfully
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // Now, shut down the server
    shutdown_tx.send(()).unwrap();
    server_handle.await.unwrap();

    // The handler should now exit gracefully.
    let result = tokio::time::timeout(Duration::from_secs(2), handler_handle).await;
    assert!(result.is_ok(), "SessionHandler did not exit after server shutdown");
}
