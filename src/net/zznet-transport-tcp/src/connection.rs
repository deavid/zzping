//! TCP transport connection implementation.
//!
//! This module provides TcpTransport which implements the TransportConnection trait.

use async_trait::async_trait;
use bytes::Bytes;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error};
use x509_parser::prelude::*;

use zznet_api::error::TransportError;
use zznet_api::transport::TransportConnection;
use zznet_api::types::PeerTLSIdentity;

use crate::framing;

/// TCP transport connection with optional TLS.
///
/// This wraps either a plain TCP stream or a TLS-encrypted stream and
/// implements the TransportConnection trait for use with zznet-hello.
pub(crate) struct TcpTransport {
    /// The actual stream (plain or TLS).
    stream: TcpTransportStream,
    /// Peer address for logging.
    peer_addr: SocketAddr,
    /// Cached peer identity extracted from certificate or HELLO.
    peer_identity: Option<PeerTLSIdentity>,
}

enum TcpTransportStream {
    /// Plain TCP connection.
    Plain(TcpStream),
    /// TLS-encrypted TCP client-side connection.
    TlsClient(Box<tokio_rustls::client::TlsStream<TcpStream>>),
    /// TLS server-side connection.
    TlsServer(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}

impl TcpTransport {
    /// Create a plain TCP transport (no encryption).
    pub(crate) fn plain(stream: TcpStream, peer_addr: SocketAddr) -> Self {
        debug!("Created plain TCP transport for {}", peer_addr);
        // For plain TCP, create a dummy identity (will not be used since peer_identity() returns None)
        TcpTransport {
            stream: TcpTransportStream::Plain(stream),
            peer_addr,
            peer_identity: None,
        }
    }

    /// Create a TLS client transport.
    pub(crate) fn tls_client(
        stream: tokio_rustls::client::TlsStream<TcpStream>,
        peer_addr: SocketAddr,
    ) -> Result<Self, zznet_api::error::TransportError> {
        debug!("Created TLS client transport for {}", peer_addr);
        let peer_identity = Some(Self::extract_identity_from_tls_client(&stream)?);
        Ok(TcpTransport {
            stream: TcpTransportStream::TlsClient(Box::new(stream)),
            peer_addr,
            peer_identity,
        })
    }

    /// Create a TLS server transport.
    pub(crate) fn tls_server(
        stream: tokio_rustls::server::TlsStream<TcpStream>,
        peer_addr: SocketAddr,
    ) -> Result<Self, zznet_api::error::TransportError> {
        debug!("Created TLS server transport for {}", peer_addr);
        let peer_identity = Some(Self::extract_identity_from_tls_server(&stream)?);
        Ok(TcpTransport {
            stream: TcpTransportStream::TlsServer(Box::new(stream)),
            peer_addr,
            peer_identity,
        })
    }

    /// Extracts peer identity from a TLS connection's certificate.
    fn extract_identity_from_tls_client(
        stream: &tokio_rustls::client::TlsStream<TcpStream>,
    ) -> Result<PeerTLSIdentity, TransportError> {
        // Get the peer certificates from the rustls session
        let (_, conn) = stream.get_ref();
        let peer_certs_opt = conn.peer_certificates();
        let peer_certs = match peer_certs_opt {
            Some(v) if !v.is_empty() => v,
            _ => {
                return Err(TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "TLS handshake completed but no peer certificate",
                )));
            }
        };

        // Take the first (leaf) certificate
        let cert_der = match peer_certs.first() {
            Some(c) => c.as_ref(),
            None => {
                return Err(TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Peer certificate list is empty",
                )));
            }
        };

        // Delegate to shared parser
        Self::parse_peer_cert_der(cert_der)
    }

    /// Extracts peer identity from a TLS server connection's certificate.
    fn extract_identity_from_tls_server(
        stream: &tokio_rustls::server::TlsStream<TcpStream>,
    ) -> Result<PeerTLSIdentity, TransportError> {
        // Similar to client, but for server stream
        let (_, conn) = stream.get_ref();
        let peer_certs_opt = conn.peer_certificates();
        let peer_certs = match peer_certs_opt {
            Some(v) if !v.is_empty() => v,
            _ => {
                return Err(TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "TLS handshake completed but no peer certificate",
                )));
            }
        };

        let cert_der = peer_certs
            .first()
            .ok_or_else(|| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Peer certificate list is empty",
                ))
            })?
            .as_ref();

        Self::parse_peer_cert_der(cert_der)
    }

    /// Parse a single DER-encoded certificate and extract the PeerIdentity.
    ///
    /// Exposed privately so unit tests can validate parsing behavior without
    /// constructing a full `TlsStream`.
    fn parse_peer_cert_der(cert_der: &[u8]) -> Result<PeerTLSIdentity, TransportError> {
        // Parse the certificate
        let (_, cert) = X509Certificate::from_der(cert_der).map_err(|e| {
            TransportError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse peer certificate: {:?}", e),
            ))
        })?;

        // Extract CN
        let common_name = cert
            .subject()
            .iter_common_name()
            .next()
            .ok_or_else(|| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Certificate missing CN",
                ))
            })?
            .as_str()
            .map_err(|_| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "CN is not a string",
                ))
            })?
            .to_string();

        // Check certificate validity period using SystemTime
        let not_before = cert.validity().not_before.to_datetime();
        let not_after = cert.validity().not_after.to_datetime();
        let now = std::time::SystemTime::now();
        if now < not_before {
            return Err(TransportError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Certificate not yet valid",
            )));
        }
        if now > not_after {
            return Err(TransportError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Certificate expired",
            )));
        }

        // Extract SAN extension and parse
        let san_ext = cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
            .ok_or_else(|| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Certificate missing SAN extension",
                ))
            })?;

        let (_, san) = x509_parser::extensions::SubjectAlternativeName::from_der(san_ext.value)
            .map_err(|e| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to parse SAN extension: {:?}", e),
                ))
            })?;

        // Find the first DNS name
        let san_username = san
            .general_names
            .iter()
            .find_map(|name| match name {
                x509_parser::extensions::GeneralName::DNSName(dns) => Some(dns.to_string()),
                _ => None,
            })
            .ok_or_else(|| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "SAN contains no DNS names",
                ))
            })?;

        Ok(PeerTLSIdentity {
            common_name,
            san_username,
        })
    }
}

