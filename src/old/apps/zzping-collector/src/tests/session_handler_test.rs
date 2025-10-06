use crate::{
    collector_service::CachedIntent,
    database_client::DatabaseClient,
    session_handler::SessionHandler,
    task_supervisor::{HealthReport, SupervisorConfig},
};
use log::info;
use ntest::timeout;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use zzping_proto::zzping::CollectorRole;
// Arc not needed in this test file

use super::common;
use common::{MockIngestionService, spawn_mock_server};

#[tokio::test]
#[timeout(3000)]
async fn test_session_handler_sends_config_on_success() {
    // This test verifies that if the heartbeat call is successful, the
    // SessionHandler correctly translates the response and sends it
    // over the watch channel.
    common::setup_logger();
    // 1. Setup
    let (config_tx, mut config_rx) = watch::channel::<Option<SupervisorConfig>>(None);
    let (health_tx, health_rx) = watch::channel(HealthReport {
        total_buffer_size: 0,
        role: CollectorRole::Standby,
        fatal_errors: vec![],
    });
    // Send an initial health report to unblock the heartbeat loop
    health_tx.send_replace(HealthReport {
        total_buffer_size: 123,
        role: CollectorRole::Primary,
        fatal_errors: vec!["Everything is on fire".to_string()],
    });

    let (persistence_tx, _persistence_rx) = mpsc::channel::<CachedIntent>(1);

    let server_addr = spawn_mock_server(MockIngestionService::with_ping_rate(0)).await;
    let client = DatabaseClient::connect(format!("http://{server_addr}"), "test-token".to_string())
        .await
        .unwrap();

    let (fsync_tx, _fsync_rx) = mpsc::channel::<u64>(10);
    let handler = SessionHandler::new(
        client,
        config_tx,
        "test-uuid".to_string(),
        health_rx,
        persistence_tx,
        fsync_tx,
        true, // Use mock client in tests
        None, // No cache file in tests for hermeticity
    );

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

    // Check that the config contains data from the mock response
    assert_eq!(config.ping_rate_pps, 0);
    assert!(config.targets.contains(&"127.0.0.1".parse().unwrap()));
}

#[tokio::test]
#[timeout(3000)]
async fn test_session_handler_exits_on_connection_failure() {
    // Initialize env_logger for debug output
    common::setup_logger();

    info!("Starting test_session_handler_exits_on_connection_failure");

    // This test verifies that the handler's run loop terminates
    // when the database connection fails.

    // To test this, we start a server, let the handler connect, then stop the server.
    let (config_tx, _) = watch::channel(None);
    let (health_tx, health_rx) = watch::channel(HealthReport {
        total_buffer_size: 0,
        role: CollectorRole::Standby,
        fatal_errors: vec![],
    });
    let (persistence_tx, _persistence_rx) = mpsc::channel::<CachedIntent>(1);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    info!("Starting mock server on {addr}");

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
    info!("Database client connected to {addr}");

    let (fsync_tx, _fsync_rx) = mpsc::channel::<u64>(10);
    let handler = SessionHandler::new(
        client,
        config_tx,
        "test-uuid".to_string(),
        health_rx,
        persistence_tx,
        fsync_tx,
        true, // Use mock client in tests
        None, // No cache file in tests for hermeticity
    );
    let handler_handle = tokio::spawn(handler.run());
    info!("SessionHandler started");

    // Send a health report to trigger the heartbeat loop
    health_tx.send_replace(HealthReport {
        total_buffer_size: 123,
        role: CollectorRole::Primary,
        fatal_errors: vec![],
    });
    info!("Health report sent");

    // Let it run once successfully with a shorter wait
    tokio::time::sleep(Duration::from_millis(10)).await;
    info!("Initial wait completed");

    // Now, shut down the server
    info!("Shutting down server");
    shutdown_tx.send(()).unwrap();
    server_handle.await.unwrap();
    info!("Server shutdown complete");

    // The handler should now exit gracefully.
    info!("Waiting for SessionHandler to exit");
    let result = tokio::time::timeout(Duration::from_millis(500), handler_handle).await;
    match result {
        Ok(handler_result) => {
            info!("SessionHandler exited within timeout");
            // Check if the handler finished (successfully or with error)
            match handler_result {
                Ok(session_result) => {
                    // The session should exit when connection fails, regardless of success/error
                    match session_result {
                        Ok(_) => {
                            info!("SessionHandler exited successfully as expected");
                        }
                        Err(e) => {
                            info!("SessionHandler exited with error (also acceptable): {e}");
                        }
                    }
                }
                Err(join_error) => panic!("Handler task panicked: {join_error}"),
            }
        }
        Err(_) => {
            info!(
                "SessionHandler did not exit within timeout - this is the bug we're investigating"
            );
            panic!("SessionHandler did not exit after server shutdown within timeout");
        }
    }

    info!("Test completed");
}

