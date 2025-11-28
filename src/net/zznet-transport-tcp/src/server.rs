//! TCP server transport implementation.
//!
//! This module provides TcpTransportServer which implements the TransportServer trait.

use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info};

use zznet_api::{EstablishedConnection, TransportError, TransportServer};

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

    /// Run the accept loop, sending accepted connections to the recipient.
    pub fn serve(self, recipient: actix::Recipient<zznet_api::AcceptTransport>) {
        let listener = self.listener;
        let tls_acceptor = self.tls_acceptor;

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((tcp_stream, peer_addr)) => {
                        info!("Accepted connection from {}", peer_addr);

                        let transport_result = if let Some(ref acceptor) = tls_acceptor {
                            acceptor
                                .accept(tcp_stream)
                                .await
                                .map_err(|e| {
                                    error!("TLS handshake failed with {}: {}", peer_addr, e);
                                    TransportError::IoError(std::io::Error::other(format!(
                                        "TLS handshake failed: {}",
                                        e
                                    )))
                                })
                                .and_then(|tls_stream| {
                                    TcpTransport::tls_server(tls_stream, peer_addr)
                                })
                        } else {
                            Ok(TcpTransport::plain(tcp_stream, peer_addr))
                        };

                        match transport_result {
                            Ok(transport) => {
                                let connection = transport.into_established();
                                let peer_addr_str = connection.peer_addr.clone();
                                let peer_identity = connection.peer_identity.clone();
                                let tx = connection.tx;
                                let rx = connection.rx;
                                let msg = zznet_api::AcceptTransport {
                                    tx,
                                    rx,
                                    peer_addr: peer_addr_str,
                                    peer_identity,
                                };
                                if recipient.send(msg).await.is_err() {
                                    error!("Recipient closed, stopping accept loop");
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to create transport for {}: {}", peer_addr, e);
                                // Continue accepting other connections
                            }
                        }
                    }
                    Err(e) => {
                        error!("Accept error: {}", e);
                        // Brief pause before retrying
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }
}

#[async_trait]
impl TransportServer for TcpTransportServer {
    async fn accept(&mut self) -> Result<EstablishedConnection, TransportError> {
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
            Ok(transport.into_established())
        } else {
            debug!("Using plain TCP (no TLS) for {}", peer_addr);
            Ok(TcpTransport::plain(tcp_stream, peer_addr).into_established())
        }
    }
}
