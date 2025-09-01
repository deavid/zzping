use crate::ping_client::PingResult;
use log::{error, info};
use std::collections::VecDeque;
use std::future::IntoFuture;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use zzping_proto::zzping::{
    ingestion_client::IngestionClient, ingest_request::Payload as IngestRequestPayload,
    AckResponse, HandshakeRequest, IngestRequest, RawDataRecord,
};

async fn timeout<F>(
    duration: Duration,
    future: F,
) -> Result<<F as IntoFuture>::Output, tokio::time::error::Elapsed>
where
    F: IntoFuture,
{
    tokio::time::timeout(duration, future).await
}

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

        // Send handshake first to avoid deadlock
        let handshake = HandshakeRequest {
            source_hostname: source_hostname.clone(),
            target_ip: target_ip.to_string(),
        };
        if timeout(
            Duration::from_secs(1),
            request_tx.send(IngestRequest {
                payload: Some(IngestRequestPayload::Handshake(handshake)),
            }),
        )
        .await
        .is_err()
        {
            error!("Failed to send handshake, channel is full or closed.");
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        let request_stream = ReceiverStream::new(request_rx);

        match timeout(Duration::from_secs(1), client.ingest_stream(request_stream)).await {
            Ok(Ok(response)) => {
                let mut response_stream = response.into_inner();
                info!("gRPC stream established.");

                let (ack_tx, mut ack_rx) = mpsc::channel::<AckResponse>(100);

                // Spawn a task to handle server responses
                tokio::spawn(async move {
                    if let Ok(Some(Ok(response))) =
                        timeout(Duration::from_secs(1), response_stream.next()).await
                    {
                        if let Some(zzping_proto::zzping::ingest_response::Payload::Welcome(
                            welcome,
                        )) = response.payload
                        {
                            info!(
                                "Received welcome response with last_known_sent_nanos: {}",
                                welcome.last_known_sent_nanos
                            );
                        } else {
                            error!("First message from server was not a WelcomeResponse");
                            return;
                        }
                    }

                    while let Ok(Some(Ok(response))) =
                        timeout(Duration::from_secs(5), response_stream.next()).await
                    {
                        if let Some(zzping_proto::zzping::ingest_response::Payload::Ack(ack)) =
                            response.payload
                        {
                            if timeout(Duration::from_secs(1), ack_tx.send(ack))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    info!("Response stream closed.");
                });

                // Main streaming logic
                loop {
                    tokio::select! {
                        res = ping_results_rx.recv() => {
                            if let Some(ping_result) = res {
                                let record = RawDataRecord {
                                    sent_nanos: ping_result.sent_nanos,
                                    rtt_nanos: ping_result.rtt.map_or(u64::MAX, |rtt| rtt.as_nanos() as u64),
                                };

                                if timeout(Duration::from_secs(1), request_tx.send(IngestRequest {
                                    payload: Some(IngestRequestPayload::Record(record.clone())),
                                })).await.is_err() {
                                    error!("gRPC stream broke while sending record.");
                                    break;
                                }
                                buffer.push_back(record);
                            } else {
                                // Ping channel closed, we are done.
                                info!("Ping channel closed, terminating target manager.");
                                return;
                            }
                        }
                        Some(ack) = ack_rx.recv() => {
                            info!("Received ack for sent_nanos: {}", ack.last_acked_sent_nanos);
                            buffer.retain(|r| r.sent_nanos > ack.last_acked_sent_nanos);
                        }
                        else => {
                            break;
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                error!("Failed to establish gRPC stream: {e}. Retrying in 5 seconds.");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(_) => {
                error!("gRPC stream timed out. Retrying in 5 seconds.");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
