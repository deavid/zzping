use crate::database_client::DatabaseClient;
use anyhow::Result;
use log::{info, warn};
use std::time::Duration;
use tokio::sync::mpsc;

/// A task that relentlessly and resiliently provides healthy database connections.
pub struct ConnectionManager {
    /// The address of the database gRPC server.
    addr: String,
    /// The authentication token for this collector.
    auth_token: String,
    /// The channel to send new `DatabaseClient` handles to.
    client_tx: mpsc::Sender<DatabaseClient>,
}

impl ConnectionManager {
    /// Creates a new `ConnectionManager`.
    pub fn new(
        addr: String,
        auth_token: String,
        client_tx: mpsc::Sender<DatabaseClient>,
    ) -> Self {
        Self {
            addr,
            auth_token,
            client_tx,
        }
    }

    /// Runs the `ConnectionManager`'s infinite connect/retry loop.
    pub async fn run(self) {
        info!("ConnectionManager started.");
        loop {
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
                    // Wait for the receiver to be dropped, which signals that the session has ended
                    // and we should try to reconnect.
                    self.client_tx.closed().await;
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
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;
    use zzping_proto::zzping::{
        ingestion_server::{Ingestion, IngestionServer},
        HeartbeatRequest, HeartbeatResponse, QueryRequest, QueryResponse, SendBatchRequest,
        SendBatchResponse, GetRecentDataRequest, GetRecentDataResponse,
    };

    #[derive(Default)]
    struct MockIngestionService {}

    #[tonic::async_trait]
    impl Ingestion for MockIngestionService {
        async fn heartbeat(
            &self,
            _request: tonic::Request<HeartbeatRequest>,
        ) -> Result<tonic::Response<HeartbeatResponse>, tonic::Status> {
            unimplemented!()
        }
        async fn send_batch(
            &self,
            _request: tonic::Request<SendBatchRequest>,
        ) -> Result<tonic::Response<SendBatchResponse>, tonic::Status> {
            unimplemented!()
        }
        async fn get_recent_data(
            &self,
            _request: tonic::Request<GetRecentDataRequest>,
        ) -> Result<tonic::Response<GetRecentDataResponse>, tonic::Status> {
            unimplemented!()
        }
        async fn query_data(
            &self,
            _request: tonic::Request<QueryRequest>,
        ) -> Result<tonic::Response<QueryResponse>, tonic::Status> {
            unimplemented!()
        }
    }

    async fn spawn_mock_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service = MockIngestionService::default();
        let server = IngestionServer::new(service);

        tokio::spawn(async move {
            Server::builder()
                .add_service(server)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        addr
    }

    #[tokio::test]
    async fn test_connection_manager_connects_and_sends_client() {
        let addr = spawn_mock_server().await;
        let client_addr = format!("http://{}", addr);
        let (client_tx, mut client_rx) = mpsc::channel(1);

        let manager =
            ConnectionManager::new(client_addr, "test-token".to_string(), client_tx);
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

        let manager =
            ConnectionManager::new(client_addr, "test-token".to_string(), client_tx);
        tokio::spawn(manager.run());

        // The manager should not send a client.
        let result = tokio::time::timeout(Duration::from_secs(1), client_rx.recv()).await;
        assert!(result.is_err(), "ConnectionManager sent a client when it should have failed");

        // In a real test, we would capture logs to verify retry attempts.
        // For now, we just ensure it doesn't crash and doesn't send a client.
    }
}
