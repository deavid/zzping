use crate::ping_client::PingResult;
use log::{error, info};
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use zzping_proto::zzping::{
    ingestion_client::IngestionClient, ingest_request::Payload as IngestRequestPayload,
    AckResponse, HandshakeRequest, IngestRequest, RawDataRecord,
};

pub async fn run_target_manager(
    mut client: IngestionClient<tonic::transport::Channel>,
    source_hostname: String,
    target_ip: std::net::IpAddr,
    mut ping_results_rx: mpsc::Receiver<PingResult>,
) {
    let mut buffer: VecDeque<RawDataRecord> = VecDeque::new();

    loop {
        info!("Attempting to connect to gRPC server...");
        let (request_tx, request_rx) = mpsc::channel(100);
        let request_stream = ReceiverStream::new(request_rx);

        match client.ingest_stream(request_stream).await {
            Ok(response) => {
                let mut response_stream = response.into_inner();
                info!("gRPC stream established.");

                // Send handshake
                let handshake = HandshakeRequest {
                    source_hostname: source_hostname.clone(),
                    target_ip: target_ip.to_string(),
                };
                if request_tx
                    .send(IngestRequest {
                        payload: Some(IngestRequestPayload::Handshake(handshake)),
                    })
                    .await
                    .is_err()
                {
                    error!("Failed to send handshake, channel closed prematurely.");
                    continue;
                }

                let (ack_tx, mut ack_rx) = mpsc::channel::<AckResponse>(100);

                // Spawn a task to handle server responses
                tokio::spawn(async move {
                    if let Some(Ok(response)) = response_stream.next().await {
                        if let Some(zzping_proto::zzping::ingest_response::Payload::Welcome(
                            welcome,
                        )) = response.payload
                        {
                            info!(
                                "Received welcome response with last_known_sent_nanos: {}",
                                welcome.last_known_sent_nanos
                            );
                            // Here we would normally handle buffer replay logic
                        } else {
                            error!("First message from server was not a WelcomeResponse");
                            return;
                        }
                    }

                    while let Some(Ok(response)) = response_stream.next().await {
                        if let Some(zzping_proto::zzping::ingest_response::Payload::Ack(ack)) =
                            response.payload
                        {
                            if ack_tx.send(ack).await.is_err() {
                                // Main loop closed, just exit
                                break;
                            }
                        }
                    }
                    info!("Response stream closed.");
                });

                // Main streaming logic
                loop {
                    tokio::select! {
                        Some(ping_result) = ping_results_rx.recv() => {
                            let record = RawDataRecord {
                                sent_nanos: ping_result.sent_nanos,
                                rtt_nanos: ping_result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64),
                            };

                            if request_tx.send(IngestRequest {
                                payload: Some(IngestRequestPayload::Record(record.clone())),
                            }).await.is_err() {
                                error!("gRPC stream broke while sending record.");
                                break;
                            }
                            buffer.push_back(record);
                        }
                        Some(ack) = ack_rx.recv() => {
                            info!("Received ack for sent_nanos: {}", ack.last_acked_sent_nanos);
                            buffer.retain(|r| r.sent_nanos > ack.last_acked_sent_nanos);
                        }
                        else => {
                            // ping_results_rx and ack_rx are closed, so we are done.
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to establish gRPC stream: {}. Retrying in 5 seconds.", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
