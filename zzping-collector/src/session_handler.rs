use crate::database_client::DatabaseClient;
use crate::task_supervisor::SupervisorConfig;
use anyhow::Result;
use log::{error, info};
use std::time::Duration;
use tokio::sync::watch;
use zzping_proto::zzping::{CollectorRole, HeartbeatRequest};

/// An ephemeral task that manages all gRPC communication for the duration of
/// a single, healthy connection. It dies gracefully on any network error.
pub struct SessionHandler {
    /// The gRPC client for this session.
    client: DatabaseClient,
    /// The sender for broadcasting configuration updates.
    config_tx: watch::Sender<Option<SupervisorConfig>>,
    /// The UUID of this collector.
    collector_uuid: String,
}

impl SessionHandler {
    /// Creates a new `SessionHandler`.
    pub fn new(
        client: DatabaseClient,
        config_tx: watch::Sender<Option<SupervisorConfig>>,
        collector_uuid: String,
    ) -> Self {
        Self {
            client,
            config_tx,
            collector_uuid,
        }
    }

    /// Runs the `SessionHandler`'s main loop.
    pub async fn run(mut self) -> Result<()> {
        info!("SessionHandler started.");
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            interval.tick().await;

            let request = HeartbeatRequest {
                collector_uuid: self.collector_uuid.clone(),
                pid: std::process::id() as u64,
            };

            match self.client.heartbeat(request).await {
                Ok(response) => {
                    let response = response.into_inner();
                    // This conversion will be more complex later.
                    let role = CollectorRole::try_from(response.role)
                        .unwrap_or(CollectorRole::Standby);

                    let config = SupervisorConfig {
                        // For now, we just use a placeholder.
                        // In the future, this will be populated from the response.
                        placeholder: format!("Role: {:?}, Targets: {:?}", role, response.targets),
                    };

                    if self.config_tx.send(Some(config)).is_err() {
                        // The receiver was dropped, so we can shut down.
                        info!("Config channel closed. SessionHandler shutting down.");
                        break;
                    }
                }
                Err(e) => {
                    error!("Heartbeat RPC failed: {}. Session ending.", e);
                    // Any gRPC error is considered fatal for the session.
                    // The task will terminate, signaling the CollectorService
                    // to request a new connection.
                    break;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database_client::tests::spawn_mock_server;

    #[tokio::test]
    async fn test_session_handler_sends_config_on_success() {
        // This test verifies that if the heartbeat call is successful, the
        // SessionHandler correctly translates the response and sends it
        // over the watch channel.

        // 1. Setup
        let (config_tx, mut config_rx) = watch::channel(None);
        let server_addr = spawn_mock_server().await;
        let client = DatabaseClient::connect(
            format!("http://{}", server_addr),
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

        // 1. Setup
        let (config_tx, _) = watch::channel(None);
        // Use an address that is guaranteed to not have a server running.
        let _client =
            DatabaseClient::connect("http://127.0.0.1:0".to_string(), "test-token".to_string())
                .await;

        // The connection itself will fail, so we can't create a handler.
        // This test case is implicitly covered by the ConnectionManager, which
        // wouldn't have created the SessionHandler in the first place.
        // However, if the connection drops *during* the loop, the handler should exit.

        // To test this, we start a server, let the handler connect, then stop the server.
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(zzping_proto::zzping::ingestion_server::IngestionServer::new(crate::database_client::tests::MockIngestionService::default()))
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::TcpListenerStream::new(listener),
                    async {
                        shutdown_rx.await.ok();
                    },
                )
                .await
                .unwrap();
        });

        let client = DatabaseClient::connect(format!("http://{}", addr), "test-token".to_string())
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
}
