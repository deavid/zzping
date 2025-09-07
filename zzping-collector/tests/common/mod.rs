// This file will hold test utilities shared across integration tests.

// Allow dead code in this module, as it's a library of test utilities
// and not all tests will use all functions.
#![allow(dead_code)]

use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{transport::Server, Request, Response, Status};
use zzping_proto::zzping::{
    ingestion_server::{Ingestion, IngestionServer},
    CollectorRole, HeartbeatRequest, HeartbeatResponse, QueryRequest, QueryResponse,
    SendBatchRequest, SendBatchResponse, GetRecentDataRequest, GetRecentDataResponse,
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

/// A mock implementation of the Ingestion service for testing.
#[derive(Default)]
pub struct MockIngestionService {}

#[tonic::async_trait]
impl Ingestion for MockIngestionService {
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        // Check for the auth token in the metadata for tests that need it.
        if let Some(auth_header) = request.metadata().get("authorization") {
            if auth_header != "Bearer test-token" {
                return Err(Status::unauthenticated("Missing or invalid auth token"));
            }
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

/// Helper to spawn a mock server and get its address.
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
