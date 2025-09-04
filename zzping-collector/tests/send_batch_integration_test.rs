use ntest::timeout;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::Duration;
use tonic::{Request, Response, Status};
use zzping_collector::runner::batch_sender_loop;
use zzping_proto::zzping::{
    ingestion_server::{Ingestion, IngestionServer},
    send_batch_response, GetRecentDataRequest, GetRecentDataResponse, HeartbeatRequest,
    HeartbeatResponse, QueryRequest, QueryResponse, SendBatchRequest, SendBatchResponse,
};

#[derive(Clone)]
struct MockIngestionService {
    request_tx: mpsc::Sender<SendBatchRequest>,
    responses: Arc<Mutex<Vec<SendBatchResponse>>>,
}

#[tonic::async_trait]
impl Ingestion for MockIngestionService {
    async fn heartbeat(&self, _request: Request<HeartbeatRequest>) -> Result<Response<HeartbeatResponse>, Status> {
        unimplemented!()
    }

    async fn send_batch(&self, request: Request<SendBatchRequest>) -> Result<Response<SendBatchResponse>, Status> {
        let req = request.into_inner();
        self.request_tx.send(req).await.unwrap();
        let response = self.responses.lock().unwrap().pop().expect("Mock server ran out of responses");
        Ok(Response::new(response))
    }

    async fn get_recent_data(&self, _request: Request<GetRecentDataRequest>) -> Result<Response<GetRecentDataResponse>, Status> {
        unimplemented!()
    }

    async fn query_data(&self, _request: Request<QueryRequest>) -> Result<Response<QueryResponse>, Status> {
        unimplemented!()
    }
}

async fn spawn_mock_server(
    request_tx: mpsc::Sender<SendBatchRequest>,
    mut responses: Vec<SendBatchResponse>,
) -> String {
    responses.reverse();
    let service = MockIngestionService {
        request_tx,
        responses: Arc::new(Mutex::new(responses)),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(IngestionServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await;
    });

    format!("http://{}", addr)
}

#[tokio::test]
#[timeout(5000)]
async fn test_collector_sends_batch_and_prunes_buffer() {
    let _ = env_logger::builder().is_test(true).try_init();

    let (req_tx, mut req_rx) = mpsc::channel(10);
    let ok_response = SendBatchResponse {
        status: send_batch_response::Status::Ok as i32,
        database_confirms_last_acked_nanos: 2,
    };
    let server_addr = spawn_mock_server(req_tx, vec![ok_response]).await;

    let (ping_tx, ping_rx) = mpsc::channel(100);
    let client = zzping_proto::zzping::ingestion_client::IngestionClient::connect(server_addr).await.unwrap();
    tokio::spawn(batch_sender_loop(client, "test-collector".to_string(), ping_rx));

    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 1, rtt: None }).await.unwrap();
    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 2, rtt: None }).await.unwrap();

    let received_req = tokio::time::timeout(Duration::from_secs(2), req_rx.recv()).await.unwrap().unwrap();
    assert_eq!(received_req.records.len(), 2);
    assert_eq!(received_req.collector_believes_last_acked_nanos, 0);

    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 3, rtt: None }).await.unwrap();

    let _ = tokio::time::timeout(Duration::from_secs(2), req_rx.recv()).await;
}

#[tokio::test]
#[timeout(5000)]
async fn test_collector_rewinds_buffer_on_desync() {
    let _ = env_logger::builder().is_test(true).try_init();

    let (req_tx, mut req_rx) = mpsc::channel(10);
    let responses = vec![
        SendBatchResponse { status: send_batch_response::Status::Desync as i32, database_confirms_last_acked_nanos: 0 },
        SendBatchResponse { status: send_batch_response::Status::Ok as i32, database_confirms_last_acked_nanos: 2 },
        SendBatchResponse { status: send_batch_response::Status::Ok as i32, database_confirms_last_acked_nanos: 3 },
    ];
    let server_addr = spawn_mock_server(req_tx, responses).await;

    let (ping_tx, ping_rx) = mpsc::channel(100);
    let client = zzping_proto::zzping::ingestion_client::IngestionClient::connect(server_addr).await.unwrap();
    tokio::spawn(batch_sender_loop(client, "test-collector".to_string(), ping_rx));

    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 1, rtt: None }).await.unwrap();
    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 2, rtt: None }).await.unwrap();

    let req1 = tokio::time::timeout(Duration::from_secs(2), req_rx.recv()).await.unwrap().unwrap();
    assert_eq!(req1.records.len(), 2);
    assert_eq!(req1.collector_believes_last_acked_nanos, 0);

    let req2 = tokio::time::timeout(Duration::from_secs(2), req_rx.recv()).await.unwrap().unwrap();
    assert_eq!(req2.records.len(), 2);
    assert_eq!(req2.collector_believes_last_acked_nanos, 0);

    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 3, rtt: None }).await.unwrap();

    let req3 = tokio::time::timeout(Duration::from_secs(2), req_rx.recv()).await.unwrap().unwrap();
    assert_eq!(req3.records.len(), 1);
    assert_eq!(req3.records[0].sent_nanos, 3);
    assert_eq!(req3.collector_believes_last_acked_nanos, 2);
}
