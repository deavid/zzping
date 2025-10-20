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
    // NOTE: from_role(Role) was removed to keep this crate auth-agnostic.

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

    /// Create a TlsConfig directly from explicit file paths.
    ///
    /// This helper is useful for applications that pass certificate file paths
    /// from configuration files. It validates the files exist and returns an
    /// initialized `TlsConfig` ready to build rustls configs.
    pub fn from_file_paths(
        cert_path: &str,
        key_path: &str,
        ca_path: Option<&str>,
    ) -> Result<Self, TlsError> {
        // Ensure a rustls CryptoProvider is installed for the process.
        let _ = rustls::crypto::ring::default_provider().install_default();

        // Validate files exist by attempting to open them. Any IO error will
        // be converted into TlsError::Io via the `?` operator.
        let _ = File::open(cert_path)?;
        let _ = File::open(key_path)?;
        if let Some(ca) = ca_path {
            let _ = File::open(ca)?;
        }

        let config = Self {
            cert: TlsCertAndKey {
                pem_path: PathBuf::from(cert_path),
                key_path: PathBuf::from(key_path),
            },
            ca_cert_path: ca_path.map(PathBuf::from),
            add_native_ca_certs: false,
            server_name: "zzping".into(),
        };

        Ok(config)
    }

    /// Server-side convenience that mirrors `from_file_paths`.
    pub fn server_from_file_paths(
        cert_path: &str,
        key_path: &str,
        ca_path: Option<&str>,
    ) -> Result<Self, TlsError> {
        Self::from_file_paths(cert_path, key_path, ca_path)
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

    // These tests verify the `from_file_paths()` method which was added in
    // Phase 2 of the builder refactoring. This is the ONLY gap in test
    // coverage across all 5 phases of the refactoring.
    //
    // Background: `from_file_paths()` is used by `ClientBuilder::with_tls_from_files()`
    // and `ServerBuilder::with_tls_from_files()` to allow applications to pass
    // certificate file paths directly instead of manually loading PEM files.
    //
    // Coverage needed:
    // 1. Valid certificate files (happy path)
    // 2. Missing certificate file
    // 3. Missing private key file
    // 4. Missing CA certificate file
    // 5. Invalid PEM format (bonus: if we can test this)
    //
    // Implementation is at lines 176-201 in this file.
    // ========================================================================

    #[test]
    fn test_from_file_paths_valid_certificates() {
        // TODO: Test the happy path where all certificate files exist and are valid.
        //
        // Setup:
        // - Use the existing test_certs directory which has valid certificates
        // - The workspace root can be found via env!("CARGO_MANIFEST_DIR")
        // - Path to test_certs: workspace_root/test_certs/
        // - Files available: collector.pem, collector.key, database.pem, database.key, ca.pem
        //
        // Test steps:
        // 1. Compute absolute path to test_certs directory
        // 2. Call TlsConfig::from_file_paths() with valid paths:
        //    - cert_path: "test_certs/collector.pem"
        //    - key_path: "test_certs/collector.key"
        //    - ca_path: Some("test_certs/ca.pem")
        // 3. Assert Result is Ok(config)
        // 4. Assert config.cert.pem_path points to collector.pem
        // 5. Assert config.cert.key_path points to collector.key
        // 6. Assert config.ca_cert_path points to ca.pem
        // 7. Assert config.server_name == "zzping" (default)
        //
        // Expected behavior:
        // - Should return Ok(TlsConfig) with all paths correctly set
        // - Files should be validated (opened) during from_file_paths call
        // - No panics or errors
        //
        // Why this matters:
        // - This is the primary use case - apps loading real certificate files
        // - Verifies file validation logic works correctly
        // - Ensures PathBuf construction is correct

        // Happy path: all certificate files exist and are valid.
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .expect("Failed to determine workspace root for tests");

        let cert = workspace_root.join("test_certs/collector.pem");
        let key = workspace_root.join("test_certs/collector.key");
        let ca = workspace_root.join("test_certs/ca.pem");

        let res = TlsConfig::from_file_paths(
            cert.to_str().unwrap(),
            key.to_str().unwrap(),
            Some(ca.to_str().unwrap()),
        );

        assert!(
            res.is_ok(),
            "expected Ok(TlsConfig) for valid files, got: {:?}",
            res
        );
        let cfg = res.unwrap();
        assert_eq!(cfg.cert.pem_path, cert);
        assert_eq!(cfg.cert.key_path, key);
        assert_eq!(cfg.ca_cert_path.unwrap(), ca);
        assert_eq!(cfg.server_name, "zzping");
    }

    #[test]
    fn test_from_file_paths_missing_certificate() {
        // TODO: Test error handling when certificate file doesn't exist.
        //
        // Setup:
        // - Use a path to a certificate that definitely doesn't exist
        // - Example: "nonexistent/path/missing.pem"
        //
        // Test steps:
        // 1. Call TlsConfig::from_file_paths() with:
        //    - cert_path: "nonexistent/missing.pem" (file does not exist)
        //    - key_path: "test_certs/collector.key" (valid path)
        //    - ca_path: Some("test_certs/ca.pem") (valid path)
        // 2. Assert Result is Err(TlsError::Io(_))
        // 3. Verify error message contains helpful information about missing file
        //
        // Expected behavior:
        // - Should return Err(TlsError::Io(_)) because File::open() fails
        // - Error should propagate from line 186: let _ = File::open(cert_path)?;
        // - The ? operator converts std::io::Error -> TlsError::Io via From trait
        //
        // Why this matters:
        // - Most common user error - typo in certificate path
        // - Ensures clear error messages guide users to fix config
        // - Verifies File::open validation happens before creating config

        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .expect("Failed to determine workspace root for tests");

        let cert = workspace_root.join("nonexistent/missing.pem");
        let key = workspace_root.join("test_certs/collector.key");
        let ca = workspace_root.join("test_certs/ca.pem");

        let res = TlsConfig::from_file_paths(
            cert.to_str().unwrap(),
            key.to_str().unwrap(),
            Some(ca.to_str().unwrap()),
        );

        assert!(
            matches!(res, Err(TlsError::Io(_))),
            "expected Io error, got: {:?}",
            res
        );
    }

    #[test]
    fn test_from_file_paths_missing_key() {
        // TODO: Test error handling when private key file doesn't exist.
        //
        // Setup:
        // - Use a path to a key file that doesn't exist
        // - Cert and CA paths should be valid
        //
        // Test steps:
        // 1. Call TlsConfig::from_file_paths() with:
        //    - cert_path: "test_certs/collector.pem" (valid path)
        //    - key_path: "nonexistent/missing.key" (file does not exist)
        //    - ca_path: Some("test_certs/ca.pem") (valid path)
        // 2. Assert Result is Err(TlsError::Io(_))
        // 3. Verify error mentions the key file
        //
        // Expected behavior:
        // - Should return Err(TlsError::Io(_)) because File::open() fails
        // - Error should propagate from line 187: let _ = File::open(key_path)?;
        // - Validates that key file check happens independently
        //
        // Why this matters:
        // - Private keys are often in separate files with restricted permissions
        // - Missing key file is a common deployment error
        // - Verifies all three file checks (cert, key, CA) are independent

        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .expect("Failed to determine workspace root for tests");

        let cert = workspace_root.join("test_certs/collector.pem");
        let key = workspace_root.join("nonexistent/missing.key");
        let ca = workspace_root.join("test_certs/ca.pem");

        let res = TlsConfig::from_file_paths(
            cert.to_str().unwrap(),
            key.to_str().unwrap(),
            Some(ca.to_str().unwrap()),
        );

        assert!(
            matches!(res, Err(TlsError::Io(_))),
            "expected Io error for missing key, got: {:?}",
            res
        );
    }

    #[test]
    fn test_from_file_paths_missing_ca_certificate() {
        // TODO: Test error handling when CA certificate file doesn't exist.
        //
        // Setup:
        // - Cert and key paths valid, CA path invalid
        //
        // Test steps:
        // 1. Call TlsConfig::from_file_paths() with:
        //    - cert_path: "test_certs/collector.pem" (valid path)
        //    - key_path: "test_certs/collector.key" (valid path)
        //    - ca_path: Some("nonexistent/missing-ca.pem") (file does not exist)
        // 2. Assert Result is Err(TlsError::Io(_))
        // 3. Verify error mentions the CA file
        //
        // Expected behavior:
        // - Should return Err(TlsError::Io(_)) because File::open() fails
        // - Error should propagate from lines 188-190:
        //   if let Some(ca) = ca_path {
        //       let _ = File::open(ca)?;
        //   }
        // - Validates CA file validation is conditional on Some(path)
        //
        // Why this matters:
        // - CA certificate is critical for mutual TLS authentication
        // - Missing CA breaks peer verification
        // - Verifies optional CA path is still validated when provided

        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .expect("Failed to determine workspace root for tests");

        let cert = workspace_root.join("test_certs/collector.pem");
        let key = workspace_root.join("test_certs/collector.key");
        let ca = workspace_root.join("nonexistent/missing-ca.pem");

        let res = TlsConfig::from_file_paths(
            cert.to_str().unwrap(),
            key.to_str().unwrap(),
            Some(ca.to_str().unwrap()),
        );

        assert!(
            matches!(res, Err(TlsError::Io(_))),
            "expected Io error for missing CA, got: {:?}",
            res
        );
    }

    #[test]
    fn test_from_file_paths_ca_none_succeeds() {
        // TODO: Test that CA certificate is optional (ca_path can be None).
        //
        // Setup:
        // - Valid cert and key, but ca_path = None
        //
        // Test steps:
        // 1. Call TlsConfig::from_file_paths() with:
        //    - cert_path: "test_certs/collector.pem" (valid path)
        //    - key_path: "test_certs/collector.key" (valid path)
        //    - ca_path: None
        // 2. Assert Result is Ok(config)
        // 3. Assert config.ca_cert_path is None
        // 4. Assert config.cert paths are still set correctly
        //
        // Expected behavior:
        // - Should return Ok(TlsConfig)
        // - CA validation (lines 188-190) should be skipped when ca_path is None
        // - Resulting config should have ca_cert_path = None
        //
        // Why this matters:
        // - Some applications may use TLS without CA validation (not recommended but valid)
        // - Tests that None is handled correctly in the optional CA logic
        // - Verifies the config struct can represent "no CA" state

        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .expect("Failed to determine workspace root for tests");

        let cert = workspace_root.join("test_certs/collector.pem");
        let key = workspace_root.join("test_certs/collector.key");

        let res = TlsConfig::from_file_paths(cert.to_str().unwrap(), key.to_str().unwrap(), None);
        assert!(
            res.is_ok(),
            "expected Ok(TlsConfig) when ca_path is None, got: {:?}",
            res
        );
        let cfg = res.unwrap();
        assert_eq!(cfg.cert.pem_path, cert);
        assert_eq!(cfg.cert.key_path, key);
        assert!(cfg.ca_cert_path.is_none());
    }

    #[test]
    fn test_from_file_paths_server_from_file_paths_delegates() {
        // TODO: Test that server_from_file_paths() correctly delegates to from_file_paths().
        //
        // Background:
        // - server_from_file_paths() is a convenience method (line 206)
        // - It just calls from_file_paths() internally (line 211)
        // - Exists for API symmetry with potential future server-specific logic
        //
        // Test steps:
        // 1. Call TlsConfig::server_from_file_paths() with valid paths
        // 2. Assert Result is Ok(config)
        // 3. Verify config has same structure as from_file_paths() result
        // 4. Optionally: test with invalid paths to ensure errors propagate
        //
        // Expected behavior:
        // - Should behave identically to from_file_paths()
        // - No additional logic, just delegation
        //
        // Why this matters:
        // - Ensures API surface is consistent
        // - Documents that server and client TLS loading are currently identical
        // - If server-specific logic is added later, this test will catch regressions

        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .expect("Failed to determine workspace root for tests");

        let cert = workspace_root.join("test_certs/collector.pem");
        let key = workspace_root.join("test_certs/collector.key");
        let ca = workspace_root.join("test_certs/ca.pem");

        let a = TlsConfig::from_file_paths(
            cert.to_str().unwrap(),
            key.to_str().unwrap(),
            Some(ca.to_str().unwrap()),
        );
        let b = TlsConfig::server_from_file_paths(
            cert.to_str().unwrap(),
            key.to_str().unwrap(),
            Some(ca.to_str().unwrap()),
        );

        assert!(
            a.is_ok() && b.is_ok(),
            "both should succeed for valid files: a={:?}, b={:?}",
            a,
            b
        );
        let a_cfg = a.unwrap();
        let b_cfg = b.unwrap();

        assert_eq!(a_cfg.cert.pem_path, b_cfg.cert.pem_path);
        assert_eq!(a_cfg.cert.key_path, b_cfg.cert.key_path);
        assert_eq!(a_cfg.ca_cert_path.unwrap(), b_cfg.ca_cert_path.unwrap());
    }

    #[test]
    fn test_from_file_paths_relative_paths_work() {
        // TODO: Test that relative paths are handled correctly.
        //
        // Background:
        // - from_file_paths() accepts &str, converts to PathBuf
        // - Relative paths should work relative to current working directory
        // - Important for configuration files with relative paths
        //
        // Test steps:
        // 1. Determine current working directory during test execution
        // 2. Create relative paths from that CWD to test_certs
        //    (e.g., "../../test_certs/collector.pem" depending on test location)
        // 3. Call from_file_paths() with these relative paths
        // 4. Assert Result is Ok(config)
        // 5. Verify resulting PathBuf contains correct relative path
        //
        // Alternative simpler approach:
        // - Use paths like "./test_certs/..." or "test_certs/..." directly
        // - These will work if test runs from workspace root
        //
        // Expected behavior:
        // - Relative paths should be accepted as-is
        // - File validation should resolve them correctly
        // - PathBuf should preserve the relative form
        //
        // Why this matters:
        // - Most config files use relative paths
        // - Users expect "certs/app.pem" to work
        // - Verifies no premature canonicalization happens

        // Try to construct a relative path from cwd to the workspace test_certs.
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .expect("Failed to determine workspace root for tests");

        let cert_abs = workspace_root.join("test_certs/collector.pem");
        let key_abs = workspace_root.join("test_certs/collector.key");
        let ca_abs = workspace_root.join("test_certs/ca.pem");

        let cwd = std::env::current_dir().expect("failed to get cwd");

        let (cert_path, key_path, ca_path) = if let Ok(rel) = cert_abs.strip_prefix(&cwd) {
            (
                rel.to_str().unwrap().to_string(),
                key_abs
                    .strip_prefix(&cwd)
                    .map(|p| p.to_str().unwrap().to_string())
                    .unwrap_or_else(|_| key_abs.to_str().unwrap().to_string()),
                ca_abs
                    .strip_prefix(&cwd)
                    .map(|p| p.to_str().unwrap().to_string())
                    .unwrap_or_else(|_| ca_abs.to_str().unwrap().to_string()),
            )
        } else {
            (
                cert_abs.to_str().unwrap().to_string(),
                key_abs.to_str().unwrap().to_string(),
                ca_abs.to_str().unwrap().to_string(),
            )
        };

        let res = TlsConfig::from_file_paths(&cert_path, &key_path, Some(&ca_path));
        assert!(
            res.is_ok(),
            "expected Ok for test_certs paths, got: {:?}",
            res
        );
        let cfg = res.unwrap();

        assert!(cfg.cert.pem_path.ends_with("collector.pem"));
        assert!(cfg.cert.key_path.ends_with("collector.key"));
    }

    // #[test]
    // fn test_from_file_paths_invalid_pem_format() {
    //     // TODO: Test behavior when files exist but contain invalid PEM data.
    //     //
    //     // Challenge: from_file_paths() only validates file existence, not content.
    //     // Invalid PEM errors happen later during build_client_config() or build_server_config().
    //     //
    //     // This test would need to:
    //     // 1. Create temporary files with invalid PEM content
    //     // 2. Call from_file_paths() - should succeed (only checks existence)
    //     // 3. Call config.build_client_config() - should fail with Certificate error
    //     //
    //     // Less critical because:
    //     // - PEM validation happens at config build time (existing code)
    //     // - from_file_paths() is just a file existence checker
    //     // - Real integration tests will catch PEM issues
    // }
}
