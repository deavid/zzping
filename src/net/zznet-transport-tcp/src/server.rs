//! TCP server transport implementation.
//!
//! This module provides TcpTransportServer which implements the TransportServer trait.

use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info};

use zznet_api::{TransportError, TransportServer};

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

        let socket_addr: SocketAddr = addr.parse().map_err(|e| {
            TransportError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid address {}: {}", addr, e),
            ))
        })?;

        let listener = TcpListener::bind(socket_addr).await.map_err(|e| {
            error!("Failed to bind to {}: {}", addr, e);
            TransportError::IoError(e)
        })?;

        let bound_addr = listener.local_addr().map_err(TransportError::IoError)?;
        info!("TCP server bound to {}", bound_addr);

        let tls_acceptor = match tls_config {
            Some(cfg) => {
                let server_config = cfg
                    .build_server_config()
                    .map_err(|e| TransportError::IoError(std::io::Error::other(e.to_string())))?;
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

    /// Get the local address the server is bound to.
    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.listener.local_addr().map_err(TransportError::IoError)
    }
}

#[async_trait]
impl TransportServer for TcpTransportServer {
    async fn accept(&mut self) -> Result<Box<dyn zznet_api::TransportConnection>, TransportError> {
        debug!("Waiting for incoming connection");

        let (tcp_stream, peer_addr) = self.listener.accept().await.map_err(|e| {
            error!("Failed to accept connection: {}", e);
            TransportError::IoError(e)
        })?;

        info!("Accepted connection from {}", peer_addr);

        if let Some(ref acceptor) = self.tls_acceptor {
            let tls_stream = acceptor.accept(tcp_stream).await.map_err(|e| {
                error!("TLS handshake failed with {}: {}", peer_addr, e);
                TransportError::IoError(std::io::Error::other(format!(
                    "TLS handshake failed: {}",
                    e
                )))
            })?;

            info!("TLS handshake completed with {}", peer_addr);
            let transport = TcpTransport::tls_server(tls_stream, peer_addr).map_err(|e| {
                TransportError::IoError(std::io::Error::other(format!(
                    "Failed to extract peer identity: {}",
                    e
                )))
            })?;
            Ok(Box::new(transport))
        } else {
            debug!("Using plain TCP (no TLS) for {}", peer_addr);
            Ok(Box::new(TcpTransport::plain(tcp_stream, peer_addr)))
        }
    }
}
