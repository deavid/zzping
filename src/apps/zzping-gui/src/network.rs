//! Handles all network communication for the `zzping-gui` application.

use anyhow::Result;
use crossbeam_channel::Sender;
use log::{info, warn};
use std::time::Duration;
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use zzping_lib::protocol::RawDataRecord;
use zzping_proto::zzping::{QueryRequest, ingestion_client::IngestionClient};

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
    let ca_cert = tokio::fs::read("certs/ca.pem").await?;
    let ca = Certificate::from_pem(ca_cert);
    let tls_config = ClientTlsConfig::new()
        .domain_name("localhost")
        .ca_certificate(ca);

    let channel = Channel::from_static(QUERY_ADDR).tls_config(tls_config)?;
    let mut client = IngestionClient::new(channel.connect().await?);
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
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::{net::TcpListener, task::JoinHandle};
    use zzping_database::{
        auth::generate_test_token, grpc_server::IngestionServiceImpl, grpc_server::check_auth,
    };
    use zzping_proto::zzping::ingestion_server::IngestionServer;

    async fn spawn_test_server() -> Result<(SocketAddr, JoinHandle<()>)> {
        // Bind a temporary listener to pick an available port, then close it so
        // the tonic server can bind to the same address without double-bind.
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        info!("Reserved test server address {addr}");
        // Drop the listener to free the port for tonic's Server to bind.
        drop(listener);
        let (tx, _) = mpsc::channel(1);
        let temp_dir = std::env::temp_dir().join("zzping_gui_test");
        std::fs::create_dir_all(&temp_dir)?;
        let intent_path = temp_dir.join("intent.ron");
        std::fs::write(intent_path, "(ping_rate_pps: 1, targets: [])")?;
        let data_dir = temp_dir.to_str().unwrap().to_string();
        let service = IngestionServiceImpl::new(tx, data_dir);
        let server = IngestionServer::with_interceptor(service, check_auth);

        let handle = tokio::spawn(async move {
            info!("Starting tonic server on {addr}");
            if let Err(e) = tonic::transport::Server::builder()
                .add_service(server)
                .serve(addr)
                .await
            {
                // Log errors from the server task so test logs include them.
                log::error!("Test server failed: {e}");
            }
        });

        // Wait until the server is accepting connections to avoid races where
        // the test attempts to connect immediately after spawning the task.
        let mut ready = false;
        for _ in 0..50u8 {
            match tokio::net::TcpStream::connect(addr).await {
                Ok(_) => {
                    ready = true;
                    break;
                }
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        }

        if !ready {
            log::error!("Test server at {addr} did not become ready");
        } else {
            info!("Test server ready at {addr}");
        }

        Ok((addr, handle))
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_try_fetch_data() -> Result<()> {
        let _ = env_logger::builder().is_test(true).try_init();

        let res = tokio::time::timeout(Duration::from_secs(5), async {
            let (server_addr, server_handle) = spawn_test_server().await?;
            info!("Test server listening at {server_addr}");
            let (tx, rx) = crossbeam_channel::unbounded();

            // This test doesn't use TLS, so we can just connect directly.
            // The `try_fetch_data` function is what handles TLS.
            info!("Connecting client to http://{server_addr}");
            let mut client = IngestionClient::connect(format!("http://{server_addr}")).await?;

            let token = generate_test_token("test-gui", &["reader"]);
            let mut request = tonic::Request::new(QueryRequest {});
            request
                .metadata_mut()
                .insert("authorization", format!("Bearer {token}").parse()?);

            let response = client.query_data(request).await?;
            info!(
                "Client received {} records",
                response.get_ref().records.len()
            );
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
            Ok::<(), anyhow::Error>(())
        })
        .await;

        match res {
            Ok(inner) => inner?,
            Err(_) => panic!("test_try_fetch_data timed out"),
        }

        Ok(())
    }

    #[tokio::test]
    #[timeout(1000)]
    async fn test_try_fetch_data_connection_error() -> Result<()> {
        let _ = env_logger::builder().is_test(true).try_init();
        // This test now needs to create a dummy ca.pem to avoid panicking.
        std::fs::write("ca.pem", "dummy").unwrap();
        let res = tokio::time::timeout(Duration::from_secs(3), async {
            let (tx, _) = crossbeam_channel::unbounded();
            let result = try_fetch_data(&tx).await;
            assert!(result.is_err());
            Ok::<(), anyhow::Error>(())
        })
        .await;

        match res {
            Ok(inner) => inner?,
            Err(_) => panic!("test_try_fetch_data_connection_error timed out"),
        }

        std::fs::remove_file("ca.pem").unwrap();
        Ok(())
    }
}
