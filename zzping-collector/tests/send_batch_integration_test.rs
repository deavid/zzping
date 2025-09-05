use ntest::timeout;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::Duration;
use zzping_collector::batch_submitter::BatchSubmitter;
use zzping_collector::database_client::{DatabaseClient, SharedDatabaseClient};
use zzping_database::auth::generate_test_token;
use zzping_proto::zzping::{
    GetRecentDataRequest, GetRecentDataResponse, HeartbeatRequest, HeartbeatResponse,
    SendBatchRequest, SendBatchResponse, send_batch_response,
};

#[derive(Clone)]
struct MockDatabaseClient {
    request_tx: mpsc::Sender<SendBatchRequest>,
    responses: Arc<Mutex<Vec<SendBatchResponse>>>,
}

#[async_trait::async_trait]
impl DatabaseClient for MockDatabaseClient {
    async fn heartbeat(
        &self,
        _req: tonic::Request<HeartbeatRequest>,
    ) -> anyhow::Result<HeartbeatResponse> {
        unimplemented!()
    }

    async fn send_batch(
        &self,
        req: tonic::Request<SendBatchRequest>,
    ) -> anyhow::Result<SendBatchResponse> {
        let req_inner = req.into_inner();
        self.request_tx.send(req_inner).await.unwrap();
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop()
            .expect("Mock server ran out of responses");
        Ok(response)
    }

    async fn get_recent_data(
        &self,
        _req: tonic::Request<GetRecentDataRequest>,
    ) -> anyhow::Result<GetRecentDataResponse> {
        unimplemented!()
    }
}

async fn create_mock_client(
    request_tx: mpsc::Sender<SendBatchRequest>,
    mut responses: Vec<SendBatchResponse>,
) -> SharedDatabaseClient {
    responses.reverse();
    let mock_client = MockDatabaseClient {
        request_tx,
        responses: Arc::new(Mutex::new(responses)),
    };
    Arc::new(tokio::sync::Mutex::new(mock_client))
}

#[tokio::test]
#[timeout(2000)]
async fn test_collector_sends_batch_and_prunes_buffer() {
    let _ = env_logger::builder().is_test(true).try_init();

    let (req_tx, mut req_rx) = mpsc::channel(10);
    let ok_response = SendBatchResponse {
        status: send_batch_response::Status::Ok as i32,
        database_confirms_last_acked_nanos: 2,
    };

    let (ping_tx, ping_rx) = mpsc::channel(100);
    let client = create_mock_client(req_tx, vec![ok_response]).await;
    let token = generate_test_token("test-collector", &["collector"]);

    let batch_submitter = BatchSubmitter::new(client, ping_rx, "test-collector".to_string(), token);
    tokio::spawn(async move {
        let _ = batch_submitter.run().await;
    });

    let target_ip = "1.1.1.1".parse().unwrap();
    ping_tx
        .send(zzping_collector::ping_client::PingResult {
            target: target_ip,
            sent_nanos: 1,
            rtt: None,
        })
        .await
        .unwrap();
    ping_tx
        .send(zzping_collector::ping_client::PingResult {
            target: target_ip,
            sent_nanos: 2,
            rtt: None,
        })
        .await
        .unwrap();

    let received_req = tokio::time::timeout(Duration::from_secs(2), req_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received_req.records.len(), 2);
    assert_eq!(received_req.collector_believes_last_acked_nanos, 0);

    ping_tx
        .send(zzping_collector::ping_client::PingResult {
            target: target_ip,
            sent_nanos: 3,
            rtt: None,
        })
        .await
        .unwrap();

    let _ = tokio::time::timeout(Duration::from_secs(2), req_rx.recv()).await;
}

#[tokio::test]
#[timeout(2000)]
async fn test_collector_rewinds_buffer_on_desync() {
    let _ = env_logger::builder().is_test(true).try_init();

    let (req_tx, mut req_rx) = mpsc::channel(10);
    let responses = vec![
        SendBatchResponse {
            status: send_batch_response::Status::Desync as i32,
            database_confirms_last_acked_nanos: 0,
        },
        SendBatchResponse {
            status: send_batch_response::Status::Ok as i32,
            database_confirms_last_acked_nanos: 2,
        },
        SendBatchResponse {
            status: send_batch_response::Status::Ok as i32,
            database_confirms_last_acked_nanos: 3,
        },
    ];

    let (ping_tx, ping_rx) = mpsc::channel(100);
    let client = create_mock_client(req_tx, responses).await;
    let token = generate_test_token("test-collector", &["collector"]);

    let batch_submitter = BatchSubmitter::new(client, ping_rx, "test-collector".to_string(), token);
    tokio::spawn(async move {
        let _ = batch_submitter.run().await;
    });

    let target_ip = "1.1.1.1".parse().unwrap();
    ping_tx
        .send(zzping_collector::ping_client::PingResult {
            target: target_ip,
            sent_nanos: 1,
            rtt: None,
        })
        .await
        .unwrap();
    ping_tx
        .send(zzping_collector::ping_client::PingResult {
            target: target_ip,
            sent_nanos: 2,
            rtt: None,
        })
        .await
        .unwrap();

    let req1 = tokio::time::timeout(Duration::from_secs(2), req_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(req1.records.len(), 2);
    assert_eq!(req1.collector_believes_last_acked_nanos, 0);

    let req2 = tokio::time::timeout(Duration::from_secs(2), req_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(req2.records.len(), 2);
    assert_eq!(req2.collector_believes_last_acked_nanos, 0);

    ping_tx
        .send(zzping_collector::ping_client::PingResult {
            target: target_ip,
            sent_nanos: 3,
            rtt: None,
        })
        .await
        .unwrap();

    let req3 = tokio::time::timeout(Duration::from_secs(2), req_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(req3.records.len(), 1);
    assert_eq!(req3.records[0].sent_nanos, 3);
    assert_eq!(req3.collector_believes_last_acked_nanos, 2);
}
