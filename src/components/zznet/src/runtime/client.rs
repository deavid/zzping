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

/// The primary state machine for a client's network connection. It holds the configuration and provides the interface to connect to the server.
pub struct ClientRuntime {
    config: ClientConfig,
}

impl ClientRuntime {
    /// Prepares the runtime with the necessary connection parameters.
    pub fn new(config: ClientConfig) -> Self {
        Self { config }
    }

    /// Returns a stream that perpetually tries to maintain a connection to the server.
    /// Each time a connection is successfully established, a new `Connection` object is yielded by the stream.
    /// If a connection is lost, the stream will internally try to reconnect and will yield a new `Connection` object once it succeeds.
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

    /// Attempts to establish a connection to the server by iterating through the configured socket addresses.
    /// Performs TCP connection and optional TLS handshake. Returns the stream on success or an error if all addresses fail.
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
                                        log::warn!("TLS handshake failed for {}: {}", addr, e)
                                    }
                                }
                            }
                            Err(e) => log::warn!("Failed to build TLS config for {}: {}", addr, e),
                        }
                    } else {
                        return Ok(Box::new(stream));
                    }
                }
                Err(e) => log::warn!("TCP connection failed for {}: {}", addr, e),
            }
        }
        Err(anyhow::anyhow!("Failed to connect to any server address"))
    }
}
