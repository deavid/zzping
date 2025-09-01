use crate::ingestion_item::IngestionItem;
use crate::query::get_last_hour_records;
use dashmap::DashMap;
use futures_core::Stream;
use log::{error, info, warn};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use zzping_proto::zzping::{
    AckResponse, IngestRequest, IngestResponse, QueryRequest, QueryResponse, WelcomeResponse,
    ingest_request::Payload as IngestRequestPayload,
    ingest_response::Payload as IngestResponsePayload, ingestion_server::Ingestion,
};

#[derive(Debug)]
struct StreamState {
    last_sent_nanos: u64,
}

#[derive(Debug)]
pub struct IngestionServiceImpl {
    streams: Arc<DashMap<String, StreamState>>,
    storage_tx: mpsc::Sender<IngestionItem>,
}

impl IngestionServiceImpl {
    pub fn new(storage_tx: mpsc::Sender<IngestionItem>) -> Self {
        Self {
            streams: Arc::new(DashMap::new()),
            storage_tx,
        }
    }
}

type IngestStreamT = Pin<Box<dyn Stream<Item = Result<IngestResponse, Status>> + Send>>;

#[tonic::async_trait]
impl Ingestion for IngestionServiceImpl {
    type IngestStreamStream = IngestStreamT;

    async fn ingest_stream(
        &self,
        request: Request<Streaming<IngestRequest>>,
    ) -> Result<Response<Self::IngestStreamStream>, Status> {
        let mut stream = request.into_inner();
        let streams = self.streams.clone();
        let storage_tx = self.storage_tx.clone();

        let handshake = match stream.message().await? {
            Some(IngestRequest {
                payload: Some(IngestRequestPayload::Handshake(h)),
            }) => h,
            _ => {
                return Err(Status::invalid_argument(
                    "First message must be a handshake",
                ));
            }
        };

        let stream_key = format!("{}-{}", handshake.source_hostname, handshake.target_ip);
        info!("New stream from {stream_key}");

        streams.insert(stream_key.clone(), StreamState { last_sent_nanos: 0 });

        let (tx, rx) = mpsc::channel(100);

        let welcome = WelcomeResponse {
            last_known_sent_nanos: 0,
        };
        if tx
            .send(Ok(IngestResponse {
                payload: Some(IngestResponsePayload::Welcome(welcome)),
            }))
            .await
            .is_err()
        {
            warn!("Client disconnected before welcome response could be sent");
            streams.remove(&stream_key);
            let stream = ReceiverStream::new(rx);
            return Ok(Response::new(Box::pin(stream) as Self::IngestStreamStream));
        }

        tokio::spawn(async move {
            info!("Spawned task for stream {stream_key}");
            let mut record_count = 0;
            loop {
                info!("Looping in spawned task for stream {stream_key}");
                match stream.message().await {
                    Ok(Some(ingest_request)) => {
                        info!("Received message from stream {stream_key}");
                        match ingest_request.payload {
                            Some(IngestRequestPayload::Record(record)) => {
                                record_count += 1;
                                info!("Received record: {record:?}");

                                if let Some(mut state) = streams.get_mut(&stream_key) {
                                    state.last_sent_nanos = record.sent_nanos;
                                }

                                let item = IngestionItem {
                                    source_hostname: handshake.source_hostname.clone(),
                                    target: handshake.target_ip.parse().unwrap(),
                                    record: zzping_lib::protocol::RawDataRecord {
                                        sent_nanos: record.sent_nanos,
                                        rtt_nanos: record.rtt_nanos,
                                    },
                                };
                                if storage_tx.send(item).await.is_err() {
                                    error!("Storage channel closed");
                                    break;
                                }

                                if record_count % 100 == 0 {
                                    let ack = AckResponse {
                                        last_acked_sent_nanos: record.sent_nanos,
                                    };
                                    if tx
                                        .send(Ok(IngestResponse {
                                            payload: Some(IngestResponsePayload::Ack(ack)),
                                        }))
                                        .await
                                        .is_err()
                                    {
                                        warn!("Client {stream_key} disconnected, cannot send ack");
                                        break;
                                    }
                                }
                            }
                            Some(IngestRequestPayload::Handshake(_)) => {
                                warn!(
                                    "Client sent a handshake message mid-stream. This is not allowed."
                                );
                                let _ = tx
                                    .send(Err(Status::invalid_argument(
                                        "Handshake message not allowed mid-stream",
                                    )))
                                    .await;
                                break;
                            }
                            None => {
                                warn!("Client sent an empty IngestRequest payload.");
                            }
                        }
                    }
                    Ok(None) => {
                        info!("Client stream {stream_key} closed");
                        break;
                    }
                    Err(e) => {
                        warn!("Error reading from stream from {stream_key}: {e}");
                        break;
                    }
                }
            }

            info!("Stream from {stream_key} disconnected, cleaning up");
            streams.remove(&stream_key);
            info!("Finished spawned task for stream {stream_key}");
        });

        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::IngestStreamStream))
    }

    async fn query_data(
        &self,
        _request: Request<QueryRequest>,
    ) -> Result<Response<QueryResponse>, Status> {
        let records = get_last_hour_records(crate::DATA_DIR)
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
use std::future::IntoFuture;
#[cfg(feature = "test-utils")]
use std::time::Duration;
#[cfg(feature = "test-utils")]
use tokio::task::JoinHandle;

#[cfg(feature = "test-utils")]
pub async fn timeout<F>(future: F) -> <F as IntoFuture>::Output
where
    F: IntoFuture,
{
    tokio::time::timeout(Duration::from_millis(100), future)
        .await
        .expect("Timeout happened!")
}

#[cfg(feature = "test-utils")]
pub async fn spawn_test_server() -> (std::net::SocketAddr, JoinHandle<()>) {
    let listener = timeout(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, mut rx) = mpsc::channel(100); // Dummy channel for storage
    let service = IngestionServiceImpl::new(tx);

    tokio::spawn(async move {
        while (rx.recv().await).is_some() {}
    });

    let handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(zzping_proto::zzping::ingestion_server::IngestionServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    (addr, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;
    use zzping_proto::zzping::{
        HandshakeRequest, RawDataRecord, ingestion_client::IngestionClient,
    };

    #[tokio::test]
    async fn test_query_data_returns_empty_response() {
        std::fs::create_dir_all(crate::DATA_DIR).unwrap();
        let (tx, _) = mpsc::channel(1);
        let service = IngestionServiceImpl::new(tx);
        let request = Request::new(QueryRequest {});
        let response = timeout(service.query_data(request)).await.unwrap();
        assert!(response.into_inner().records.is_empty());
        std::fs::remove_dir_all(crate::DATA_DIR).unwrap();
    }

    #[tokio::test]
    async fn test_ingest_stream_flow() {
        let _ = env_logger::builder().is_test(true).try_init();
        println!("Starting test_ingest_stream_flow");
        let (server_addr, _server_handle) = timeout(spawn_test_server()).await;
        println!("Server spawned at {server_addr}");
        let mut client = timeout(IngestionClient::connect(format!("http://{server_addr}")))
            .await
            .unwrap();
        println!("Client connected");

        let (tx, rx) = mpsc::channel(10);

        let handshake = HandshakeRequest {
            source_hostname: "test-host".to_string(),
            target_ip: "1.2.3.4".to_string(),
        };
        timeout(tx.send(IngestRequest {
            payload: Some(IngestRequestPayload::Handshake(handshake)),
        }))
        .await
        .unwrap();

        let response_stream = timeout(client.ingest_stream(ReceiverStream::new(rx)))
            .await
            .unwrap();
        let mut response_stream = response_stream.into_inner();
        println!("Got response stream");

        println!("Awaiting welcome response");
        let welcome_response = timeout(response_stream.next()).await.unwrap().unwrap();
        info!("Welcome response received");

        match welcome_response.payload {
            Some(IngestResponsePayload::Welcome(welcome)) => {
                assert_eq!(welcome.last_known_sent_nanos, 0);
            }
            _ => panic!("Expected WelcomeResponse"),
        }

        let records_to_send = 250;
        println!("Spawning ack handle");
        let ack_handle = tokio::spawn(async move {
            let mut acks_received = 0;
            loop {
                match timeout(response_stream.next()).await {
                    Some(Ok(response)) => {
                        if let Some(IngestResponsePayload::Ack(_)) = response.payload {
                            println!("Ack received");
                            acks_received += 1;
                        }
                    }
                    Some(Err(e)) => {
                        println!("Error in response stream: {e:?}");
                        break;
                    }
                    None => {
                        println!("Response stream closed");
                        break;
                    }
                }
            }
            println!("Ack handle finished");
            acks_received
        });

        println!("Sending {records_to_send} records");
        for i in 0..records_to_send {
            let record = RawDataRecord {
                sent_nanos: i + 1,
                rtt_nanos: 100,
            };
            timeout(tx.send(IngestRequest {
                payload: Some(IngestRequestPayload::Record(record)),
            }))
            .await
            .unwrap();
        }

        println!("Dropping client tx");
        drop(tx);

        println!("Awaiting ack handle");
        let acks_received = timeout(ack_handle).await.unwrap();
        assert_eq!(acks_received, 2);
        println!("Test finished");
    }
}