/// Generic helper function to spawn transport tasks for any stream type.
///
/// This consolidates the identical task-spawning logic used for Plain, TlsClient, and TlsServer streams.
fn spawn_transport_tasks<S>(
    stream: S,
    mut rx: mpsc::Receiver<Bytes>,
    result_tx: mpsc::Sender<Result<Bytes, TransportError>>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut read_half, mut write_half) = tokio::io::split(stream);

    // Spawn writer task
    tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if let Err(e) = framing::write_frame(&mut write_half, &bytes).await {
                error!("Write error: {}", e);
                break;
            }
        }
    });

    // Spawn reader task
    tokio::spawn(async move {
        loop {
            match framing::read_frame(&mut read_half).await {
                Ok(bytes) => {
                    if result_tx.send(Ok(bytes)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = result_tx.send(Err(e.into())).await;
                    break;
                }
            }
        }
    });
}

#[async_trait]
impl TransportConnection for TcpTransport {
    fn start(
        self: Box<Self>,
    ) -> (
        mpsc::Sender<Bytes>,
        mpsc::Receiver<Result<Bytes, TransportError>>,
    ) {
        let (tx, rx): (mpsc::Sender<Bytes>, mpsc::Receiver<Bytes>) = mpsc::channel(32);
        let (result_tx, result_rx) = mpsc::channel(32);

        // Split the stream into read and write halves
        match self.stream {
            TcpTransportStream::Plain(stream) => {
                spawn_transport_tasks(stream, rx, result_tx);
            }
            TcpTransportStream::TlsClient(stream) => {
                spawn_transport_tasks(*stream, rx, result_tx);
            }
            TcpTransportStream::TlsServer(stream) => {
                spawn_transport_tasks(*stream, rx, result_tx);
            }
        }

        (tx, result_rx)
    }

    fn peer_addr(&self) -> Option<String> {
        Some(self.peer_addr.to_string())
    }

    fn peer_tls_identity(&self) -> Option<PeerTLSIdentity> {
        self.peer_identity.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tokio::net::{TcpListener, TcpStream};
    use x509_parser::pem::parse_x509_pem;

    #[tokio::test]
    async fn test_plain_tcp_transport_roundtrip() {
        // Create a TCP listener
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn server task
        let server_handle = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let transport = TcpTransport::plain(stream, peer_addr);
            let (tx, mut rx) = Box::new(transport).start();

            // Receive a message
            let msg = rx.recv().await.unwrap().unwrap();
            assert_eq!(msg.as_ref(), b"Hello from client");

            // Send a response
            tx.send(Bytes::from("Hello from server")).await.unwrap();
        });

        // Client connects
        let stream = TcpStream::connect(addr).await.unwrap();
        let peer = stream.peer_addr().unwrap();
        let transport = TcpTransport::plain(stream, peer);
        let (tx, mut rx) = Box::new(transport).start();

        // Send a message
        tx.send(Bytes::from("Hello from client")).await.unwrap();

        // Receive response
        let response = rx.recv().await.unwrap().unwrap();
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
        let transport = TcpTransport::plain(stream, peer);
        let (_tx, mut rx) = Box::new(transport).start();

