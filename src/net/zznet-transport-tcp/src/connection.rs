//! TCP transport connection implementation.
//!
//! This module provides TcpTransport which implements the TransportConnection trait.

use async_trait::async_trait;
use bytes::Bytes;
use std::io;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::trace;
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
pub struct TcpTransport {
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
    pub fn plain(stream: TcpStream, peer_addr: SocketAddr) -> Self {
        debug!("Created plain TCP transport for {}", peer_addr);
        // For plain TCP, create a dummy identity (will not be used since peer_identity() returns None)
        TcpTransport {
            stream: TcpTransportStream::Plain(stream),
            peer_addr,
            peer_identity: None,
        }
    }

    /// Create a TLS client transport.
    pub fn tls_client(
        stream: tokio_rustls::client::TlsStream<TcpStream>,
        peer_addr: SocketAddr,
    ) -> Result<Self, zznet_api::error::TransportError> {
        debug!("Created TLS client transport for {}", peer_addr);
        let peer_identity = Some(Self::extract_identity_from_tls_client(&stream, peer_addr)?);
        Ok(TcpTransport {
            stream: TcpTransportStream::TlsClient(Box::new(stream)),
            peer_addr,
            peer_identity,
        })
    }

    /// Create a TLS server transport.
    pub fn tls_server(
        stream: tokio_rustls::server::TlsStream<TcpStream>,
        peer_addr: SocketAddr,
    ) -> Result<Self, zznet_api::error::TransportError> {
        debug!("Created TLS server transport for {}", peer_addr);
        let peer_identity = Some(Self::extract_identity_from_tls_server(&stream, peer_addr)?);
        Ok(TcpTransport {
            stream: TcpTransportStream::TlsServer(Box::new(stream)),
            peer_addr,
            peer_identity,
        })
    }

    /// Extracts peer identity from a TLS connection's certificate.
    fn extract_identity_from_tls_client(
        stream: &tokio_rustls::client::TlsStream<TcpStream>,
        peer_addr: SocketAddr,
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
        Self::parse_peer_cert_der(cert_der, peer_addr)
    }

    /// Extracts peer identity from a TLS server connection's certificate.
    fn extract_identity_from_tls_server(
        stream: &tokio_rustls::server::TlsStream<TcpStream>,
        peer_addr: SocketAddr,
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

        Self::parse_peer_cert_der(cert_der, peer_addr)
    }

    /// Parse a single DER-encoded certificate and extract the PeerIdentity.
    ///
    /// Exposed privately so unit tests can validate parsing behavior without
    /// constructing a full `TlsStream`.
    fn parse_peer_cert_der(
        cert_der: &[u8],
        peer_addr: SocketAddr,
    ) -> Result<PeerTLSIdentity, TransportError> {
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
            peer_addr: peer_addr.to_string(),
        })
    }
}

#[async_trait]
impl TransportConnection for TcpTransport {
    async fn send(&mut self, data: Bytes) -> Result<(), TransportError> {
        trace!("Sending {} bytes to {}", data.len(), self.peer_addr);

        let result = match &mut self.stream {
            TcpTransportStream::Plain(stream) => framing::write_frame(stream, &data).await,
            TcpTransportStream::TlsClient(stream) => {
                framing::write_frame(stream.as_mut(), &data).await
            }
            TcpTransportStream::TlsServer(stream) => {
                framing::write_frame(stream.as_mut(), &data).await
            }
        };

        result.map_err(|e| {
            error!("Send error to {}: {}", self.peer_addr, e);
            TransportError::IoError(e)
        })
    }

    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        trace!("Waiting to receive frame from {}", self.peer_addr);

        let result = match &mut self.stream {
            TcpTransportStream::Plain(stream) => framing::read_frame(stream).await,
            TcpTransportStream::TlsClient(stream) => framing::read_frame(stream.as_mut()).await,
            TcpTransportStream::TlsServer(stream) => framing::read_frame(stream.as_mut()).await,
        };

        match result {
            Ok(bytes) => {
                trace!("Received {} bytes from {}", bytes.len(), self.peer_addr);
                Ok(Some(bytes))
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                debug!("Connection closed by peer {}", self.peer_addr);
                Ok(None)
            }
            Err(e) => {
                error!("Receive error from {}: {}", self.peer_addr, e);
                Err(TransportError::IoError(e))
            }
        }
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
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
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
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let id = TcpTransport::parse_peer_cert_der(der, addr).expect("failed to parse cert DER");

        // Expect the CN to be 'database' and SAN to be 'zzping' (updated dev certs)
        assert_eq!(id.common_name, "database");
        assert_eq!(id.san_username, "zzping");
    }

    #[test]
    fn test_parse_peer_cert_der_invalid_der() {
        let bad = b"not a der";
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let res = TcpTransport::parse_peer_cert_der(bad, addr);
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

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let res = TcpTransport::parse_peer_cert_der(&der, addr);
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

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let res = TcpTransport::parse_peer_cert_der(der.as_slice(), addr);
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

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let res = TcpTransport::parse_peer_cert_der(der.as_slice(), addr);
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
        let addr: SocketAddr = "127.0.0.1:5555".parse().unwrap();

        let identity = TcpTransport::parse_peer_cert_der(cert_der, addr)
            .expect("Failed to parse collector cert");

        assert_eq!(identity.common_name, "collector");
        assert_eq!(identity.san_username, "zzping");
        assert_eq!(identity.peer_addr, "127.0.0.1:5555");
    }
}
