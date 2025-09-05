use crate::config::{load_intent_config, IntentConfig};
use crate::ingestion_item::IngestionItem;
use crate::query::get_last_hour_records;
use crate::scheduler::Scheduler;
use dashmap::DashMap;
use log::{info, warn};
use std::sync::Arc;
use tokio::sync::mpsc;
use tonic::{Request, Response, Status};
use zzping_proto::zzping::{
    ingestion_server::Ingestion, send_batch_response, GetRecentDataRequest,
    GetRecentDataResponse, HeartbeatRequest, HeartbeatResponse, QueryRequest, QueryResponse,
    SendBatchRequest, SendBatchResponse,
};

#[allow(clippy::result_large_err)]
pub fn check_auth(req: Request<()>) -> Result<Request<()>, Status> {
    let token = req
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|str| str.strip_prefix("Bearer "));

    if let Some(token) = token {
        if token == "my-secret-token" {
            Ok(req)
        } else {
            Err(Status::unauthenticated("Invalid token"))
        }
    } else {
        Err(Status::unauthenticated("Missing auth token"))
    }
}

#[derive(Debug)]
pub struct IngestionServiceImpl {
    storage_tx: mpsc::Sender<IngestionItem>,
    data_dir: String,
    intent_config: Arc<IntentConfig>,
    collector_states: Arc<DashMap<String, u64>>,
    scheduler: Arc<Scheduler>,
}

impl IngestionServiceImpl {
    pub fn new(storage_tx: mpsc::Sender<IngestionItem>, data_dir: String) -> Self {
        let intent_path = format!("{data_dir}/intent.ron");
        let intent_config = load_intent_config(&intent_path)
            .unwrap_or_else(|e| panic!("Failed to load intent config from {intent_path}: {e}"));
        info!("Loaded intent config: {intent_config:?}");

        Self {
            storage_tx,
            data_dir,
            intent_config: Arc::new(intent_config),
            collector_states: Arc::new(DashMap::new()),
            scheduler: Arc::new(Scheduler::new()),
        }
    }
}

#[tonic::async_trait]
impl Ingestion for IngestionServiceImpl {
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let request = request.into_inner();
        let (role, swap_at_nanos) = self.scheduler.process_heartbeat(
            &request.collector_uuid,
            request.pid,
            std::time::Instant::now(),
        );

        Ok(Response::new(HeartbeatResponse {
            targets: self.intent_config.targets.clone(),
            ping_rate_pps: self.intent_config.ping_rate_pps,
            role: role as i32,
            swap_at_nanos,
        }))
    }

    async fn send_batch(
        &self,
        request: Request<SendBatchRequest>,
    ) -> Result<Response<SendBatchResponse>, Status> {
        let request = request.into_inner();
        let collector_id = request.collector_uuid;
        let collector_believes_last_acked = request.collector_believes_last_acked_nanos;

        let mut collector_state = self
            .collector_states
            .entry(collector_id.clone())
            .or_insert(0);

        let db_last_acked = *collector_state;

        if db_last_acked != collector_believes_last_acked {
            warn!(
                "Collector {collector_id} is out of sync. DB acked: {db_last_acked}, collector believed: {collector_believes_last_acked}"
            );
            return Ok(Response::new(SendBatchResponse {
                status: send_batch_response::Status::Desync as i32,
                database_confirms_last_acked_nanos: db_last_acked,
            }));
        }

        let mut last_sent_nanos = db_last_acked;
        for record in request.records {
            if record.sent_nanos <= db_last_acked {
                warn!("Collector {collector_id} sent record with old timestamp, ignoring.");
                continue;
            }
            info!("Record from {collector_id}: {record:?}");
            last_sent_nanos = record.sent_nanos;
        }

        *collector_state = last_sent_nanos;

        Ok(Response::new(SendBatchResponse {
            status: send_batch_response::Status::Ok as i32,
            database_confirms_last_acked_nanos: last_sent_nanos,
        }))
    }

    async fn get_recent_data(
        &self,
        _request: Request<GetRecentDataRequest>,
    ) -> Result<Response<GetRecentDataResponse>, Status> {
        info!("get_recent_data called, returning empty response for now.");
        Ok(Response::new(GetRecentDataResponse { records: vec![] }))
    }

    async fn query_data(
        &self,
        _request: Request<QueryRequest>,
    ) -> Result<Response<QueryResponse>, Status> {
        let records = get_last_hour_records(&self.data_dir)
            .map_err(|e| Status::internal(e.to_string()))?
            .into_iter()
            .map(|r| zzping_proto::zzping::RawDataRecord {
                sent_nanos: r.sent_nanos,
                rtt_nanos: r.rtt_nanos,
            })
            .collect();

        Ok(Response::new(QueryResponse { records }))
    }
}

#[cfg(feature = "test-utils")]
use std::time::Duration;
#[cfg(feature = "test-utils")]
use tokio::task::JoinHandle;
#[cfg(feature = "test-utils")]
use tokio_stream::wrappers::TcpListenerStream;

