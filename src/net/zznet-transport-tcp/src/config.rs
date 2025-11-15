//! TLS configuration for secure TCP connections.
//!
//! This module provides TLS configuration with mutual authentication,
//! role-based certificate selection, and CA validation.

use rustls;
use rustls_native_certs;
use rustls_pemfile;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

/// TLS configuration errors.
#[derive(Error, Debug)]
pub(crate) enum TlsError {
    #[error("IO error: {0}")]
    /// Underlying I/O error.
    Io(#[from] std::io::Error),

    #[error("TLS error: {0}")]
    /// Error returned by rustls.
    Rustls(#[from] rustls::Error),

    #[error("Certificate error: {0}")]
    /// Certificate parsing or validation failure.
    Certificate(String),

    #[error("No private key found in file")]
    /// No private key was present in the provided PEM file.
    NoPrivateKey,
}

/// Certificate and private key paths for a specific component role.
///
/// Simplifies certificate management by using role-based naming conventions,
/// ensuring each zzping component uses its designated security credentials.
#[derive(Debug, Clone)]
pub struct TlsCertAndKey {
    /// Path to the certificate PEM file.
    pub pem_path: PathBuf,
    /// Path to the private key file.
    pub key_path: PathBuf,
}

impl TlsCertAndKey {
    /// Derives certificate paths from a role name string.
    ///
    /// This is the generic, reusable API that doesn't depend on the concrete `Role` enum.
    /// Applications can use any role name they want, as long as matching certificates exist.
    pub fn from_role_name(role_name: &str, certs_dir: Option<&str>) -> Self {
        let dir = certs_dir.unwrap_or("certs");
        TlsCertAndKey {
            pem_path: format!("{dir}/{role_name}.pem").into(),
            key_path: format!("{dir}/{role_name}.key").into(),
        }
    }
}

/// Complete TLS configuration for rustls connections.
///
/// Centralizes all TLS parameters to ensure consistent, secure communication
/// across all zzping components with mutual TLS authentication.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Certificate and key used by this endpoint.
    pub cert: TlsCertAndKey,
    /// Optional CA certificate path used to verify peers.
    pub ca_cert_path: Option<PathBuf>,
    /// Whether to include native system CAs.
    pub add_native_ca_certs: bool,
    /// Server name used for SNI verification.
    pub server_name: String,
}

impl TlsConfig {
    /// Builds a rustls ClientConfig with mutual TLS authentication.
    ///
    /// The client will:
    /// - Verify server certificates against CA
    /// - Present its own certificate for mutual TLS
    /// - Use configured server name for SNI
    pub(crate) fn build_client_config(&self) -> Result<rustls::ClientConfig, TlsError> {
        let root_store = self.build_root_store()?;
        let (certs, private_key) = self.load_cert_and_key()?;

        let config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(certs, private_key)?;

        Ok(config)
    }

    /// Builds a rustls ServerConfig with mutual TLS verification.
    ///
    /// The server will:
    /// - Verify client certificates against CA
    /// - Present its own certificate
    /// - Require client authentication
    pub(crate) fn build_server_config(&self) -> Result<rustls::ServerConfig, TlsError> {
        let (certs, key) = self.load_cert_and_key()?;
        let root_store = self.build_root_store()?;

        let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| TlsError::Certificate(format!("Failed to build verifier: {}", e)))?;

        let config = rustls::ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)?;

        Ok(config)
    }

    /// Loads certificate and key pair for TLS configuration.
    fn load_cert_and_key(
        &self,
    ) -> Result<
        (
            Vec<rustls::pki_types::CertificateDer<'static>>,
            rustls::pki_types::PrivateKeyDer<'static>,
        ),
        TlsError,
    > {
        let certs = Self::load_certs_from_path(&self.cert.pem_path)?;
        let private_key = Self::load_private_key_from_path(&self.cert.key_path)?;
        Ok((certs, private_key))
    }

    /// Builds root certificate store with CA and optionally system certificates.
    fn build_root_store(&self) -> Result<rustls::RootCertStore, TlsError> {
        let mut root_store = rustls::RootCertStore::empty();

        if let Some(ca_path) = &self.ca_cert_path {
            let ca_certs = Self::load_certs_from_path(ca_path)?;
            for cert in ca_certs {
                root_store
                    .add(cert)
                    .map_err(|e| TlsError::Certificate(e.to_string()))?;
            }
        }

        if self.add_native_ca_certs {
            let native_result = rustls_native_certs::load_native_certs();
            // native_result is CertificateResult { certs: Vec<Certificate>, errors: Vec<Error> }
            for cert in native_result.certs {
                root_store
                    .add(cert)
                    .map_err(|e| TlsError::Certificate(e.to_string()))?;
            }
            // Log errors but don't fail if some native certs couldn't be loaded
            if !native_result.errors.is_empty() {
                tracing::warn!(
                    "Some native certs failed to load: {:?}",
                    native_result.errors
                );
            }
        }

        Ok(root_store)
    }

    fn load_certs_from_path(
        path: &std::path::Path,
    ) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, TlsError> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let certs = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TlsError::Certificate(format!("Failed to parse certificates: {}", e)))?;

        if certs.is_empty() {
            return Err(TlsError::Certificate(
                "No certificates found in file".to_string(),
            ));
        }

        Ok(certs)
    }

    fn load_private_key_from_path(
        path: &std::path::Path,
    ) -> Result<rustls::pki_types::PrivateKeyDer<'static>, TlsError> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let key = rustls_pemfile::private_key(&mut reader)
            .map_err(|e| TlsError::Certificate(format!("Failed to parse private key: {}", e)))?
            .ok_or(TlsError::NoPrivateKey)?;
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_cert_and_key_from_role_name() {
        // Test the new generic API
        let cert = TlsCertAndKey::from_role_name("collector", None);
        assert_eq!(cert.pem_path.to_str().unwrap(), "certs/collector.pem");
        assert_eq!(cert.key_path.to_str().unwrap(), "certs/collector.key");

        // Works with any role name
        let cert = TlsCertAndKey::from_role_name("database", Some("test_certs"));
        assert_eq!(cert.pem_path.to_str().unwrap(), "test_certs/database.pem");
        assert_eq!(cert.key_path.to_str().unwrap(), "test_certs/database.key");

        // Custom role names work too
        let cert = TlsCertAndKey::from_role_name("my-custom-service", None);
        assert_eq!(
            cert.pem_path.to_str().unwrap(),
            "certs/my-custom-service.pem"
        );
        assert_eq!(
            cert.key_path.to_str().unwrap(),
            "certs/my-custom-service.key"
        );
    }
}
