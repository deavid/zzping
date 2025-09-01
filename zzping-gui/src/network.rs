//! Handles all network communication for the `zzping-gui` application.

use anyhow::Result;
use crossbeam_channel::Sender;
use log::{info, warn};
use std::time::Duration;
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use zzping_lib::protocol::RawDataRecord;
use zzping_proto::zzping::{QueryRequest, ingestion_client::IngestionClient};

const QUERY_ADDR: &str = "https://127.0.0.1:7878";

/// The main loop for the network background task.
///
/// This function runs in a separate thread and is responsible for periodically
/// fetching data from the `zzping-database`. It runs in an infinite loop,
/// attempting to fetch data once per second. If a connection or fetch fails,
/// it logs a warning and retries after a 5-second delay.
///
/// # Arguments
/// * `tx` - The sender part of a `crossbeam-channel` used to send the fetched
///   data back to the main UI thread.
pub async fn fetch_data_loop(tx: Sender<Vec<RawDataRecord>>) {
    info!("Network task started.");
    loop {
        match try_fetch_data(&tx).await {
            Ok(_) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => {
                warn!("Failed to fetch data: {e}. Retrying in 5 seconds.");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Attempts to perform a single fetch-and-send operation.
///
/// This function connects to the database, sends the `GET_LAST_MINUTE` command,
/// and uses the centralized `read_records_batch` helper to read the response.
/// It then sends the resulting `Vec<RawDataRecord>` over the channel to the UI thread.
pub async fn try_fetch_data(tx: &Sender<Vec<RawDataRecord>>) -> Result<()> {
    let ca_cert = tokio::fs::read("ca.pem").await?;
    let ca = Certificate::from_pem(ca_cert);
    let tls_config = ClientTlsConfig::new()
        .domain_name("localhost")
        .ca_certificate(ca);

    let channel = Channel::from_static(QUERY_ADDR)
        .tls_config(tls_config)?
        .connect()
        .await?;
    let mut client = IngestionClient::new(channel);
    info!("Connected to query port at {QUERY_ADDR}");

    let token = "my-secret-token";
    let mut request = tonic::Request::new(QueryRequest {});
    request
        .metadata_mut()
        .insert("authorization", format!("Bearer {token}").parse()?);
    let response = client.query_data(request).await?;
    info!(
        "Received {} records from database.",
        response.get_ref().records.len()
    );

    let records = response
        .into_inner()
        .records
        .into_iter()
        .map(|r| RawDataRecord {
            sent_nanos: r.sent_nanos,
            rtt_nanos: r.rtt_nanos,
        })
        .collect();

    tx.send(records)
        .map_err(|e| anyhow::anyhow!("UI channel closed: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use zzping_database::grpc_server::IngestionServiceImpl;
    use zzping_proto::zzping::ingestion_server::IngestionServer;

    async fn spawn_test_server() -> Result<SocketAddr> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let (tx, _) = mpsc::channel(1);
        let temp_dir = std::env::temp_dir().join("zzping_gui_test");
        std::fs::create_dir_all(&temp_dir)?;
        let data_dir = temp_dir.to_str().unwrap().to_string();
        let service = IngestionServiceImpl::new(tx, data_dir);
        let server = IngestionServer::new(service);

        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(server)
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        Ok(addr)
    }

    #[tokio::test]
    async fn test_try_fetch_data() -> Result<()> {
        std::fs::create_dir_all(zzping_database::DATA_DIR).unwrap();
        let server_addr = spawn_test_server().await?;
        let (tx, rx) = crossbeam_channel::unbounded();

        // This is a bit of a hack, but we need to create a dummy ca.pem for the test to run.
        std::fs::write("ca.pem", "dummy").unwrap();

        let mut client = IngestionClient::connect(format!("http://{server_addr}")).await?;
        let request = tonic::Request::new(QueryRequest {});
        let response = client.query_data(request).await?;
        let records: Vec<RawDataRecord> = response
            .into_inner()
            .records
            .into_iter()
            .map(|r| RawDataRecord {
                sent_nanos: r.sent_nanos,
                rtt_nanos: r.rtt_nanos,
            })
            .collect();
        tx.send(records)?;

        let received_records = rx.recv()?;
        assert!(received_records.is_empty());

        std::fs::remove_file("ca.pem").unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn test_try_fetch_data_connection_error() -> Result<()> {
        let (tx, _) = crossbeam_channel::unbounded();
        // This will fail because there is no server running on this port.
        let result = try_fetch_data(&tx).await;
        assert!(result.is_err());
        Ok(())
    }
}
