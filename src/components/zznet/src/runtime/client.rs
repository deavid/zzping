use crate::connection::ClientConfig;
use crate::connection_manager::Connection;
use crate::traits::AsyncReadWrite;
use anyhow::Result;
use async_stream::stream;
use futures::Stream;
use log;
use rustls::pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;

/// Manages resilient client connections with automatic reconnection.
pub struct ClientRuntime {
    config: ClientConfig,
}

impl ClientRuntime {
    /// Creates a new client runtime ready for resilient connections.
    pub fn new(config: ClientConfig) -> Self {
        Self { config }
    }

    /// Provides a stream of connections with automatic reconnection on failure.
    pub fn connections(self) -> impl Stream<Item = Result<Connection>> {
        stream! {
            loop {
                match self.try_connect().await {
                    Ok(stream) => {
                        // Create a dummy event channel that drops events, since the client binary doesn't use events
                        let (event_tx, mut event_rx) = mpsc::channel(32);
                        tokio::spawn(async move {
                            while (event_rx.recv().await).is_some() {
                                // Drop the event
                            }
                        });
                        yield Ok(Connection::new(stream, event_tx));
                    }
                    Err(e) => {
                        log::warn!("Failed to connect: {}. Retrying in {:?}...", e, self.config.reconnect_delay);
                    }
                }
                // Even if the connection was closed properly, we must wait before trying again to avoid exhausting resources.
                tokio::time::sleep(self.config.reconnect_delay).await;
            }
        }
    }

    /// Attempts connection to any configured server with fallback addresses.
    async fn try_connect(&self) -> Result<Box<dyn AsyncReadWrite + Send + Unpin>> {
        for addr in &self.config.socketaddr {
            match TcpStream::connect(addr).await {
                Ok(stream) => {
                    if let Some(tls_cfg) = &self.config.tls {
                        match tls_cfg.build_client_config() {
                            Ok(client_config) => {
                                let connector = TlsConnector::from(Arc::new(client_config));
                                let domain = ServerName::try_from(tls_cfg.common_name.clone())
                                    .map_err(|e| anyhow::anyhow!("Invalid server name: {}", e))?;
                                match connector.connect(domain, stream).await {
                                    Ok(tls_stream) => return Ok(Box::new(tls_stream)),
                                    Err(e) => {
                                        log::warn!("TLS handshake failed for {addr}: {e}")
                                    }
                                }
                            }
                            Err(e) => log::warn!("Failed to build TLS config for {addr}: {e}"),
                        }
                    } else {
                        return Ok(Box::new(stream));
                    }
                }
                Err(e) => log::warn!("TCP connection failed for {addr}: {e}"),
            }
        }
        Err(anyhow::anyhow!("Failed to connect to any server address"))
    }
}
