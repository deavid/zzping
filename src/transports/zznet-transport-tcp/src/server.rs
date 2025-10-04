//! TCP server transport implementation.
//!
//! This module provides TcpTransportServer which implements the TransportServer trait.

use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info};

use zznet_api::error::TransportError;
use zznet_api::transport::TransportServer;

use crate::config::TlsConfig;
use crate::connection::TcpTransport;

/// TCP transport server.
///
/// This implements TransportServer to accept connections from TCP clients
/// with optional TLS encryption.
pub struct TcpTransportServer {
    /// The TCP listener bound to an address.
    listener: TcpListener,
    /// TLS acceptor (if None, accepts plain TCP).
    tls_acceptor: Option<TlsAcceptor>,
}

impl TcpTransportServer {
    /// Create a new TCP server with TLS support.
    pub async fn new(addr: &str, tls_config: Option<TlsConfig>) -> Result<Self, TransportError> {
        info!("Creating TCP server on {}", addr);

        let socket_addr: SocketAddr = addr
            .parse()
            .map_err(|e| TransportError::IoError(format!("Invalid address {}: {}", addr, e)))?;

        let listener = TcpListener::bind(socket_addr).await.map_err(|e| {
            error!("Failed to bind to {}: {}", addr, e);
            TransportError::IoError(e.to_string())
        })?;

        let bound_addr = listener
            .local_addr()
            .map_err(|e| TransportError::IoError(e.to_string()))?;
        info!("TCP server bound to {}", bound_addr);

        let tls_acceptor = match tls_config {
            Some(cfg) => {
                let server_config = cfg
                    .build_server_config()
                    .map_err(|e| TransportError::IoError(e.to_string()))?;
                Some(TlsAcceptor::from(Arc::new(server_config)))
            }
            None => {
                debug!("No TLS configuration, accepting plain TCP connections");
                None
            }
        };

        Ok(Self {
            listener,
            tls_acceptor,
        })
    }

    /// Create a plain TCP server (no encryption).
    pub async fn plain(addr: &str) -> Result<Self, TransportError> {
        debug!("Creating plain TCP server (no encryption) on {}", addr);
        Self::new(addr, None).await
    }

    /// Create a TLS-enabled TCP server.
    pub async fn with_tls(addr: &str, tls_config: TlsConfig) -> Result<Self, TransportError> {
        info!("Creating TLS-enabled TCP server on {}", addr);
        Self::new(addr, Some(tls_config)).await
    }

    /// Get the local address the server is bound to.
    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.listener
            .local_addr()
            .map_err(|e| TransportError::IoError(e.to_string()))
    }
}

#[async_trait]
impl TransportServer for TcpTransportServer {
    async fn accept(
        &mut self,
    ) -> Result<Box<dyn zznet_api::transport::TransportConnection>, TransportError> {
        debug!("Waiting for incoming connection");

        let (tcp_stream, peer_addr) = self.listener.accept().await.map_err(|e| {
            error!("Failed to accept connection: {}", e);
            TransportError::IoError(e.to_string())
        })?;

        info!("Accepted connection from {}", peer_addr);

        if let Some(ref acceptor) = self.tls_acceptor {
            let tls_stream = acceptor.accept(tcp_stream).await.map_err(|e| {
                error!("TLS handshake failed with {}: {}", peer_addr, e);
                TransportError::IoError(format!("TLS handshake failed: {}", e))
            })?;

            info!("TLS handshake completed with {}", peer_addr);
            Ok(Box::new(TcpTransport::tls_server(tls_stream, peer_addr)))
        } else {
            debug!("Using plain TCP (no TLS) for {}", peer_addr);
            Ok(Box::new(TcpTransport::plain(tcp_stream, peer_addr)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::TcpTransportClient;
    use zznet_api::transport::TransportClient;

    #[tokio::test]
    async fn test_plain_tcp_server_accept() {
        let server = TcpTransportServer::plain("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            let mut server = server;
            let mut conn = server.accept().await.unwrap();

            let msg = conn.recv().await.unwrap().unwrap();
            conn.send(msg).await.unwrap();
        });

        let client = TcpTransportClient::plain(addr.to_string());
        let mut conn = client.connect().await.unwrap();

        conn.send(bytes::Bytes::from("hello server")).await.unwrap();

        let response = conn.recv().await.unwrap().unwrap();
        assert_eq!(response.as_ref(), b"hello server");

        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_connections() {
        let server = TcpTransportServer::plain("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        let addr_clone = addr;

        let server_handle = tokio::spawn(async move {
            let mut server = server;
            for _i in 0..3 {
                let mut conn = server.accept().await.unwrap();
                tokio::spawn(async move {
                    let msg = conn.recv().await.unwrap().unwrap();
                    conn.send(msg).await.unwrap();
                });
            }
        });

        let mut handles = vec![];

        for i in 0..3 {
            let addr_str = addr_clone.to_string();

            let handle = tokio::spawn(async move {
                let client = TcpTransportClient::plain(addr_str);
                let mut conn = client.connect().await.unwrap();

                let msg = format!("client {}", i);
                conn.send(bytes::Bytes::from(msg.clone())).await.unwrap();

                let response = conn.recv().await.unwrap().unwrap();
                assert_eq!(response.as_ref(), msg.as_bytes());
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_invalid_bind_address() {
        let result = TcpTransportServer::plain("999.999.999.999:8080").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_server_local_addr() {
        let server = TcpTransportServer::plain("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();

        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert!(addr.port() > 0);
    }
}
