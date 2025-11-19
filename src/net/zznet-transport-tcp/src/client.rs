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

use zznet_api::error::TransportError;
use zznet_api::transport::TransportClient;

use crate::config::TlsConfig;
use crate::connection::TcpTransport;

/// TCP transport client.
///
/// This implements TransportClient to create connections to TCP servers
/// with optional TLS encryption.
pub struct TcpTransportClient {
    /// Target address.
    addr: String,
    /// TLS configuration (if None, uses plain TCP).
    tls_config: Option<Arc<rustls::ClientConfig>>,
    /// Server name for SNI (if TLS is enabled).
    server_name: Option<String>,
}

impl TcpTransportClient {
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
    async fn connect(
        &self,
    ) -> Result<Box<dyn zznet_api::transport::TransportConnection>, TransportError> {
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
            Ok(Box::new(TcpTransport::tls_client(tls_stream, peer_addr)?))
        } else {
            debug!("Using plain TCP (no TLS) for {}", self.addr);
            Ok(Box::new(TcpTransport::plain(tcp_stream, peer_addr)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use zznet_api::transport::TransportConnection;

    #[tokio::test]
    async fn test_plain_tcp_client_connect() {
        // Start a test server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let transport = TcpTransport::plain(stream, peer_addr);
            let (tx, mut rx) = Box::new(transport).start();

            // Echo back any message
            let msg = rx.recv().await.unwrap().unwrap();
            tx.send(msg).await.unwrap();
        });

        // Create client and connect
        let client = TcpTransportClient::new(addr.to_string(), None).unwrap();
        let conn = client.connect().await.unwrap();
        let (tx, mut rx) = conn.start();

        // Send a message
        tx.send(bytes::Bytes::from("test message")).await.unwrap();

        // Receive echo
        let response = rx.recv().await.unwrap().unwrap();
        assert_eq!(response.as_ref(), b"test message");

        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_refused() {
        // Try to connect to a port that's not listening
        let client = TcpTransportClient::new("127.0.0.1:1".to_string(), None).unwrap();
        let result = client.connect().await;

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, TransportError::IoError(_)));
    }

    #[tokio::test]
    async fn test_invalid_address() {
        let client = TcpTransportClient::new("invalid:address".to_string(), None).unwrap();
        let result = client.connect().await;

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, TransportError::IoError(_)));
    }
}
