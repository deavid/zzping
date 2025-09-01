use dashmap::DashMap;
use futures_core::Stream;
use log::{info, warn};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use zzping_proto::zzping::{
    ingestion_server::Ingestion,
    ingest_request::Payload as IngestRequestPayload,
    ingest_response::Payload as IngestResponsePayload,
    AckResponse, IngestRequest, IngestResponse, QueryRequest, QueryResponse, WelcomeResponse,
};

// This will hold the state for each active ingestion stream.
// The key could be a unique identifier for the stream (e.g., "hostname-target_ip").
#[derive(Debug)]
struct StreamState {
    last_sent_nanos: u64,
}

#[derive(Debug, Default)]
pub struct IngestionServiceImpl {
    // Use Arc for shared ownership across tonic's worker threads.
    streams: Arc<DashMap<String, StreamState>>,
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

        // The first message MUST be a handshake.
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

        let stream_key = format!(
            "{}-{}",
            handshake.source_hostname, handshake.target_ip
        );
        info!("New stream from {}", stream_key);

        streams.insert(
            stream_key.clone(),
            StreamState {
                last_sent_nanos: 0,
            },
        );

        let (tx, rx) = mpsc::channel(100);

        // Send WelcomeResponse
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
            // The stream is already closed, so we don't need to do anything else.
            // Just return an empty stream.
            let stream = ReceiverStream::new(rx);
            return Ok(Response::new(Box::pin(stream) as Self::IngestStreamStream));
        }

        tokio::spawn(async move {
            info!("Spawned task for stream {}", stream_key);
            let mut record_count = 0;
            loop {
                info!("Looping in spawned task for stream {}", stream_key);
                match stream.message().await {
                    Ok(Some(ingest_request)) => {
                        info!("Received message from stream {}", stream_key);
                        match ingest_request.payload {
                            Some(IngestRequestPayload::Record(record)) => {
                                record_count += 1;
                                info!("Received record: {:?}", record);

                                if let Some(mut state) = streams.get_mut(&stream_key) {
                                    state.last_sent_nanos = record.sent_nanos;
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
                                        warn!(
                                            "Client {} disconnected, cannot send ack",
                                            stream_key
                                        );
                                        break;
                                    }
                                }
                            }
                            Some(IngestRequestPayload::Handshake(_)) => {
                                warn!("Client sent a handshake message mid-stream. This is not allowed.");
                                let _ = tx.send(Err(Status::invalid_argument(
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
                        info!("Client stream {} closed", stream_key);
                        // Stream closed by client
                        break;
                    }
                    Err(e) => {
                        warn!("Error reading from stream from {}: {}", stream_key, e);
                        break;
                    }
                }
            }

            info!("Stream from {} disconnected, cleaning up", stream_key);
            streams.remove(&stream_key);
            info!("Finished spawned task for stream {}", stream_key);
        });

        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::IngestStreamStream))
    }

    async fn query_data(
        &self,
        _request: Request<QueryRequest>,
    ) -> Result<Response<QueryResponse>, Status> {
        Ok(Response::new(QueryResponse { records: vec![] }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;
    use tokio_stream::StreamExt;
    use zzping_proto::zzping::{ingestion_client::IngestionClient, HandshakeRequest, RawDataRecord};

    #[tokio::test]
    async fn test_query_data_returns_empty_response() {
        let service = IngestionServiceImpl::default();
        let request = Request::new(QueryRequest {});
        let response = service.query_data(request).await.unwrap();
        assert!(response.into_inner().records.is_empty());
    }

    async fn spawn_test_server() -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service = IngestionServiceImpl::default();

        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(zzping_proto::zzping::ingestion_server::IngestionServer::new(
                    service,
                ))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        addr
    }

    #[tokio::test]
    async fn test_ingest_stream_flow() {
        let _ = env_logger::builder().is_test(true).try_init();
        info!("Starting test_ingest_stream_flow");
        let server_addr = spawn_test_server().await;
        info!("Server spawned at {}", server_addr);
        let mut client = IngestionClient::connect(format!("http://{}", server_addr))
            .await
            .unwrap();
        info!("Client connected");

        let (tx, rx) = mpsc::channel(10);
        let response_stream = client.ingest_stream(ReceiverStream::new(rx)).await.unwrap();
        let mut response_stream = response_stream.into_inner();
        info!("Got response stream");

        // 1. Send Handshake
        info!("Sending handshake");
        let handshake = HandshakeRequest {
            source_hostname: "test-host".to_string(),
            target_ip: "1.2.3.4".to_string(),
        };
        tx.send(IngestRequest {
            payload: Some(IngestRequestPayload::Handshake(handshake)),
        })
        .await
        .unwrap();
        info!("Handshake sent");

        // 2. Await WelcomeResponse
        info!("Awaiting welcome response");
        let welcome_response = timeout(Duration::from_secs(1), response_stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        info!("Welcome response received");

        match welcome_response.payload {
            Some(IngestResponsePayload::Welcome(welcome)) => {
                assert_eq!(welcome.last_known_sent_nanos, 0);
            }
            _ => panic!("Expected WelcomeResponse"),
        }

        // 3. Send records and await acks
        let records_to_send = 250;
        info!("Spawning ack handle");
        let ack_handle = tokio::spawn(async move {
            let mut acks_received = 0;
            while let Some(Ok(response)) = response_stream.next().await {
                if let Some(IngestResponsePayload::Ack(_)) = response.payload {
                    info!("Ack received");
                    acks_received += 1;
                }
            }
            info!("Ack handle finished");
            acks_received
        });

        info!("Sending {} records", records_to_send);
        for i in 0..records_to_send {
            let record = RawDataRecord {
                sent_nanos: i + 1,
                rtt_nanos: 100,
            };
            tx.send(IngestRequest {
                payload: Some(IngestRequestPayload::Record(record)),
            })
            .await
            .unwrap();
        }

        // Close the client stream
        info!("Dropping client tx");
        drop(tx);

        info!("Awaiting ack handle");
        let acks_received = ack_handle.await.unwrap();
        assert_eq!(acks_received, 2); // For 100 and 200
        info!("Test finished");
    }
}
