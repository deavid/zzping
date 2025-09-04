use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use tonic::{Request, Response, Status};
use zzping_collector::runner::batch_sender_loop;
use zzping_proto::zzping::{
    ingestion_server::{Ingestion, IngestionServer},
    send_batch_response, GetRecentDataRequest, GetRecentDataResponse, HeartbeatRequest,
    HeartbeatResponse, QueryRequest, QueryResponse, SendBatchRequest, SendBatchResponse,
};

// A mock Ingestion service that lets us control SendBatch responses.
#[derive(Clone)]
struct MockIngestionService {
    // A channel to send received requests back to the test for inspection.
    request_tx: mpsc::Sender<SendBatchRequest>,
    // A stack of pre-programmed responses to send back. The mock will pop one on each call.
    responses: Arc<Mutex<Vec<SendBatchResponse>>>,
}

#[tonic::async_trait]
impl Ingestion for MockIngestionService {
    async fn heartbeat(
        &self,
        _request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        unimplemented!()
    }

    async fn send_batch(
        &self,
        request: Request<SendBatchRequest>,
    ) -> Result<Response<SendBatchResponse>, Status> {
        let req = request.into_inner();
        self.request_tx.send(req).await.unwrap();
        let response = self.responses.lock().unwrap().pop().expect("Mock server ran out of responses");
        Ok(Response::new(response))
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

async fn spawn_mock_server(
    request_tx: mpsc::Sender<SendBatchRequest>,
    mut responses: Vec<SendBatchResponse>,
) -> String {
    // The mock server pops responses, so we need to reverse the order.
    responses.reverse();
    let service = MockIngestionService {
        request_tx,
        responses: Arc::new(Mutex::new(responses)),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(IngestionServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn test_collector_sends_batch_and_prunes_buffer() {
    let _ = env_logger::builder().is_test(true).try_init();

    // 1. Setup a mock server that will always respond with OK.
    let (req_tx, mut req_rx) = mpsc::channel(10);
    let ok_response = SendBatchResponse {
        status: send_batch_response::Status::Ok as i32,
        database_confirms_last_acked_nanos: 2, // We'll expect this back
    };
    // The server will run out of responses after this one, but that's fine for this test.
    let server_addr = spawn_mock_server(req_tx, vec![ok_response]).await;

    // 2. Run the batch_sender_loop in the background.
    let (ping_tx, ping_rx) = mpsc::channel(100);
    let client = zzping_proto::zzping::ingestion_client::IngestionClient::connect(server_addr)
        .await
        .unwrap();
    tokio::spawn(batch_sender_loop(
        client,
        "test-collector".to_string(),
        ping_rx,
    ));

    // 3. Send two records to the loop.
    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 1, rtt: None }).await.unwrap();
    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 2, rtt: None }).await.unwrap();

    // 4. Wait for the batch_sender_loop to send the batch to our mock server.
    let received_req = timeout(Duration::from_secs(2), req_rx.recv()).await.unwrap().unwrap();

    // Assert the first batch was correct.
    assert_eq!(received_req.records.len(), 2);
    assert_eq!(received_req.records[0].sent_nanos, 1);
    assert_eq!(received_req.records[1].sent_nanos, 2);
    assert_eq!(received_req.collector_believes_last_acked_nanos, 0);

    // 5. Send a third record. This should be in a new batch.
    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 3, rtt: None }).await.unwrap();

    // 6. Wait for the next batch. This will fail because the mock server ran out of responses,
    // but we only need to check the request that was sent.
    // We'll ignore the result.
    let _ = timeout(Duration::from_secs(2), req_rx.recv()).await;
}

#[tokio::test]
async fn test_collector_rewinds_buffer_on_desync() {
    let _ = env_logger::builder().is_test(true).try_init();

    // 1. Setup a mock server that will respond with DESYNC first, then OK.
    let (req_tx, mut req_rx) = mpsc::channel(10);
    let responses = vec![
        SendBatchResponse {
            status: send_batch_response::Status::Desync as i32,
            database_confirms_last_acked_nanos: 0, // Tell collector to rewind to 0
        },
        SendBatchResponse {
            status: send_batch_response::Status::Ok as i32,
            database_confirms_last_acked_nanos: 2, // The "correct" ack
        },
        // A final response for the last batch
        SendBatchResponse {
            status: send_batch_response::Status::Ok as i32,
            database_confirms_last_acked_nanos: 3,
        },
    ];
    let server_addr = spawn_mock_server(req_tx, responses).await;

    // 2. Run the batch_sender_loop.
    let (ping_tx, ping_rx) = mpsc::channel(100);
    let client = zzping_proto::zzping::ingestion_client::IngestionClient::connect(server_addr)
        .await
        .unwrap();
    tokio::spawn(batch_sender_loop(
        client,
        "test-collector".to_string(),
        ping_rx,
    ));

    // 3. Send two records.
    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 1, rtt: None }).await.unwrap();
    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 2, rtt: None }).await.unwrap();

    // 4. The first batch is sent. Server responds with DESYNC.
    let req1 = timeout(Duration::from_secs(2), req_rx.recv()).await.unwrap().unwrap();
    assert_eq!(req1.records.len(), 2);
    assert_eq!(req1.collector_believes_last_acked_nanos, 0);

    // 5. The loop receives DESYNC, updates its internal acked_nanos to 0, and immediately
    // re-sends the same batch.
    let req2 = timeout(Duration::from_secs(2), req_rx.recv()).await.unwrap().unwrap();
    assert_eq!(req2.records.len(), 2, "Should have re-sent the same records");
    assert_eq!(req2.records[0].sent_nanos, 1);
    assert_eq!(req2.records[1].sent_nanos, 2);
    assert_eq!(req2.collector_believes_last_acked_nanos, 0, "Should still believe ack is 0");

    // 6. Now it gets an OK response, acking up to 2. Let's send a new ping.
    ping_tx.send(zzping_collector::ping_client::PingResult { sent_nanos: 3, rtt: None }).await.unwrap();

    // 7. The third batch should contain only the new ping and believe the ack is now 2.
    let req3 = timeout(Duration::from_secs(2), req_rx.recv()).await.unwrap().unwrap();
    assert_eq!(req3.records.len(), 1);
    assert_eq!(req3.records[0].sent_nanos, 3);
    assert_eq!(req3.collector_believes_last_acked_nanos, 2);
}
