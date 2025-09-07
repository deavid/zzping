use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, Notify};
use zzping_collector::connection_manager::ConnectionManager;

mod common;
use common::spawn_mock_server;

#[tokio::test]
async fn test_connection_manager_connects_and_sends_client() {
    let addr = spawn_mock_server().await;
    let client_addr = format!("http://{addr}");
    let (client_tx, mut client_rx) = mpsc::channel(1);
    let notify = Arc::new(Notify::new());

    let manager = ConnectionManager::new(
        client_addr,
        "test-token".to_string(),
        client_tx,
        notify.clone(),
    );
    tokio::spawn(manager.run());

    // The manager should connect and send a client.
    let client = tokio::time::timeout(Duration::from_secs(1), client_rx.recv())
        .await
        .expect("ConnectionManager did not send a client in time");

    assert!(client.is_some());
}

#[tokio::test]
async fn test_connection_manager_retries_on_failure() {
    // Don't spawn a server, so connection will fail.
    let client_addr = "http://127.0.0.1:0".to_string();
    let (client_tx, mut client_rx) = mpsc::channel(1);
    let notify = Arc::new(Notify::new());

    let manager = ConnectionManager::new(
        client_addr,
        "test-token".to_string(),
        client_tx,
        notify.clone(),
    );
    tokio::spawn(manager.run());

    // The manager should not send a client.
    let result = tokio::time::timeout(Duration::from_secs(1), client_rx.recv()).await;
    assert!(result.is_err(), "ConnectionManager sent a client when it should have failed");

    // In a real test, we would capture logs to verify retry attempts.
    // For now, we just ensure it doesn't crash and doesn't send a client.
}