#[cfg(feature = "test-utils")]
pub async fn spawn_test_server(data_dir: String) -> (std::net::SocketAddr, JoinHandle<()>) {
    let listener = tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::TcpListener::bind("127.0.0.1:0"),
    )
    .await
    .expect("Listener bind timed out")
    .unwrap();

    let addr = listener.local_addr().unwrap();
    let (tx, _) = mpsc::channel(100);
    let service = IngestionServiceImpl::new(tx, data_dir);

    let handle = tokio::spawn(async move {
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            tonic::transport::Server::builder()
                .add_service(zzping_proto::zzping::ingestion_server::IngestionServer::new(
                    service,
                ))
                .serve_with_incoming(TcpListenerStream::new(listener)),
        )
        .await;
    });

    (addr, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;
    use std::io::Write;

    use tempfile::tempdir;
    use zzping_proto::zzping::{ingestion_client::IngestionClient, RawDataRecord};

    #[test]
    #[timeout(100)]
    fn test_check_auth() {
        let mut good_req = Request::new(());
        good_req
            .metadata_mut()
            .insert("authorization", "Bearer my-secret-token".parse().unwrap());
        let mut bad_req_invalid = Request::new(());
        bad_req_invalid
            .metadata_mut()
            .insert("authorization", "Bearer invalid-token".parse().unwrap());
        let bad_req_missing = Request::new(());
        assert!(check_auth(good_req).is_ok());
        assert!(check_auth(bad_req_invalid).is_err());
        assert!(check_auth(bad_req_missing).is_err());
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_query_data_returns_empty_response() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let intent_path = data_dir.join("intent.ron");
        let mut file = std::fs::File::create(intent_path).unwrap();
        write!(file, "(ping_rate_pps: 1, targets: [])").unwrap();
        let (tx, _) = mpsc::channel(1);
        let service = IngestionServiceImpl::new(tx, data_dir.to_str().unwrap().to_string());
        let request = Request::new(QueryRequest {});
        let response = service.query_data(request).await.unwrap();
        assert!(response.into_inner().records.is_empty());
    }

    #[tokio::test]
    #[timeout(1000)] // This test spawns a server, so it needs a longer timeout.
    async fn test_heartbeat_rpc() {
        let temp_dir = tempdir().unwrap();
        let intent_path = temp_dir.path().join("intent.ron");
        let mut file = std::fs::File::create(intent_path).unwrap();
        let expected_config = r#"(ping_rate_pps: 50, targets: ["1.2.3.4", "5.6.7.8"])"#;
        write!(file, "{expected_config}").unwrap();
        let (server_addr, server_handle) =
            spawn_test_server(temp_dir.path().to_str().unwrap().to_string()).await;

        let mut client = IngestionClient::connect(format!("http://{server_addr}")).await.unwrap();
        let request = Request::new(HeartbeatRequest {
            collector_uuid: "test-collector".to_string(),
            pid: 1234,
        });
        let response = client.heartbeat(request).await.unwrap();

        server_handle.abort();

        let response = response.into_inner();
        assert_eq!(response.ping_rate_pps, 50);
        assert_eq!(response.targets, vec!["1.2.3.4", "5.6.7.8"]);
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_sendbatch_accepts_good_data() {
        let (tx, _) = mpsc::channel(100);
        let service = IngestionServiceImpl {
            storage_tx: tx,
            data_dir: "".to_string(),
            intent_config: Arc::new(IntentConfig { ping_rate_pps: 0, targets: vec![] }),
            collector_states: Arc::new(DashMap::new()),
            scheduler: Arc::new(Scheduler::new()),
        };
        service.collector_states.insert("collector-1".to_string(), 100);

        let request = Request::new(SendBatchRequest {
            collector_uuid: "collector-1".to_string(),
            collector_believes_last_acked_nanos: 100,
            records: vec![
                RawDataRecord { sent_nanos: 101, rtt_nanos: 10 },
                RawDataRecord { sent_nanos: 102, rtt_nanos: 11 },
            ],
        });

        let response = service.send_batch(request).await.unwrap().into_inner();
        assert_eq!(response.status, send_batch_response::Status::Ok as i32);
        assert_eq!(response.database_confirms_last_acked_nanos, 102);
        assert_eq!(*service.collector_states.get("collector-1").unwrap(), 102);
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_sendbatch_rejects_desync_data() {
        let (tx, _) = mpsc::channel(100);
        let service = IngestionServiceImpl {
            storage_tx: tx,
            data_dir: "".to_string(),
            intent_config: Arc::new(IntentConfig { ping_rate_pps: 0, targets: vec![] }),
            collector_states: Arc::new(DashMap::new()),
            scheduler: Arc::new(Scheduler::new()),
        };
        service.collector_states.insert("collector-1".to_string(), 100);

        let request = Request::new(SendBatchRequest {
            collector_uuid: "collector-1".to_string(),
            collector_believes_last_acked_nanos: 99,
            records: vec![RawDataRecord { sent_nanos: 101, rtt_nanos: 10 }],
        });

        let response = service.send_batch(request).await.unwrap().into_inner();
        assert_eq!(response.status, send_batch_response::Status::Desync as i32);
        assert_eq!(response.database_confirms_last_acked_nanos, 100);
        assert_eq!(*service.collector_states.get("collector-1").unwrap(), 100);
    }
}