#[tokio::test]
#[timeout(3000)]
async fn test_session_handler_reports_last_processed_command_in_heartbeat() {
    common::setup_logger();
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();

    let (config_tx, _) = watch::channel::<Option<SupervisorConfig>>(None);
    let (health_tx, health_rx) = watch::channel(HealthReport {
        total_buffer_size: 0,
        role: CollectorRole::Standby,
        fatal_errors: vec![],
    });
    health_tx.send_replace(HealthReport {
        total_buffer_size: 123,
        role: CollectorRole::Primary,
        fatal_errors: vec![],
    });

    let (persistence_tx, mut persistence_rx) = mpsc::channel::<CachedIntent>(1);

    // Keep the persistence receiver alive to prevent the state manager from hanging
    tokio::spawn(async move {
        while let Some(_intent) = persistence_rx.recv().await {
            // Just consume and discard persistence messages
        }
    });

    // Prepare mock server with ability to push a command
    let mock = MockIngestionService::with_ping_rate(0);
    let server_addr = spawn_mock_server(mock.clone()).await;
    let client = DatabaseClient::connect(format!("http://{server_addr}"), "test-token".to_string())
        .await
        .unwrap();

    let (fsync_tx, _fsync_rx) = mpsc::channel::<u64>(10);
    let handler = SessionHandler::new(
        client.clone(),
        config_tx,
        "test-uuid".to_string(),
        health_rx,
        persistence_tx,
        fsync_tx,
        true,
        None, // No cache file in tests for hermeticity
    );

    // Run handler
    tokio::spawn(handler.run());

    // Wait for subscription to establish - poll until command_stream_tx is available
    let mut max_attempts = 50; // 50 attempts * 10ms = 500ms max wait
    while max_attempts > 0 {
        if mock.command_stream_tx.lock().unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        max_attempts -= 1;
    }

    // Ensure command stream is available
    assert!(
        mock.command_stream_tx.lock().unwrap().is_some(),
        "Command stream subscription not established within 500ms"
    );

    // Send a command on the mock command stream with id 42
    if let Some(tx) = &*mock.command_stream_tx.lock().unwrap() {
        let cmd = zzping_proto::zzping::Command {
            command_id: 42,
            command_type: Some(zzping_proto::zzping::command::CommandType::ChangeRole(
                zzping_proto::zzping::CollectorRole::Primary as i32,
            )),
        };
        let send_result = tx.clone().try_send(Ok(cmd));
        assert!(
            send_result.is_ok(),
            "Failed to send command: {:?}",
            send_result
        );
        println!("✓ Command sent successfully");
    } else {
        panic!("Command stream not available");
    }

    // Wait for the command to be processed and heartbeat to be sent
    tokio::time::sleep(Duration::from_millis(200)).await;

    let received = mock.received_heartbeats.lock().unwrap().clone();
    assert!(
        !received.is_empty(),
        "No heartbeats received by mock server"
    );

    println!("✓ Received {} heartbeats", received.len());

    // The last heartbeat should include last_processed_command_id == 42
    let last = received.last().unwrap();
    println!(
        "Last heartbeat: last_processed_command_id = {}",
        last.last_processed_command_id
    );

    // Wait longer to ensure command is processed
    tokio::time::sleep(Duration::from_millis(800)).await;

    // Check again for more heartbeats
    let received = mock.received_heartbeats.lock().unwrap().clone();
    println!(
        "✓ Total received {} heartbeats after waiting",
        received.len()
    );

    if let Some(latest) = received.last() {
        println!(
            "Latest heartbeat: last_processed_command_id = {}",
            latest.last_processed_command_id
        );
        assert_eq!(latest.last_processed_command_id, 42);
    } else {
        panic!("No heartbeats received");
    }
}
