use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming, transport::Server};
use zzping_collector::target_manager::run_target_manager;
use zzping_proto::zzping::{
    IngestRequest, IngestResponse, QueryRequest, QueryResponse,
    ingestion_server::{Ingestion, IngestionServer},
};

#[derive(Default)]
struct MockIngestionService {
    should_fail_stream: bool,
}

#[tonic::async_trait]
impl Ingestion for MockIngestionService {
    type IngestStreamStream = ReceiverStream<Result<IngestResponse, Status>>;
    async fn ingest_stream(
        &self,
        _request: Request<Streaming<IngestRequest>>,
    ) -> Result<Response<Self::IngestStreamStream>, Status> {
        if self.should_fail_stream {
            Err(Status::internal("Test error"))
        } else {
            let (_tx, rx) = mpsc::channel(1);
            Ok(Response::new(ReceiverStream::new(rx)))
        }
    }

    async fn query_data(
        &self,
        _request: Request<QueryRequest>,
    ) -> Result<Response<QueryResponse>, Status> {
        Ok(Response::new(QueryResponse { records: vec![] }))
    }
}

async fn spawn_mock_server(service: MockIngestionService) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        Server::builder()
            .add_service(IngestionServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    addr
}

#[tokio::test]
async fn test_target_manager_reconnects_on_stream_error() {
    let service = MockIngestionService {
        should_fail_stream: true,
    };
    let server_addr = spawn_mock_server(service).await;

    let (ping_tx, ping_rx) = mpsc::channel(100);

    let manager_handle = tokio::spawn(run_target_manager(
        vec![], // empty ca_cert for http
        format!("http://{server_addr}"),
        "test-host".to_string(),
        "1.2.3.4".parse::<IpAddr>().unwrap(),
        "my-secret-token".to_string(),
        ping_rx,
        Duration::from_millis(10),
    ));

    // The manager should try to connect, fail, and then sleep.
    // We'll wait for a bit more than that to ensure it has time to loop.
    let res = tokio::time::timeout(Duration::from_millis(50), manager_handle).await;
    assert!(
        res.is_err(),
        "Target manager should have run forever and not exited"
    );

    drop(ping_tx);
}
