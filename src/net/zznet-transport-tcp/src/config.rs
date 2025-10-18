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
use zznet_api::types::Role;

/// TLS configuration errors.
#[derive(Error, Debug)]
pub enum TlsError {
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
    /// Derives certificate paths from role, enforcing zzping's security conventions.
    ///
    /// # Arguments
    /// * `role` - The component role (Collector, Database, ClientRo, ClientAdmin)
    /// * `certs_dir` - Optional directory path, defaults to "certs"
    ///
    /// # Example
    /// ```
    /// use zznet_transport_tcp::TlsCertAndKey;
    /// use zznet_api::types::Role;
    ///
    /// let cert = TlsCertAndKey::from_role(Role::Collector, None);
    /// assert_eq!(cert.pem_path.to_str().unwrap(), "certs/collector.pem");
    /// ```
    pub fn from_role(role: Role, certs_dir: Option<&str>) -> Self {
        let dir = certs_dir.unwrap_or("certs");
        match role {
            Role::Collector => TlsCertAndKey {
                pem_path: format!("{dir}/collector.pem").into(),
                key_path: format!("{dir}/collector.key").into(),
            },
            Role::Database => TlsCertAndKey {
                pem_path: format!("{dir}/database.pem").into(),
                key_path: format!("{dir}/database.key").into(),
            },
            Role::ClientRo => TlsCertAndKey {
                pem_path: format!("{dir}/client-ro.pem").into(),
                key_path: format!("{dir}/client-ro.key").into(),
            },
            Role::ClientAdmin => TlsCertAndKey {
                pem_path: format!("{dir}/client-admin.pem").into(),
                key_path: format!("{dir}/client-admin.key").into(),
            },
        }
    }

    /// Derives certificate paths from a role name string.
    ///
    /// This is the generic, reusable API that doesn't depend on the concrete `Role` enum.
    /// Applications can use any role name they want, as long as matching certificates exist.
    ///
    /// # Arguments
    /// * `role_name` - The role identifier as a string (e.g., "collector", "database", "my-custom-role")
    /// * `certs_dir` - Optional directory path, defaults to "certs"
    ///
    /// # Example
    /// ```
    /// use zznet_transport_tcp::TlsCertAndKey;
    ///
    /// // Works with any role name - no dependency on zzping's Role enum
    /// let cert = TlsCertAndKey::from_role_name("collector", None);
    /// assert_eq!(cert.pem_path.to_str().unwrap(), "certs/collector.pem");
    ///
    /// // New applications can use their own role names
    /// let cert = TlsCertAndKey::from_role_name("my-custom-service", Some("/etc/certs"));
    /// assert_eq!(cert.pem_path.to_str().unwrap(), "/etc/certs/my-custom-service.pem");
    /// ```
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
    /// Creates TLS configuration from a role name string (generic, reusable API).
    ///
    /// This is the preferred way to create TLS configuration. It doesn't depend on
    /// the concrete `Role` enum, allowing the transport layer to be used by any application.
    ///
    /// # Arguments
    /// * `role_name` - The role identifier as a string (e.g., "collector", "database")
    /// * `certs_dir` - Optional directory path, defaults to "certs"
    ///
    /// # Returns
    /// A TlsConfig ready to build client or server configurations
    ///
    /// # Example
    /// ```no_run
    /// use zznet_transport_tcp::TlsConfig;
    ///
    /// // Generic API - works with any application
    /// let config = TlsConfig::from_role_name("collector", None)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_role_name(role_name: &str, certs_dir: Option<&str>) -> Result<Self, TlsError> {
        let dir = certs_dir.unwrap_or("certs");
        // Ensure a rustls CryptoProvider is installed for the process.
        let _ = rustls::crypto::ring::default_provider().install_default();

        // Build initial config
        let mut config = Self {
            cert: TlsCertAndKey::from_role_name(role_name, certs_dir),
            ca_cert_path: Some(format!("{}/ca.pem", dir).into()),
            add_native_ca_certs: false,
            server_name: "zzping".into(),
        };

