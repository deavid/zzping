use anyhow::Result;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;
use zzping_proto::zzping::{
    ingestion_client::IngestionClient, HeartbeatRequest, HeartbeatResponse,
};

/// A lightweight, cloneable wrapper around the `tonic` gRPC client that
/// centralizes request creation and authentication logic.
#[derive(Clone)]
pub struct DatabaseClient {
    /// The underlying gRPC client.
    client: IngestionClient<Channel>,
    /// The authentication token for this collector.
    auth_token: String,
}

impl DatabaseClient {
    /// Connects to the database and creates a new `DatabaseClient`.
    ///
    /// # Arguments
    ///
    /// * `addr` - The address of the database gRPC server.
    /// * `auth_token` - The authentication token for this collector.
    pub async fn connect(addr: String, auth_token: String) -> Result<Self> {
        let endpoint = Endpoint::from_shared(addr)?;
        let channel = endpoint.connect().await?;
        let client = IngestionClient::new(channel);

        Ok(Self { client, auth_token })
    }

    /// Performs a Heartbeat RPC.
    ///
    /// # Arguments
    ///
    /// * `request` - The `HeartbeatRequest` to send.
    pub async fn heartbeat(
        &mut self,
        request: HeartbeatRequest,
    ) -> Result<tonic::Response<HeartbeatResponse>> {
        let mut tonic_request = Request::new(request);

        // Add the authentication token to the request metadata.
        let token = format!("Bearer {}", self.auth_token);
        tonic_request
            .metadata_mut()
            .insert("authorization", token.parse()?);

        let response = self.client.heartbeat(tonic_request).await?;
        Ok(response)
    }

    // `send_batch` and other RPC wrappers will be added in later steps.
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{transport::Server, Response, Status};
    use zzping_proto::zzping::{
        ingestion_server::{Ingestion, IngestionServer},
        CollectorRole, QueryRequest, QueryResponse, SendBatchRequest, SendBatchResponse,
        GetRecentDataRequest, GetRecentDataResponse,
    };

    // A mock implementation of the Ingestion service for testing.
    #[derive(Default)]
    pub struct MockIngestionService {
        // We can add channels here to assert on received requests.
    }

    #[tonic::async_trait]
    impl Ingestion for MockIngestionService {
        async fn heartbeat(
            &self,
            request: Request<HeartbeatRequest>,
        ) -> Result<Response<HeartbeatResponse>, Status> {
            // Check for the auth token in the metadata.
            let metadata = request.metadata();
            if metadata.get("authorization").is_none()
                || metadata.get("authorization").unwrap() != "Bearer test-token"
            {
                return Err(Status::unauthenticated("Missing or invalid auth token"));
            }

            // Simulate a successful response.
            let response = HeartbeatResponse {
                targets: vec!["8.8.8.8".to_string()],
                ping_rate_pps: 100,
                role: CollectorRole::Primary as i32,
                swap_at_nanos: 0,
            };
            Ok(Response::new(response))
        }

        async fn send_batch(
            &self,
            _request: Request<SendBatchRequest>,
        ) -> Result<Response<SendBatchResponse>, Status> {
            unimplemented!()
        }

        async fn get_recent_data(
            &self,
            _request: Request<GetRecentDataRequest>,
        ) -> Result<Response<GetRecentDataResponse>, Status> {
            unimplemented!()
        }

        async fn query_data(
            &self,
            _request: Request<QueryRequest>,
        ) -> Result<Response<QueryResponse>, Status> {
            unimplemented!()
        }
    }

    // Helper to spawn a mock server and get its address.
    pub async fn spawn_mock_server() -> SocketAddr {
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
    async fn test_database_client_connect_and_heartbeat() {
        let addr = spawn_mock_server().await;
        let client_addr = format!("http://{}", addr);

        // Test successful connection and heartbeat.
        let mut client = DatabaseClient::connect(client_addr.clone(), "test-token".to_string())
            .await
            .unwrap();

        let request = HeartbeatRequest {
            collector_uuid: "test-uuid".to_string(),
            pid: 1234,
        };
        let response = client.heartbeat(request).await;
        assert!(response.is_ok());
        let response = response.unwrap().into_inner();
        assert_eq!(response.ping_rate_pps, 100);
        assert_eq!(response.role, CollectorRole::Primary as i32);

        // Test with a bad token.
        let mut bad_client = DatabaseClient::connect(client_addr, "bad-token".to_string())
            .await
            .unwrap();
        let request = HeartbeatRequest {
            collector_uuid: "test-uuid".to_string(),
            pid: 1234,
        };
        let response = bad_client.heartbeat(request).await;
        assert!(response.is_err());
        let err = response.unwrap_err();
        let status = err.downcast_ref::<tonic::Status>().unwrap();
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
    }
}