        // Try to receive - should be an error for connection closed
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        let result = rx.recv().await;
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap(),
            Err(TransportError::ConnectionClosed(_))
        ));
    }

    #[tokio::test]
    async fn test_multiple_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let transport = TcpTransport::plain(stream, peer_addr);
            let (tx, mut rx) = Box::new(transport).start();

            // Echo back 3 messages
            for _ in 0..3 {
                let msg = rx.recv().await.unwrap().unwrap();
                tx.send(msg).await.unwrap();
            }
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let peer = stream.peer_addr().unwrap();
        let transport = TcpTransport::plain(stream, peer);
        let (tx, mut rx) = Box::new(transport).start();

        // Send and receive 3 messages
        for i in 1..=3 {
            let msg = format!("Message {}", i);
            tx.send(Bytes::from(msg.clone())).await.unwrap();

            let response = rx.recv().await.unwrap().unwrap();
            assert_eq!(response.as_ref(), msg.as_bytes());
        }

        server_handle.await.unwrap();
    }

    #[test]
    #[ignore]
    fn test_parse_peer_cert_der_from_pem() {
        // Load the PEM file from test_certs
        let pem_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../test_certs/database.pem"
        );
        let pem_data = fs::read(pem_path).expect("failed to read test cert pem");
        let (_rem, pem) = parse_x509_pem(&pem_data).expect("failed to parse PEM");
        let der = pem.contents.as_slice();

        // Use loopback addr as peer addr
        let id = TcpTransport::parse_peer_cert_der(der).expect("failed to parse cert DER");

        // Expect the CN to be 'database' and SAN to be 'zzping' (updated dev certs)
        assert_eq!(id.common_name, "database");
        assert_eq!(id.san_username, "zzping");
    }

    #[test]
    fn test_parse_peer_cert_der_invalid_der() {
        let bad = b"not a der";
        let res = TcpTransport::parse_peer_cert_der(bad);
        assert!(res.is_err());
        if let Err(e) = res {
            match e {
                TransportError::IoError(_) => {}
                _ => panic!("expected IoError for invalid DER"),
            }
        }
    }

    #[test]
    fn test_parse_peer_cert_der_rejects_expired_cert() {
        // Generate a certificate with a validity period in the past using rcgen
        let mut params = rcgen::CertificateParams::new(vec![]).expect("failed to create params");
        // set CN
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "expired-test");
        // set validity into the past
        params.not_before = rcgen::date_time_ymd(2000, 1, 1);
        params.not_after = rcgen::date_time_ymd(2000, 1, 2);

        let signing_key = rcgen::KeyPair::generate().expect("failed to generate key");
        let cert = params
            .self_signed(&signing_key)
            .expect("failed to create rcgen cert");
        let der = cert.der().to_vec();

        let res = TcpTransport::parse_peer_cert_der(&der);
        assert!(res.is_err(), "expected expired cert to be rejected");
    }

    #[test]
    fn test_parse_peer_cert_der_missing_cn() {
        // Generate a cert with SAN but no CN using rcgen
        let mut params = rcgen::CertificateParams::new(vec!["example.com".to_string()])
            .expect("failed to create params");
        // Intentionally leave subject_alt_names but clear the distinguished_name
        params.distinguished_name = rcgen::DistinguishedName::new();
        // Add DNS SAN
        params.subject_alt_names.push(rcgen::SanType::DnsName(
            rcgen::string::Ia5String::try_from("alice").unwrap(),
        ));

        let signing_key = rcgen::KeyPair::generate().expect("failed to generate key");
        let cert = params
            .self_signed(&signing_key)
            .expect("failed to build cert");
        let der = cert.der().to_vec();

        let res = TcpTransport::parse_peer_cert_der(der.as_slice());
        assert!(res.is_err());
        if let Err(e) = res {
            match e {
                TransportError::IoError(_) => {}
                _ => panic!("expected IoError for missing CN"),
            }
        }
    }

    #[test]
    fn test_parse_peer_cert_der_missing_san() {
        // Generate a cert with CN but no SAN using rcgen
        let mut params = rcgen::CertificateParams::new(vec![]).expect("failed to create params");
        // Set CN via distinguished name
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "database");

        let signing_key = rcgen::KeyPair::generate().expect("failed to generate key");
        let cert = params
            .self_signed(&signing_key)
            .expect("failed to build cert");
        let der = cert.der().to_vec();

        let res = TcpTransport::parse_peer_cert_der(der.as_slice());
        assert!(res.is_err());
        if let Err(e) = res {
            match e {
                TransportError::IoError(_) => {}
                _ => panic!("expected IoError for missing SAN"),
            }
        }
    }

    #[test]
    #[ignore]
    fn test_parse_collector_cert() {
        // Load the test collector certificate and verify parsing
        let cert_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("test_certs/collector.pem");

        let pem_data = std::fs::read(&cert_path).expect("Failed to read collector.pem");
        let (_rem, pem) = parse_x509_pem(&pem_data).expect("Failed to parse PEM");
        let cert_der = pem.contents.as_slice();

        let identity =
            TcpTransport::parse_peer_cert_der(cert_der).expect("Failed to parse collector cert");

        assert_eq!(identity.common_name, "collector");
        assert_eq!(identity.san_username, "zzping");
    }
}