        // If any of the expected files don't exist, try resolving them relative
        // to the workspace root (useful for integration tests that compute an
        // absolute path to the workspace test_certs directory).
        // This keeps existing behavior but makes tests more robust when paths
        // are passed in different forms.
        let pem_exists = config.cert.pem_path.exists();
        let key_exists = config.cert.key_path.exists();
        let ca_exists = config
            .ca_cert_path
            .as_ref()
            .map(|p| p.exists())
            .unwrap_or(false);

        if !(pem_exists && key_exists && ca_exists)
            && let Some(provided_dir) = certs_dir
        {
            // Try resolving relative to the workspace root inferred from
            // this crate's manifest dir. This mirrors how tests compute
            // workspace root.
            if let Some(workspace_root) = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
            {
                let alt_dir = workspace_root.join(provided_dir);
                let alt_pem = alt_dir.join(format!("{role_name}.pem"));
                let alt_key = alt_dir.join(format!("{role_name}.key"));
                let alt_ca = alt_dir.join("ca.pem");

                if alt_pem.exists() && alt_key.exists() && alt_ca.exists() {
                    config.cert.pem_path = alt_pem;
                    config.cert.key_path = alt_key;
                    config.ca_cert_path = Some(alt_ca);
                }
            }
        }

        Ok(config)
    }

    /// Pre-configures TLS for a role, using zzping's security conventions.
    ///
    /// # Deprecated
    /// Use `from_role_name()` instead. This method is maintained for backward compatibility
    /// but couples this library to zzping's specific role definitions.
    ///
    /// # Arguments
    /// * `role` - The component role
    /// * `certs_dir` - Optional directory path, defaults to "certs"
    ///
    /// # Returns
    /// A TlsConfig ready to build client or server configurations
    #[deprecated(
        since = "0.3.0",
        note = "Use `from_role_name()` instead to decouple from zzping's Role enum"
    )]
    pub fn from_role(role: Role, certs_dir: Option<&str>) -> Result<Self, TlsError> {
        Self::from_role_name(role.cert_name(), certs_dir)
    }

    /// Builds a rustls ClientConfig with mutual TLS authentication.
    ///
    /// The client will:
    /// - Verify server certificates against CA
    /// - Present its own certificate for mutual TLS
    /// - Use configured server name for SNI
    pub fn build_client_config(&self) -> Result<rustls::ClientConfig, TlsError> {
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
    pub fn build_server_config(&self) -> Result<rustls::ServerConfig, TlsError> {
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
    fn test_tls_cert_and_key_from_role() {
        let cert = TlsCertAndKey::from_role(Role::Collector, None);
        assert_eq!(cert.pem_path.to_str().unwrap(), "certs/collector.pem");
        assert_eq!(cert.key_path.to_str().unwrap(), "certs/collector.key");

        let cert = TlsCertAndKey::from_role(Role::Database, Some("test_certs"));
        assert_eq!(cert.pem_path.to_str().unwrap(), "test_certs/database.pem");
        assert_eq!(cert.key_path.to_str().unwrap(), "test_certs/database.key");
    }

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

    #[test]
    #[allow(deprecated)]
    fn test_tls_config_from_role() {
        // Test backward compatibility - from_role() is deprecated but should still work
        let config = TlsConfig::from_role(Role::ClientRo, None).unwrap();
        assert_eq!(
            config.cert.pem_path.to_str().unwrap(),
            "certs/client-ro.pem"
        );
        assert_eq!(
            config.ca_cert_path.unwrap().to_str().unwrap(),
            "certs/ca.pem"
        );
        assert_eq!(config.server_name, "zzping");
    }

    #[test]
    fn test_tls_config_from_role_name() {
        // Test the new generic API
        let config = TlsConfig::from_role_name("client-ro", None).unwrap();
        assert_eq!(
            config.cert.pem_path.to_str().unwrap(),
            "certs/client-ro.pem"
        );
        assert_eq!(
            config.ca_cert_path.unwrap().to_str().unwrap(),
            "certs/ca.pem"
        );
        assert_eq!(config.server_name, "zzping");

        // Generic API works with any role name
        let config = TlsConfig::from_role_name("my-service", Some("test_certs")).unwrap();
        assert_eq!(
            config.cert.pem_path.to_str().unwrap(),
            "test_certs/my-service.pem"
        );
        assert_eq!(
            config.ca_cert_path.unwrap().to_str().unwrap(),
            "test_certs/ca.pem"
        );
    }
}
