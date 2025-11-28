//! TCP transport connection implementation.
//!
//! This module provides TcpTransport which creates EstablishedConnection instances.

use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error};
use x509_parser::prelude::*;

use zznet_api::{EstablishedConnection, PeerTLSIdentity, TransportError, TransportFrame};

use crate::framing;

/// TCP transport connection with optional TLS.
///
/// This wraps either a plain TCP stream or a TLS-encrypted stream and
/// provides the `into_established()` method for creating EstablishedConnection instances.
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
    ) -> Result<Self, zznet_api::TransportError> {
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
    ) -> Result<Self, zznet_api::TransportError> {
        debug!("Created TLS server transport for {}", peer_addr);
        let peer_identity = Some(Self::extract_identity_from_tls_server(&stream)?);
        Ok(TcpTransport {
            stream: TcpTransportStream::TlsServer(Box::new(stream)),
            peer_addr,
            peer_identity,
        })
    }

    /// Convert this TcpTransport into an EstablishedConnection by spawning I/O tasks.
    pub(crate) fn into_established(self) -> EstablishedConnection {
        let (tx, rx): (mpsc::Sender<TransportFrame>, mpsc::Receiver<TransportFrame>) =
            mpsc::channel(32);
        let (result_tx, result_rx) = mpsc::channel(32);

        // Spawn transport tasks and get their handles
        let (writer_handle, reader_handle) = match self.stream {
            TcpTransportStream::Plain(stream) => spawn_transport_tasks(stream, rx, result_tx),
            TcpTransportStream::TlsClient(stream) => spawn_transport_tasks(*stream, rx, result_tx),
            TcpTransportStream::TlsServer(stream) => spawn_transport_tasks(*stream, rx, result_tx),
        };

        // Create a watcher that resolves when EITHER task finishes.
        // We use select! to detect the first failure/completion, then abort the other
        // task to ensure we don't leak resources (e.g. a reader blocked on socket).
        let watcher = Box::pin(async move {
            use std::pin::pin;

            let mut writer = pin!(writer_handle);
            let mut reader = pin!(reader_handle);

            tokio::select! {
                _ = &mut writer => {
                    // Local shutdown (Actor dropped channel)
                    reader.abort();
                },
                _ = &mut reader => {
                    // Remote shutdown (Peer closed connection or Error)
                    writer.abort();
                },
            }
        });

        EstablishedConnection {
            tx,
            rx: result_rx,
            watcher,
            peer_addr: self.peer_addr.to_string(),
            peer_identity: self.peer_identity,
        }
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
    /// Uses the Directory Model to extract identity from Subject DN:
    /// - OU (OrganizationalUnit): Maps to role
    /// - CN (CommonName): Maps to username
    fn parse_peer_cert_der(cert_der: &[u8]) -> Result<PeerTLSIdentity, TransportError> {
        // Parse the certificate
        let (_, cert) = X509Certificate::from_der(cert_der).map_err(|e| {
            TransportError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse peer certificate: {:?}", e),
            ))
        })?;

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

        // Extract OU (OrganizationalUnit) -> role
        let role = cert
            .subject()
            .iter_organizational_unit()
            .next()
            .ok_or_else(|| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Certificate missing OU (OrganizationalUnit) field",
                ))
            })?
            .as_str()
            .map_err(|_| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "OU field is not a string",
                ))
            })?
            .to_string();

        // Extract CN (CommonName) -> username
        let username = cert
            .subject()
            .iter_common_name()
            .next()
            .ok_or_else(|| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Certificate missing CN (CommonName) field",
                ))
            })?
            .as_str()
            .map_err(|_| {
                TransportError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "CN field is not a string",
                ))
            })?
            .to_string();

        Ok(PeerTLSIdentity { role, username })
    }
}

/// Generic helper function to spawn transport tasks for any stream type.
///
/// This consolidates the identical task-spawning logic used for Plain, TlsClient, and TlsServer streams.
fn spawn_transport_tasks<S>(
    stream: S,
    mut rx: mpsc::Receiver<TransportFrame>,
    result_tx: mpsc::Sender<Result<TransportFrame, TransportError>>,
) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut read_half, mut write_half) = tokio::io::split(stream);

    // Spawn writer task
    let writer_handle = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if let Err(e) = framing::write_frame(&mut write_half, frame.get_bytes()).await {
                error!("Write error: {}", e);
                break;
            }
        }
    });

    // Spawn reader task
    let reader_handle = tokio::spawn(async move {
        loop {
            match framing::read_frame(&mut read_half).await {
                Ok(bytes) => {
                    if result_tx
                        .send(Ok(TransportFrame::from(bytes)))
                        .await
                        .is_err()
                    {
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

    (writer_handle, reader_handle)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
