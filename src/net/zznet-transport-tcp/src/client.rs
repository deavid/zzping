//! TCP client transport implementation.
//!
//! This module provides TcpTransportClient which implements the TransportClient trait.

use async_trait::async_trait;
use rustls::pki_types::ServerName;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tracing::{debug, error, info};

use actix::Recipient;
use zznet_api::{AcceptTransport, TransportClient, TransportError, EstablishedConnection};

use crate::config::{ReconnectConfig, TlsConfig};
use crate::connection::TcpTransport;

/// TCP transport client.
///
/// This implements TransportClient to create connections to TCP servers
/// with optional TLS encryption.
#[derive(Clone)]
pub struct TcpTransportClient {
    /// Target address.
    addr: String,
    /// TLS configuration (if None, uses plain TCP).
    tls_config: Option<Arc<rustls::ClientConfig>>,
    /// Server name for SNI (if TLS is enabled).
    server_name: Option<String>,
}

impl TcpTransportClient {
    /// Get the target address this client connects to.
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Create a new TCP client with TLS support.
    ///
    /// # Arguments
    ///
    /// * `addr` - Target address (e.g., "192.168.1.1:5555")
    /// * `tls_config` - TLS configuration. If None, connections will be plain TCP.
    pub fn new(addr: String, tls_config: Option<TlsConfig>) -> Result<Self, TransportError> {
        let server_name = tls_config.as_ref().map(|cfg| cfg.server_name.clone());
        let tls_config = match tls_config {
            Some(cfg) => {
                let client_config = cfg
                    .build_client_config()
                    .map_err(|e| TransportError::IoError(std::io::Error::other(e.to_string())))?;
                Some(Arc::new(client_config))
            }
            None => None,
        };

        Ok(Self {
            addr,
            tls_config,
            server_name,
        })
    }
}

#[async_trait]
impl TransportClient for TcpTransportClient {
    async fn connect(&self) -> Result<EstablishedConnection, TransportError> {
        info!("Connecting to {}", self.addr);

        // Parse the address
        let socket_addr: SocketAddr = self.addr.parse().map_err(|e| {
            TransportError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid address {}: {}", self.addr, e),
            ))
        })?;

        // Connect TCP stream
        let tcp_stream = TcpStream::connect(socket_addr).await.map_err(|e| {
            error!("Failed to connect to {}: {}", self.addr, e);
            TransportError::IoError(e)
        })?;

        let peer_addr = tcp_stream.peer_addr().map_err(TransportError::IoError)?;

        debug!("TCP connection established to {}", self.addr);

        // If TLS is configured, perform TLS handshake
        if let Some(ref tls_config) = self.tls_config {
            // Use configured server_name for SNI (not the IP address)
            // This allows connecting via any IP while using a consistent SAN name
            let server_name_str = self.server_name.as_ref().ok_or_else(|| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "TLS enabled but no server_name configured",
                ))
            })?;
            let server_name = ServerName::try_from(server_name_str.clone()).map_err(|e| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Invalid server name: {}", e),
                ))
            })?;

            let connector = TlsConnector::from(tls_config.clone());
            let tls_stream = connector
                .connect(server_name, tcp_stream)
                .await
                .map_err(|e| {
                    error!("TLS handshake failed: {}", e);
                    TransportError::IoError(std::io::Error::other(format!(
                        "TLS handshake failed: {}",
                        e
                    )))
                })?;

            info!("TLS handshake completed for {}", self.addr);
            Ok(TcpTransport::tls_client(tls_stream, peer_addr)?.into_established())
        } else {
            debug!("Using plain TCP (no TLS) for {}", self.addr);
            Ok(TcpTransport::plain(tcp_stream, peer_addr).into_established())
        }
    }
}

impl TcpTransportClient {
    /// Maintain a persistent connection to the server, automatically reconnecting on failure.
    ///
    /// This method spawns a background task that continuously attempts to connect to the
    /// configured server. When a connection is established, it sends an `AcceptTransport`
    /// message to the recipient and waits for the connection to die before retrying.
    ///
    /// # Arguments
    /// * `recipient` - The actor that will receive `AcceptTransport` messages for new connections
    /// * `config` - Configuration for reconnection behavior (retry delay, etc.)
    pub fn maintain(self, recipient: Recipient<AcceptTransport>, config: ReconnectConfig) {
        tokio::spawn(async move {
            loop {
                match self.connect().await {
                    Ok(connection) => {
                        info!("Connection established, sending to recipient");

                        let peer_addr = connection.peer_addr.clone();
                        let peer_identity = connection.peer_identity.clone();
                        let tx = connection.tx;
                        let rx = connection.rx;
                        let watcher = connection.watcher;
                        let msg = AcceptTransport {
                            tx,
                            rx,
                            peer_addr,
                            peer_identity,
                        };
                        if recipient.send(msg).await.is_err() {
                            error!("Recipient closed, stopping maintain loop");
                            break;
                        }

                        // Wait for the connection to die
                        watcher.await;
                        info!("Connection died, will reconnect");
                    }
                    Err(e) => {
                        error!("Connection failed: {}", e);
                    }
                }

                info!("Reconnecting in {:?}", config.retry_delay);
                tokio::time::sleep(config.retry_delay).await;
            }
        });
    }
}
