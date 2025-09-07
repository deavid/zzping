// This file will hold test utilities shared across integration tests.

// Allow dead code in this module, as it's a library of test utilities
// and not all tests will use all functions.
#![allow(dead_code)]

use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
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
pub struct VectorLogger {
    log_messages: Arc<Mutex<Vec<String>>>,
}

impl Log for VectorLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let msg = format!("{}", record.args());
            // We only care about connection messages for our test.
            if msg.starts_with("Attempting to connect") || msg.starts_with("Session ended") {
                self.log_messages.lock().unwrap().push(msg);
            }
        }
    }

    fn flush(&self) {}
}

impl VectorLogger {
    /// Initializes the global logger with a `VectorLogger` instance.
    pub fn init(log_messages: Arc<Mutex<Vec<String>>>) -> Result<(), SetLoggerError> {
        let logger = Box::new(VectorLogger { log_messages });
        log::set_boxed_logger(logger)?;
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
    pub received_batches: Arc<Mutex<Vec<SendBatchRequest>>>,
    pub received_heartbeats: Arc<Mutex<Vec<HeartbeatRequest>>>,
    pub send_batch_response: Arc<Mutex<SendBatchResponse>>,
    pub heartbeat_response: Arc<Mutex<HeartbeatResponse>>,
    pub command_stream_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<Result<Command, Status>>>>>,
}

impl Default for MockIngestionService {
    fn default() -> Self {
        Self::new()
    }
}

impl MockIngestionService {
    pub fn new() -> Self {
        Self {
            received_batches: Arc::new(Mutex::new(Vec::new())),
            received_heartbeats: Arc::new(Mutex::new(Vec::new())),
            send_batch_response: Arc::new(Mutex::new(SendBatchResponse {
                status: send_batch_response::Status::Ok as i32,
                database_confirms_last_acked_received_nanos: 0,
            })),
            heartbeat_response: Arc::new(Mutex::new(HeartbeatResponse {
                targets: vec!["127.0.0.1".to_string()],
                ping_rate_pps: 100,
                role: CollectorRole::Primary as i32,
                swap_at_nanos: 0,
            })),
            command_stream_tx: Arc::new(Mutex::new(None)),
        }
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
            .unwrap()
            .push(request.into_inner());

        let response = self.heartbeat_response.lock().unwrap().clone();
        Ok(Response::new(response))
    }

    async fn send_batch(
        &self,
        request: Request<SendBatchRequest>,
    ) -> Result<Response<SendBatchResponse>, Status> {
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
            .unwrap()
            .push(request.into_inner());
        let response = self.send_batch_response.lock().unwrap().clone();
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
        *self.command_stream_tx.lock().unwrap() = Some(tx);
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream)))
    }
}

use tokio::sync::mpsc;
use zzping_collector::target_worker::WorkerCommand;

// --- Mock TargetWorker ---

/// A mock handle for a TargetWorker, used to receive commands in tests.
pub struct MockTargetWorkerHandle {
    pub command_rx: mpsc::Receiver<WorkerCommand>,
}

/// A factory function to create a real `TargetWorkerHandle` for the supervisor
/// to use, and a `MockTargetWorkerHandle` for the test to inspect.
pub fn mock_worker_factory() -> (
    zzping_collector::target_worker::TargetWorkerHandle,
    MockTargetWorkerHandle,
) {
    let (command_tx, command_rx) = mpsc::channel(10);
    let mock_handle = MockTargetWorkerHandle { command_rx };

    // The real handle that the supervisor will interact with.
    let real_handle = zzping_collector::target_worker::TargetWorkerHandle {
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

    tokio::spawn(async move {
        Server::builder()
            .add_service(server)
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    addr
}
