use crate::database_client::DatabaseClient;
use log::{info, warn};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, Notify};

/// A task that relentlessly and resiliently provides healthy database connections.
pub struct ConnectionManager {
    /// The address of the database gRPC server.
    addr: String,
    /// The authentication token for this collector.
    auth_token: String,
    /// The channel to send new `DatabaseClient` handles to.
    client_tx: mpsc::Sender<DatabaseClient>,
    /// A channel used by the `CollectorService` to signal that we need to
    /// create a new connection because the previous session has ended.
    reconnect_notify: Arc<Notify>,
    /// An optional channel used in tests to signal that a connection
    /// attempt is being made.
    #[cfg(feature = "test-utils")]
    connect_notify: Option<Arc<Notify>>,
}

impl ConnectionManager {
    /// Creates a new `ConnectionManager`.
    pub fn new(
        addr: String,
        auth_token: String,
        client_tx: mpsc::Sender<DatabaseClient>,
        reconnect_notify: Arc<Notify>,
    ) -> Self {
        Self {
            addr,
            auth_token,
            client_tx,
            reconnect_notify,
            #[cfg(feature = "test-utils")]
            connect_notify: None,
        }
    }

    /// Injects a notifier for testing purposes to observe connection attempts.
    #[cfg(feature = "test-utils")]
    pub fn set_connect_notify(&mut self, notify: Arc<Notify>) {
        self.connect_notify = Some(notify);
    }

    /// Runs the `ConnectionManager`'s infinite connect/retry loop.
    pub async fn run(self) {
        info!("ConnectionManager started.");
        loop {
            // Notify tests that we are about to attempt a connection.
            #[cfg(feature = "test-utils")]
            if let Some(notify) = &self.connect_notify {
                notify.notify_one();
            }

            info!("Attempting to connect to database at {}...", self.addr);
            match DatabaseClient::connect(self.addr.clone(), self.auth_token.clone()).await {
                Ok(client) => {
                    info!("Successfully connected to database.");
                    if self.client_tx.send(client).await.is_err() {
                        // The receiver was dropped, which means the CollectorService has shut down.
                        // We can exit the loop.
                        info!("Client channel closed. ConnectionManager shutting down.");
                        break;
                    }
                    // Wait for the CollectorService to signal that the session has ended
                    // and we need to create a new connection.
                    self.reconnect_notify.notified().await;
                    info!("Session ended. Reconnecting...");
                }
                Err(e) => {
                    warn!(
                        "Failed to connect to database: {}. Retrying in 2 seconds...",
                        e
                    );
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database_client::tests::spawn_mock_server;
    use std::time::Duration;
    use tokio::sync::{mpsc, Notify};

    #[tokio::test]
    async fn test_connection_manager_connects_and_sends_client() {
        let addr = spawn_mock_server().await;
        let client_addr = format!("http://{}", addr);
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
}
