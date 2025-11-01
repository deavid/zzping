//! TLS configuration and certificate loading utilities.
//!
//! This module provides utilities for loading TLS certificates and keys from PEM files,
//! building rustls configurations for both client and server use cases.
//!
//! # Examples
//!
//! ## Client TLS Configuration
//!
//! ```rust,ignore
//! use zznet_builder::tls::{load_client_tls, ClientTlsConfig};
//!
//! let tls_config = ClientTlsConfig {
//!     ca_cert_path: "certs/ca.pem".to_string(),
//!     client_cert_path: "certs/client.pem".to_string(),
//!     client_key_path: "certs/client.key".to_string(),
//! };
//!
//! let rustls_config = load_client_tls(&tls_config)?;
//! ```
//!
//! ## Server TLS Configuration
//!
//! ```rust,ignore
//! use zznet_builder::tls::{load_server_tls, ServerTlsConfig};
//!
//! let tls_config = ServerTlsConfig {
//!     ca_cert_paths: vec!["certs/ca.pem".to_string()],
//!     server_cert_path: "certs/server.pem".to_string(),
//!     server_key_path: "certs/server.key".to_string(),
//! };
//!
//! let rustls_config = load_server_tls(&tls_config)?;
//! ```

use crate::error::{Error, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

/// Client TLS configuration specifying paths to certificates and keys.
#[derive(Debug, Clone)]
pub struct ClientTlsConfig {
    /// Path to CA certificate for verifying the server.
    pub ca_cert_path: String,
    /// Path to client certificate (for mTLS).
    pub client_cert_path: String,
    /// Path to client private key.
    pub client_key_path: String,
}

/// Server TLS configuration specifying paths to certificates and keys.
#[derive(Debug, Clone)]
pub struct ServerTlsConfig {
    /// Paths to CA certificates for verifying clients (supports multiple for rotation).
    pub ca_cert_paths: Vec<String>,
    /// Path to server certificate.
    pub server_cert_path: String,
    /// Path to server private key.
    pub server_key_path: String,
}

/// Load and build a rustls ClientConfig for mTLS client connections.
///
/// This function:
/// 1. Loads the CA certificate to verify the server
/// 2. Loads the client certificate for authentication
/// 3. Loads the client private key
/// 4. Builds a ClientConfig with client authentication enabled
///
/// # Arguments
///
/// * `config` - Client TLS configuration with paths to certificates and keys
///
/// # Returns
///
/// Returns an `Arc<ClientConfig>` ready to use with rustls/tokio-rustls.
///
/// # Errors
///
/// Returns an error if:
/// - Any certificate or key file cannot be opened
/// - PEM parsing fails
/// - No valid certificates or keys are found
/// - rustls configuration building fails
pub fn load_client_tls(config: &ClientTlsConfig) -> Result<Arc<ClientConfig>> {
    // 1. Load CA certificate (to verify database server)
    let ca_file = File::open(&config.ca_cert_path).map_err(|e| {
        Error::Tls(format!(
            "Failed to open CA file {}: {}",
            config.ca_cert_path, e
        ))
    })?;
    let mut ca_reader = BufReader::new(ca_file);
    let ca_certs: Vec<_> = certs(&mut ca_reader)
        .map(|r| {
            r.map_err(|e| Error::Tls(format!("Failed to parse CA certs: {}", e)))
                .map(|c| Box::leak(c.as_ref().to_vec().into_boxed_slice()))
        })
        .collect::<Result<_>>()?;

    if ca_certs.is_empty() {
        return Err(Error::Tls("No CA certificates found".into()));
    }

    let mut root_store = RootCertStore::empty();
    for cert in &ca_certs {
        root_store
            .add(CertificateDer::from(&**cert))
            .map_err(|e| Error::Tls(format!("Failed to add CA cert: {}", e)))?;
    }

    // 2. Load client certificate
    let cert_file = File::open(&config.client_cert_path).map_err(|e| {
        Error::Tls(format!(
            "Failed to open client cert {}: {}",
            config.client_cert_path, e
        ))
    })?;
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain: Vec<_> = certs(&mut cert_reader)
        .map(|r| {
            r.map_err(|e| Error::Tls(format!("Failed to parse client cert: {}", e)))
                .map(|c| Box::leak(c.as_ref().to_vec().into_boxed_slice()))
        })
        .collect::<Result<_>>()?;

    if cert_chain.is_empty() {
        return Err(Error::Tls("No client certificate found".into()));
    }

    // 3. Load client private key
    let key_file = File::open(&config.client_key_path).map_err(|e| {
        Error::Tls(format!(
            "Failed to open client key {}: {}",
            config.client_key_path, e
        ))
    })?;
    let mut key_reader = BufReader::new(key_file);
    let keys: Vec<_> = pkcs8_private_keys(&mut key_reader)
        .map(|r| r.map_err(|e| Error::Tls(format!("Failed to parse private key: {}", e))))
        .collect::<Result<_>>()?;

    if keys.is_empty() {
        return Err(Error::Tls("No private key found".into()));
    }

    // Convert to PrivateKeyDer directly from the owned key
    let private_key = PrivateKeyDer::Pkcs8(keys.into_iter().next().unwrap());

    // 4. Build client config - convert cert_chain to owned CertificateDer
    let cert_chain_der: Vec<CertificateDer<'static>> = cert_chain
        .into_iter()
        .map(|c| CertificateDer::from(c.to_vec()))
        .collect();

    let client_config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_client_auth_cert(cert_chain_der, private_key)
        .map_err(|e| Error::Tls(format!("Failed to build TLS config: {}", e)))?;

    Ok(Arc::new(client_config))
}

/// Load and build a rustls ServerConfig for mTLS server connections.
///
/// This function:
/// 1. Loads CA certificates to verify clients (supports multiple for rotation)
/// 2. Loads the server certificate
/// 3. Loads the server private key
/// 4. Builds a ServerConfig with client authentication required
///
/// # Arguments
///
/// * `config` - Server TLS configuration with paths to certificates and keys
///
/// # Returns
///
/// Returns an `Arc<ServerConfig>` ready to use with rustls/tokio-rustls.
///
/// # Errors
///
/// Returns an error if:
/// - Any certificate or key file cannot be opened
/// - PEM parsing fails
/// - No valid certificates or keys are found
/// - rustls configuration building fails
pub fn load_server_tls(config: &ServerTlsConfig) -> Result<Arc<ServerConfig>> {
    // 1. Load CA certificates (to verify clients)
    let mut root_store = RootCertStore::empty();

    for ca_path in &config.ca_cert_paths {
        let ca_file = File::open(ca_path)
            .map_err(|e| Error::Tls(format!("Failed to open CA file: {}", e)))?;
        let mut ca_reader = BufReader::new(ca_file);
        let ca_certs: Vec<_> = certs(&mut ca_reader)
            .map(|r| {
                r.map_err(|e| Error::Tls(format!("Failed to parse CA certs: {}", e)))
                    .map(|c| Box::leak(c.as_ref().to_vec().into_boxed_slice()))
            })
            .collect::<Result<_>>()?;

        if ca_certs.is_empty() {
            tracing::warn!("No CA certificates found in {}", ca_path);
            continue;
        }

        for cert in &ca_certs {
            root_store
                .add(CertificateDer::from(&**cert))
                .map_err(|e| Error::Tls(format!("Failed to add CA cert: {}", e)))?;
        }
    }

    if root_store.is_empty() {
        return Err(Error::Tls("No CA certificates loaded from any path".into()));
    }

    // 2. Load server certificate
    let cert_file = File::open(&config.server_cert_path).map_err(|e| {
        Error::Tls(format!(
            "Failed to open server cert {}: {}",
            config.server_cert_path, e
        ))
    })?;
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain: Vec<_> = certs(&mut cert_reader)
        .map(|r| {
            r.map_err(|e| Error::Tls(format!("Failed to parse server cert: {}", e)))
                .map(|c| Box::leak(c.as_ref().to_vec().into_boxed_slice()))
        })
        .collect::<Result<_>>()?;

    if cert_chain.is_empty() {
        return Err(Error::Tls("No server certificate found".into()));
    }

    // 3. Load server private key
    let key_file = File::open(&config.server_key_path).map_err(|e| {
        Error::Tls(format!(
            "Failed to open server key {}: {}",
            config.server_key_path, e
        ))
    })?;
    let mut key_reader = BufReader::new(key_file);
    let keys: Vec<_> = pkcs8_private_keys(&mut key_reader)
        .map(|r| r.map_err(|e| Error::Tls(format!("Failed to parse private key: {}", e))))
        .collect::<Result<_>>()?;

    if keys.is_empty() {
        return Err(Error::Tls("No private key found".into()));
    }

    let private_key = PrivateKeyDer::Pkcs8(keys.into_iter().next().unwrap());

    // 4. Build server config - convert cert_chain to owned CertificateDer
    let cert_chain_der: Vec<CertificateDer<'static>> = cert_chain
        .into_iter()
        .map(|c| CertificateDer::from(c.to_vec()))
        .collect();

    // Create a client cert verifier from the root store
    let client_cert_verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store))
        .build()
        .map_err(|e| Error::Rustls(format!("Failed to build client cert verifier: {}", e)))?;

    let server_config = ServerConfig::builder()
        .with_client_cert_verifier(client_cert_verifier)
        .with_single_cert(cert_chain_der, private_key)
        .map_err(|e| Error::Tls(format!("Failed to build server TLS config: {}", e)))?;

    Ok(Arc::new(server_config))
}

