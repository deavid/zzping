//! TCP transport connection implementation.
//!
//! This module provides TcpTransport which implements the TransportConnection trait.

use async_trait::async_trait;
use bytes::Bytes;
use std::io;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::{debug, error};

use zznet_api::error::TransportError;
use zznet_api::transport::TransportConnection;

use crate::framing;

/// TCP transport connection with optional TLS.
///
/// This wraps either a plain TCP stream or a TLS-encrypted stream and
/// implements the TransportConnection trait for use with zznet-hello.
pub struct TcpTransport {
    /// The actual stream (plain or TLS).
    stream: TcpTransportStream,
    /// Peer address for logging.
    peer_addr: SocketAddr,
}

enum TcpTransportStream {
    /// Plain TCP connection (for testing).
    Plain(TcpStream),
    /// TLS-encrypted TCP connection (production).
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
    /// TLS server-side connection.
    TlsServer(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}

impl TcpTransport {
    /// Create a plain TCP transport (no encryption).
    ///
    /// This is primarily for testing. Production should use TLS.
    pub fn plain(stream: TcpStream, peer_addr: SocketAddr) -> Self {
        debug!("Created plain TCP transport for {}", peer_addr);
        TcpTransport {
            stream: TcpTransportStream::Plain(stream),
            peer_addr,
        }
    }

    /// Create a TLS client transport.
    pub fn tls_client(
        stream: tokio_rustls::client::TlsStream<TcpStream>,
        peer_addr: SocketAddr,
    ) -> Self {
        debug!("Created TLS client transport for {}", peer_addr);
        TcpTransport {
            stream: TcpTransportStream::Tls(Box::new(stream)),
            peer_addr,
        }
    }

    /// Create a TLS server transport.
    pub fn tls_server(
        stream: tokio_rustls::server::TlsStream<TcpStream>,
        peer_addr: SocketAddr,
    ) -> Self {
        debug!("Created TLS server transport for {}", peer_addr);
        TcpTransport {
            stream: TcpTransportStream::TlsServer(Box::new(stream)),
            peer_addr,
        }
    }
}

#[async_trait]
impl TransportConnection for TcpTransport {
    async fn send(&mut self, data: Bytes) -> Result<(), TransportError> {
        debug!("Sending {} bytes to {}", data.len(), self.peer_addr);

        let result = match &mut self.stream {
            TcpTransportStream::Plain(stream) => framing::write_frame(stream, &data).await,
            TcpTransportStream::Tls(stream) => framing::write_frame(stream.as_mut(), &data).await,
            TcpTransportStream::TlsServer(stream) => {
                framing::write_frame(stream.as_mut(), &data).await
            }
        };

        result.map_err(|e| {
            error!("Send error to {}: {}", self.peer_addr, e);
            TransportError::IoError(e.to_string())
        })
    }

    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        debug!("Waiting to receive frame from {}", self.peer_addr);

        let result = match &mut self.stream {
            TcpTransportStream::Plain(stream) => framing::read_frame(stream).await,
            TcpTransportStream::Tls(stream) => framing::read_frame(stream.as_mut()).await,
            TcpTransportStream::TlsServer(stream) => framing::read_frame(stream.as_mut()).await,
        };

        match result {
            Ok(bytes) => {
                debug!("Received {} bytes from {}", bytes.len(), self.peer_addr);
                Ok(Some(bytes))
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                debug!("Connection closed by peer {}", self.peer_addr);
                Ok(None)
            }
            Err(e) => {
                error!("Receive error from {}: {}", self.peer_addr, e);
                Err(TransportError::IoError(e.to_string()))
            }
        }
    }

    fn peer_addr(&self) -> Option<String> {
        Some(self.peer_addr.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn test_plain_tcp_transport_roundtrip() {
        // Create a TCP listener
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn server task
        let server_handle = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let mut transport = TcpTransport::plain(stream, peer_addr);

            // Receive a message
            let msg = transport.recv().await.unwrap().unwrap();
            assert_eq!(msg.as_ref(), b"Hello from client");

            // Send a response
            transport
                .send(Bytes::from("Hello from server"))
                .await
                .unwrap();
        });

        // Client connects
        let stream = TcpStream::connect(addr).await.unwrap();
        let peer = stream.peer_addr().unwrap();
        let mut transport = TcpTransport::plain(stream, peer);

        // Send a message
        transport
            .send(Bytes::from("Hello from client"))
            .await
            .unwrap();

        // Receive response
        let response = transport.recv().await.unwrap().unwrap();
        assert_eq!(response.as_ref(), b"Hello from server");

        // Wait for server to finish
        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_closed() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn server that closes immediately
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream); // Close connection
        });

        // Client connects
        let stream = TcpStream::connect(addr).await.unwrap();
        let peer = stream.peer_addr().unwrap();
        let mut transport = TcpTransport::plain(stream, peer);

        // Try to receive - should get None (connection closed)
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let result = transport.recv().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_multiple_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let mut transport = TcpTransport::plain(stream, peer_addr);

            // Echo back 3 messages
            for _ in 0..3 {
                let msg = transport.recv().await.unwrap().unwrap();
                transport.send(msg).await.unwrap();
            }
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let peer = stream.peer_addr().unwrap();
        let mut transport = TcpTransport::plain(stream, peer);

        // Send and receive 3 messages
        for i in 1..=3 {
            let msg = format!("Message {}", i);
            transport.send(Bytes::from(msg.clone())).await.unwrap();

            let response = transport.recv().await.unwrap().unwrap();
            assert_eq!(response.as_ref(), msg.as_bytes());
        }

        server_handle.await.unwrap();
    }
}
