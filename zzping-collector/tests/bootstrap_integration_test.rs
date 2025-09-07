use anyhow::Result;
use std::{io::Write, net::SocketAddr, time::Duration};
use tempfile::NamedTempFile;
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{transport::Server, Request, Response, Status};
use zzping_collector::run_with_config_path;
use zzping_proto::zzping::{
    ingestion_server::{Ingestion, IngestionServer},
    CollectorRole, HeartbeatRequest, HeartbeatResponse, QueryRequest, QueryResponse,
    SendBatchRequest, SendBatchResponse, GetRecentDataRequest, GetRecentDataResponse,
};

// A mock implementation of the Ingestion service for testing.
#[derive(Default)]
struct MockIngestionService {}

#[tonic::async_trait]
impl Ingestion for MockIngestionService {
    async fn heartbeat(
        &self,
        _request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
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

#[tokio::test]
async fn test_full_bootstrap_and_reconnect() {
    // 1. Setup a mock server
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

    // 2. Setup a mock config file
    let config_content = format!(
        r#"
(
    collector_uuid: "integ-test-uuid",
    database_addr: "http://{}",
    auth_token: "test-token",
)
"#,
        addr
    );
    let mut config_file = NamedTempFile::new().unwrap();
    config_file.write_all(config_content.as_bytes()).unwrap();
    let config_path = config_file.path().to_str().unwrap().to_string();

    // 3. Run the collector
    let collector_handle = tokio::spawn(run_with_config_path(config_path));

    // Give it time to connect and receive the first heartbeat.
    // We can't easily assert on logs without more dependencies, but if the
    // collector panics or exits, this test will fail.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 4. Shutdown the server to test reconnection
    shutdown_tx.send(()).unwrap();
    server_handle.await.unwrap();

    // The collector should now be in a retry loop.
    // We check that it doesn't crash.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // 5. Check that the collector is still running
    assert!(
        !collector_handle.is_finished(),
        "Collector crashed after server shutdown"
    );

    // 6. Abort the collector task to clean up the test.
    collector_handle.abort();
}
