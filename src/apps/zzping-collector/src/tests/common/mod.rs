// This file will hold test utilities shared across integration tests.

// Allow dead code in this module, as it's a library of test utilities
// and not all tests will use all functions.

use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError, warn};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status, transport::Server};
use zzping_proto::zzping::{
    AnnouncePingsRequest, AnnouncePingsResponse, CollectorRole, GetRecentDataRequest,
    GetRecentDataResponse, HeartbeatRequest, HeartbeatResponse, QueryRequest, QueryResponse,
    SendBatchRequest, SendBatchResponse,
    ingestion_server::{Ingestion, IngestionServer},
    send_batch_response,
};

/// A simple logger that captures log messages into a shared vector.
pub struct VectorLogger {}

// Global storage for the most-recent test's log_messages buffer. Tests call
// `VectorLogger::init` with their own Arc<Mutex<Vec>>; we store it here so the
// single global logger instance can push into the most recent buffer. Use a
// Mutex to allow replacing the buffer between tests.
static GLOBAL_LOG_MESSAGES: Mutex<Option<Arc<Mutex<Vec<String>>>>> = Mutex::new(None);

impl Log for VectorLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let msg = format!("{}", record.args());
            // Capture relevant messages for tests
            if msg.starts_with("Attempting to connect")
                || msg.starts_with("Session ended")
                || msg.contains("Deferring worker creation")
                || msg.contains("TaskSupervisor: Adding worker")
            {
                // Also print to stderr for test debugging visibility (keeps stdout clean)
                eprintln!("[zzping-test] {msg}");
                let guard = GLOBAL_LOG_MESSAGES
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if let Some(ref arc) = *guard {
                    arc.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
                }
            }
        }
    }

    fn flush(&self) {}
}

impl VectorLogger {
    /// Initializes or replaces the global test log buffer. Multiple tests may
    /// call this; the global logger instance is set once and future calls will
    /// replace the buffer that log messages are pushed into.
    pub fn init(log_messages: Arc<Mutex<Vec<String>>>) -> Result<(), SetLoggerError> {
        // Replace the global buffer.
        let mut guard = GLOBAL_LOG_MESSAGES
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = Some(log_messages);

        // Try to set the global logger. If it fails because a logger was
        // already installed, ignore the error — we replaced the buffer above
        // so the existing logger will now write into the new buffer.
        let _ = log::set_boxed_logger(Box::new(VectorLogger {}));
        log::set_max_level(LevelFilter::Info);
        Ok(())
    }
}

// --- Mock gRPC Server ---

use zzping_proto::zzping::Command;

/// A mock implementation of the Ingestion service for testing.
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct MockIngestionService {
    /// Received SendBatch RPCs captured for inspection by tests.
    pub received_batches: Arc<Mutex<Vec<SendBatchRequest>>>,
    /// Received Heartbeat RPCs captured for inspection by tests.
    pub received_heartbeats: Arc<Mutex<Vec<HeartbeatRequest>>>,
    /// Configurable SendBatchResponse returned by the mock.
    pub send_batch_response: Arc<Mutex<SendBatchResponse>>,
    /// Configurable HeartbeatResponse returned by the mock.
    pub heartbeat_response: Arc<Mutex<HeartbeatResponse>>,
    /// Test-only hook: when present, the heartbeat RPC will await a
    /// HeartbeatResponse sent on this receiver. This lets tests deterministically
    /// trigger heartbeat responses containing e.g. `last_fsynced_received_nanos`.
    /// Optional override channel for supplying HeartbeatResponse values.
    pub heartbeat_override_rx:
        Arc<AsyncMutex<Option<tokio::sync::mpsc::Receiver<HeartbeatResponse>>>>,
    /// Response to return for get_recent_data RPCs.
    pub get_recent_data_response: Arc<Mutex<GetRecentDataResponse>>,
    /// Sender used to push command stream items to subscribers.
    pub command_stream_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<Result<Command, Status>>>>>,
    /// When true, send_batch RPCs return an error to exercise error paths.
    pub send_batch_should_fail: Arc<Mutex<bool>>,
}

impl Default for MockIngestionService {
    fn default() -> Self {
        Self::new()
    }
}

impl MockIngestionService {
    /// Create a new mock ingestion service with the default heartbeat
    /// ping_rate_pps of 0 (prevents real pings in tests). Use
    /// `with_ping_rate` if tests need a different default.
    pub fn new() -> Self {
        Self::with_ping_rate(0)
    }

    /// Create a new mock ingestion service with a configurable
    /// `ping_rate_pps` in the default heartbeat response.
    pub fn with_ping_rate(ping_rate_pps: u64) -> Self {
        Self {
            received_batches: Arc::new(Mutex::new(Vec::new())),
            received_heartbeats: Arc::new(Mutex::new(Vec::new())),
            send_batch_response: Arc::new(Mutex::new(SendBatchResponse {
                status: send_batch_response::Status::Ok as i32,
                database_confirms_last_acked_received_nanos: 0,
            })),
            heartbeat_response: Arc::new(Mutex::new(HeartbeatResponse {
                targets: vec!["127.0.0.1".to_string()],
                ping_rate_pps,
                role: CollectorRole::Primary as i32,
                swap_at_nanos: 0,
                last_fsynced_received_nanos: 0,
            })),
            heartbeat_override_rx: Arc::new(AsyncMutex::new(None)),
            get_recent_data_response: Arc::new(Mutex::new(GetRecentDataResponse {
                records: vec![],
                database_confirms_last_acked_received_nanos: 0,
            })),
            command_stream_tx: Arc::new(Mutex::new(None)),
            send_batch_should_fail: Arc::new(Mutex::new(false)),
        }
    }

