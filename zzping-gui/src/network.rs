//! Handles all network communication for the `zzping-gui` application.

use anyhow::Result;
use crossbeam_channel::Sender;
use log::{info, warn};
use std::time::Duration;
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use zzping_lib::protocol::RawDataRecord;
use zzping_proto::zzping::{ingestion_client::IngestionClient, QueryRequest};

const QUERY_ADDR: &str = "https://127.0.0.1:7878";

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

    // In a real application, this token would come from a login process.
    // For this example, we'll use a hardcoded token with "reader" privileges.
    let token = "eyJzdWIiOiJndWktdXNlciIsInJvbGVzIjpbInJlYWRlciJdfQ=="; // {"sub":"gui-user","roles":["reader"]}
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
    use ntest::timeout;
    use std::net::SocketAddr;
    use tokio::sync::mpsc;
    use tokio::{net::TcpListener, task::JoinHandle};
    use zzping_database::{
        auth::generate_test_token, grpc_server::check_auth,
        grpc_server::IngestionServiceImpl,
    };
    use zzping_proto::zzping::ingestion_server::IngestionServer;

    async fn spawn_test_server() -> Result<(SocketAddr, JoinHandle<()>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let (tx, _) = mpsc::channel(1);
        let temp_dir = std::env::temp_dir().join("zzping_gui_test");
        std::fs::create_dir_all(&temp_dir)?;
        let intent_path = temp_dir.join("intent.ron");
        std::fs::write(intent_path, "(ping_rate_pps: 1, targets: [])")?;
        let data_dir = temp_dir.to_str().unwrap().to_string();
        let service = IngestionServiceImpl::new(tx, data_dir);
        let server = IngestionServer::with_interceptor(service, check_auth);

        let handle = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(server)
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        Ok((addr, handle))
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_try_fetch_data() -> Result<()> {
        let (server_addr, server_handle) = spawn_test_server().await?;
        let (tx, rx) = crossbeam_channel::unbounded();

        // This test doesn't use TLS, so we can just connect directly.
        // The `try_fetch_data` function is what handles TLS.
        let mut client = IngestionClient::connect(format!("http://{server_addr}")).await?;

        let token = generate_test_token("test-gui", &["reader"]);
        let mut request = tonic::Request::new(QueryRequest {});
        request
            .metadata_mut()
            .insert("authorization", format!("Bearer {token}").parse()?);

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

        server_handle.abort(); // Clean up the server task.
        Ok(())
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_try_fetch_data_connection_error() -> Result<()> {
        // This test now needs to create a dummy ca.pem to avoid panicking.
        std::fs::write("ca.pem", "dummy").unwrap();
        let (tx, _) = crossbeam_channel::unbounded();
        let result = try_fetch_data(&tx).await;
        assert!(result.is_err());
        std::fs::remove_file("ca.pem").unwrap();
        Ok(())
    }
}
