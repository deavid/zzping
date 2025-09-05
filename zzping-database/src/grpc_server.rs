use crate::{
    auth::{AuthToken, UserIdentity},
    config::{load_intent_config, IntentConfig},
    ingestion_item::IngestionItem,
    query::get_last_hour_records,
    scheduler::Scheduler,
};
use base64::{engine::general_purpose, Engine as _};
use dashmap::DashMap;
use log::{info, warn};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::mpsc;
use tonic::{Request, Response, Status};
use zzping_proto::zzping::{
    ingestion_server::Ingestion, send_batch_response, GetRecentDataRequest,
    GetRecentDataResponse, HeartbeatRequest, HeartbeatResponse, QueryRequest, QueryResponse,
    SendBatchRequest, SendBatchResponse,
};

#[allow(clippy::result_large_err)]
pub fn check_auth<T>(mut req: Request<T>) -> Result<Request<T>, Status> {
    let token = req
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|str| str.strip_prefix("Bearer "));

    if let Some(token) = token {
        let decoded = general_purpose::STANDARD
            .decode(token)
            .map_err(|e| Status::unauthenticated(format!("Invalid token format: {e}")))?;

        let auth_token: AuthToken = serde_json::from_slice(&decoded)
            .map_err(|e| Status::unauthenticated(format!("Invalid token JSON: {e}")))?;

        let identity = UserIdentity {
            id: auth_token.sub,
            roles: auth_token.roles.into_iter().collect::<HashSet<String>>(),
        };

        req.extensions_mut().insert(identity);

        Ok(req)
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
        let identity = request.extensions().get::<UserIdentity>().ok_or_else(|| {
            Status::internal("Missing user identity. This should have been handled by the auth interceptor.")
        })?;

        if !identity.has_role("collector") {
            return Err(Status::permission_denied("Missing 'collector' role."));
        }

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
        let identity = request.extensions().get::<UserIdentity>().ok_or_else(|| {
            Status::internal("Missing user identity. This should have been handled by the auth interceptor.")
        })?;

        if !identity.has_role("collector") {
            return Err(Status::permission_denied("Missing 'collector' role."));
        }

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
        request: Request<GetRecentDataRequest>,
    ) -> Result<Response<GetRecentDataResponse>, Status> {
        let identity = request.extensions().get::<UserIdentity>().ok_or_else(|| {
            Status::internal("Missing user identity. This should have been handled by the auth interceptor.")
        })?;

        if !identity.has_role("collector") {
            return Err(Status::permission_denied("Missing 'collector' role."));
        }

        info!("get_recent_data called by '{}', returning empty response for now.", identity.id);
        Ok(Response::new(GetRecentDataResponse { records: vec![] }))
    }

    async fn query_data(
        &self,
        request: Request<QueryRequest>,
    ) -> Result<Response<QueryResponse>, Status> {
        let identity = request.extensions().get::<UserIdentity>().ok_or_else(|| {
            Status::internal("Missing user identity. This should have been handled by the auth interceptor.")
        })?;

        if !identity.has_role("reader") {
            return Err(Status::permission_denied("Missing 'reader' role."));
        }

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
use crate::auth;
#[cfg(feature = "test-utils")]
use std::time::Duration;
#[cfg(feature = "test-utils")]
use tokio::task::JoinHandle;
#[cfg(feature = "test-utils")]
use tokio_stream::wrappers::TcpListenerStream;
#[cfg(feature = "test-utils")]
use zzping_proto::zzping::ingestion_server::IngestionServer;

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
    let server = IngestionServer::with_interceptor(service, check_auth);

    let handle = tokio::spawn(async move {
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            tonic::transport::Server::builder()
                .add_service(server)
                .serve_with_incoming(TcpListenerStream::new(listener)),
        )
        .await;
    });

    (addr, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::generate_test_token;
    use ntest::timeout;
    use std::io::Write;
    use tempfile::tempdir;
    use tonic::metadata::AsciiMetadataValue;
    use zzping_proto::zzping::{ingestion_client::IngestionClient, RawDataRecord};

    #[test]
    #[timeout(100)]
    fn test_check_auth() {
        // Good token
        let token = generate_test_token("test-user", &["reader", "collector"]);
        let mut good_req = Request::new(());
        good_req.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().unwrap(),
        );
        let result = check_auth(good_req);
        assert!(result.is_ok());
        let identity = result.unwrap().extensions().get::<UserIdentity>().unwrap().clone();
        assert_eq!(identity.id, "test-user");
        assert!(identity.has_role("reader"));
        assert!(identity.has_role("collector"));
        assert!(!identity.has_role("admin"));

        // Bad tokens
        let mut bad_req_invalid = Request::new(());
        bad_req_invalid
            .metadata_mut()
            .insert("authorization", "Bearer not-base64".parse().unwrap());
        assert!(check_auth(bad_req_invalid).is_err());

        let mut bad_req_missing = Request::new(());
        assert!(check_auth(bad_req_missing).is_err());
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_query_data_permission() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path();
        let intent_path = data_dir.join("intent.ron");
        let mut file = std::fs::File::create(intent_path).unwrap();
        write!(file, "(ping_rate_pps: 1, targets: [])").unwrap();

        let (server_addr, server_handle) =
            spawn_test_server(temp_dir.path().to_str().unwrap().to_string()).await;

        let mut client = IngestionClient::connect(format!("http://{server_addr}")).await.unwrap();

        // Test with correct role
        let token = generate_test_token("test-reader", &["reader"]);
        let mut request = Request::new(QueryRequest {});
        request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
        let response = client.query_data(request).await;
        assert!(response.is_ok());

        // Test with incorrect role
        let token = generate_test_token("test-collector", &["collector"]);
        let mut request = Request::new(QueryRequest {});
        request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
        let response = client.query_data(request).await;
        assert_eq!(response.err().unwrap().code(), tonic::Code::PermissionDenied);

        server_handle.abort();
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_heartbeat_rpc_permission() {
        let temp_dir = tempdir().unwrap();
        let intent_path = temp_dir.path().join("intent.ron");
        let mut file = std::fs::File::create(intent_path).unwrap();
        write!(file, "(ping_rate_pps: 50, targets: [])").unwrap();
        let (server_addr, server_handle) =
            spawn_test_server(temp_dir.path().to_str().unwrap().to_string()).await;

        let mut client = IngestionClient::connect(format!("http://{server_addr}")).await.unwrap();

        // Test with correct role
        let token = generate_test_token("test-collector", &["collector"]);
        let mut request = Request::new(HeartbeatRequest {
            collector_uuid: "test-collector".to_string(),
            pid: 1234,
        });
        request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
        let response = client.heartbeat(request).await;
        assert!(response.is_ok());

        // Test with incorrect role
        let token = generate_test_token("test-reader", &["reader"]);
        let mut request = Request::new(HeartbeatRequest {
            collector_uuid: "test-collector".to_string(),
            pid: 1234,
        });
        request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
        let response = client.heartbeat(request).await;
        assert_eq!(response.err().unwrap().code(), tonic::Code::PermissionDenied);

        server_handle.abort();
    }

    fn create_test_service() -> IngestionServiceImpl {
        let (tx, _) = mpsc::channel(100);
        IngestionServiceImpl {
            storage_tx: tx,
            data_dir: "".to_string(),
            intent_config: Arc::new(IntentConfig { ping_rate_pps: 0, targets: vec![] }),
            collector_states: Arc::new(DashMap::new()),
            scheduler: Arc::new(Scheduler::new()),
        }
    }

    fn create_authed_request<T>(payload: T, sub: &str, roles: &[&str]) -> Request<T> {
        let mut request = Request::new(payload);
        let identity = UserIdentity {
            id: sub.to_string(),
            roles: roles.iter().map(|s| s.to_string()).collect(),
        };
        request.extensions_mut().insert(identity);
        request
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_sendbatch_accepts_good_data() {
        let service = create_test_service();
        service.collector_states.insert("collector-1".to_string(), 100);

        let payload = SendBatchRequest {
            collector_uuid: "collector-1".to_string(),
            collector_believes_last_acked_nanos: 100,
            records: vec![
                RawDataRecord { sent_nanos: 101, rtt_nanos: 10 },
                RawDataRecord { sent_nanos: 102, rtt_nanos: 11 },
            ],
        };
        let request = create_authed_request(payload, "collector-1", &["collector"]);

        let response = service.send_batch(request).await.unwrap().into_inner();
        assert_eq!(response.status, send_batch_response::Status::Ok as i32);
        assert_eq!(response.database_confirms_last_acked_nanos, 102);
        assert_eq!(*service.collector_states.get("collector-1").unwrap(), 102);
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_sendbatch_rejects_desync_data() {
        let service = create_test_service();
        service.collector_states.insert("collector-1".to_string(), 100);

        let payload = SendBatchRequest {
            collector_uuid: "collector-1".to_string(),
            collector_believes_last_acked_nanos: 99,
            records: vec![RawDataRecord { sent_nanos: 101, rtt_nanos: 10 }],
        };
        let request = create_authed_request(payload, "collector-1", &["collector"]);

        let response = service.send_batch(request).await.unwrap().into_inner();
        assert_eq!(response.status, send_batch_response::Status::Desync as i32);
        assert_eq!(response.database_confirms_last_acked_nanos, 100);
        assert_eq!(*service.collector_states.get("collector-1").unwrap(), 100);
    }
}