    /// Install a test-only override channel. Returns the `Sender` side.
    /// Send a `HeartbeatResponse` on the returned sender when you want the
    /// next `heartbeat` call to return that response. The channel is kept
    /// installed until the sender or receiver is dropped.
    pub async fn install_heartbeat_override_channel(
        &self,
    ) -> tokio::sync::mpsc::Sender<HeartbeatResponse> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let mut guard = self.heartbeat_override_rx.lock().await;
        *guard = Some(rx);
        tx
    }
}

#[tonic::async_trait]
impl Ingestion for MockIngestionService {
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        // Check authentication
        if let Some(auth_header) = request.metadata().get("authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                if !auth_str.starts_with("Bearer ") || &auth_str[7..] != "test-token" {
                    return Err(Status::unauthenticated("Invalid token"));
                }
            } else {
                return Err(Status::unauthenticated("Invalid token format"));
            }
        } else {
            return Err(Status::unauthenticated("Missing authorization header"));
        }

        self.received_heartbeats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(request.into_inner());

        // If a test has installed an override channel, await a supplied
        // HeartbeatResponse from the test. Use the async mutex so we can
        // await without blocking the runtime.
        if let Some(rx) = self.heartbeat_override_rx.lock().await.as_mut()
            && let Some(resp) = rx.recv().await
        {
            return Ok(Response::new(resp));
        }

        let response = self
            .heartbeat_response
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        Ok(Response::new(response))
    }

    async fn send_batch(
        &self,
        request: Request<SendBatchRequest>,
    ) -> Result<Response<SendBatchResponse>, Status> {
        if *self
            .send_batch_should_fail
            .lock()
            .unwrap_or_else(|e| e.into_inner())
        {
            return Err(Status::unavailable("Mock service is configured to fail"));
        }

        // Check authentication
        if let Some(auth_header) = request.metadata().get("authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                if !auth_str.starts_with("Bearer ") || &auth_str[7..] != "test-token" {
                    return Err(Status::unauthenticated("Invalid token"));
                }
            } else {
                return Err(Status::unauthenticated("Invalid token format"));
            }
        } else {
            return Err(Status::unauthenticated("Missing authorization header"));
        }

        self.received_batches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(request.into_inner());
        let response = *self
            .send_batch_response
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Ok(Response::new(response))
    }

    async fn get_recent_data(
        &self,
        _request: Request<GetRecentDataRequest>,
    ) -> Result<Response<GetRecentDataResponse>, Status> {
        let response = self
            .get_recent_data_response
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        Ok(Response::new(response))
    }

    async fn query_data(
        &self,
        _request: Request<QueryRequest>,
    ) -> Result<Response<QueryResponse>, Status> {
        // For tests, return an empty QueryResponse. This keeps behavior simple
        // and avoids panics from unimplemented RPCs when test suites exercise
        // query-related paths.
        Ok(Response::new(QueryResponse { records: vec![] }))
    }

    async fn announce_pings(
        &self,
        request: Request<AnnouncePingsRequest>,
    ) -> Result<Response<AnnouncePingsResponse>, Status> {
        // Check authentication
        if let Some(auth_header) = request.metadata().get("authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                if !auth_str.starts_with("Bearer ") || &auth_str[7..] != "test-token" {
                    return Err(Status::unauthenticated("Invalid token"));
                }
            } else {
                return Err(Status::unauthenticated("Invalid token format"));
            }
        } else {
            return Err(Status::unauthenticated("Missing authorization header"));
        }

        // This is a fire-and-forget RPC, so we just return OK.
        Ok(Response::new(AnnouncePingsResponse {}))
    }

    type SubscribeToCommandsStream =
        std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<Command, Status>> + Send>>;

    async fn subscribe_to_commands(
        &self,
        request: Request<zzping_proto::zzping::CommandRequest>,
    ) -> Result<Response<Self::SubscribeToCommandsStream>, Status> {
        // Check authentication
        if let Some(auth_header) = request.metadata().get("authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                if !auth_str.starts_with("Bearer ") || &auth_str[7..] != "test-token" {
                    return Err(Status::unauthenticated("Invalid token"));
                }
            } else {
                return Err(Status::unauthenticated("Invalid token format"));
            }
        } else {
            return Err(Status::unauthenticated("Missing authorization header"));
        }

        let (tx, rx) = tokio::sync::mpsc::channel(10);
        *self
            .command_stream_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(tx);
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream)))
    }
}

use crate::target_worker::WorkerCommand;
use tokio::sync::mpsc;

// --- Mock TargetWorker ---

/// A mock handle for a TargetWorker, used to receive commands in tests.
pub struct MockTargetWorkerHandle {
    /// Receiver for worker commands produced during tests.
    pub command_rx: mpsc::Receiver<WorkerCommand>,
}

/// A factory function to create a real `TargetWorkerHandle` for the supervisor
/// to use, and a `MockTargetWorkerHandle` for the test to inspect.
pub fn mock_worker_factory() -> (
    crate::target_worker::TargetWorkerHandle,
    MockTargetWorkerHandle,
) {
    let (command_tx, command_rx) = mpsc::channel(10);
    let mock_handle = MockTargetWorkerHandle { command_rx };

    // The real handle that the supervisor will interact with.
    let real_handle = crate::target_worker::TargetWorkerHandle {
        command_tx,
        // The task handle is not used by the supervisor test, so we can use a dummy one.
        task_handle: tokio::spawn(async {}),
    };
    (real_handle, mock_handle)
}

/// Helper to spawn a mock server and get its address.
pub async fn spawn_mock_server(service: MockIngestionService) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = IngestionServer::new(service);
    warn!("Spawning mock server with address: {addr}");
    tokio::spawn(async move {
        Server::builder()
            .add_service(server)
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    addr
}

pub(crate) fn setup_logger() {
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();
}