/// Validate that TLS certificate and key files exist at the specified paths.
///
/// This is a lightweight check that can be done during configuration validation
/// before attempting to load the certificates.
///
/// # Arguments
///
/// * `ca_cert_path` - Optional path to CA certificate
/// * `cert_path` - Path to certificate
/// * `key_path` - Path to private key
///
/// # Returns
///
/// Returns `Ok(())` if all files exist, or an error describing which file is missing.
pub fn validate_tls_paths(
    ca_cert_path: Option<&str>,
    cert_path: &str,
    key_path: &str,
) -> Result<()> {
    if let Some(ca_path) = ca_cert_path
        && !Path::new(ca_path).exists()
    {
        return Err(Error::Config(format!(
            "CA certificate not found: {}",
            ca_path
        )));
    }

    if !Path::new(cert_path).exists() {
        return Err(Error::Config(format!(
            "Certificate not found: {}",
            cert_path
        )));
    }

    if !Path::new(key_path).exists() {
        return Err(Error::Config(format!(
            "Private key not found: {}",
            key_path
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_dummy_cert() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "-----BEGIN CERTIFICATE-----").unwrap();
        writeln!(file, "dummy cert data").unwrap();
        writeln!(file, "-----END CERTIFICATE-----").unwrap();
        file
    }

    fn create_dummy_key() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "-----BEGIN PRIVATE KEY-----").unwrap();
        writeln!(file, "dummy key data").unwrap();
        writeln!(file, "-----END PRIVATE KEY-----").unwrap();
        file
    }

    #[test]
    #[ignore]
    fn test_load_client_tls_valid_certs() {
        // Initialize Rustls default CryptoProvider
        let _ = rustls::crypto::CryptoProvider::install_default(
            rustls::crypto::ring::default_provider(),
        );

        let ca_cert = create_dummy_cert();
        let client_cert = create_dummy_cert();
        let client_key = create_dummy_key();

        let config = ClientTlsConfig {
            ca_cert_path: ca_cert.path().to_str().unwrap().to_string(),
            client_cert_path: client_cert.path().to_str().unwrap().to_string(),
            client_key_path: client_key.path().to_str().unwrap().to_string(),
        };

        let result = load_client_tls(&config);
        assert!(result.is_ok(), "Should load valid client TLS config");
    }

    #[test]
    #[ignore]
    fn test_load_server_tls_valid_certs() {
        // Initialize Rustls default CryptoProvider
        let _ = rustls::crypto::CryptoProvider::install_default(
            rustls::crypto::ring::default_provider(),
        );

        let ca_cert = create_dummy_cert();
        let server_cert = create_dummy_cert();
        let server_key = create_dummy_key();

        let config = ServerTlsConfig {
            ca_cert_paths: vec![ca_cert.path().to_str().unwrap().to_string()],
            server_cert_path: server_cert.path().to_str().unwrap().to_string(),
            server_key_path: server_key.path().to_str().unwrap().to_string(),
        };

        let result = load_server_tls(&config);
        assert!(result.is_ok(), "Should load valid server TLS config");
    }

    #[test]
    fn test_load_client_tls_missing_ca() {
        let client_cert = create_dummy_cert();
        let client_key = create_dummy_key();
        let config = ClientTlsConfig {
            ca_cert_path: "/nonexistent/ca.pem".to_string(),
            client_cert_path: client_cert.path().to_str().unwrap().to_string(),
            client_key_path: client_key.path().to_str().unwrap().to_string(),
        };

        let result = load_client_tls(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("CA file"));
    }

    #[test]
    #[ignore]
    fn test_validate_tls_paths_all_exist() {
        let ca_cert = create_dummy_cert();
        let cert = create_dummy_cert();
        let key = create_dummy_key();

        let result = validate_tls_paths(
            Some(ca_cert.path().to_str().unwrap()),
            cert.path().to_str().unwrap(),
            key.path().to_str().unwrap(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_tls_paths_missing_cert() {
        let key = create_dummy_key();
        let result = validate_tls_paths(None, "/nonexistent/cert.pem", key.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Certificate"));
    }
}
