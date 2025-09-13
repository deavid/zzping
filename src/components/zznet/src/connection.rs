use crate::proto::hello::Role;
use anyhow::Result;
use rustls;
use rustls_native_certs;
use rustls_pemfile;
use std::fs::File;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

pub struct TlsCertAndKey {
    pub pem_path: PathBuf,
    pub key_path: PathBuf,
}

impl TlsCertAndKey {
    pub fn from_role(role: Role) -> Self {
        match role {
            Role::Collector => TlsCertAndKey {
                pem_path: "certs/collector.pem".into(),
                key_path: "certs/collector.key".into(),
            },
            Role::Database => TlsCertAndKey {
                pem_path: "certs/database.pem".into(),
                key_path: "certs/database.key".into(),
            },
            Role::ClientRo => TlsCertAndKey {
                pem_path: "certs/client-ro.pem".into(),
                key_path: "certs/client-ro.key".into(),
            },
            Role::ClientAdmin => TlsCertAndKey {
                pem_path: "certs/client-admin.pem".into(),
                key_path: "certs/client-admin.key".into(),
            },
        }
    }
}

pub struct TlsCfg {
    pub cert: TlsCertAndKey,
    pub ca_cert_path: Option<PathBuf>,
    pub add_native_ca_certs: bool,
    pub common_name: String,
}

impl TlsCfg {
    pub fn from_role(role: Role) -> Self {
        Self {
            cert: TlsCertAndKey::from_role(role),
            ca_cert_path: Some("certs/ca.pem".into()),
            add_native_ca_certs: false,
            common_name: "zzping".into(),
        }
    }

    /// Builds a rustls ClientConfig from this TLS configuration, handling certificate loading, root store setup, and client authentication.
    pub fn build_client_config(&self) -> Result<rustls::ClientConfig> {
        let root_store = self.build_root_store()?;
        let (certs, private_key) = self.load_cert_and_key()?;

        let config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(certs, private_key)?;

        Ok(config)
    }

    /// Builds a rustls ServerConfig from this TLS configuration, handling certificate loading and optional mutual TLS verification.
    pub fn build_server_config(&self) -> Result<rustls::ServerConfig> {
        let (certs, key) = self.load_cert_and_key()?;
        let root_store = self.build_root_store()?;

        let config = rustls::ServerConfig::builder()
            .with_client_cert_verifier(
                rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store)).build()?,
            )
            .with_single_cert(certs, key)?;

        Ok(config)
    }

    /// Loads the certificate and private key pair from the configured paths, returning them ready for use in TLS configuration.
    fn load_cert_and_key(
        &self,
    ) -> Result<(
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    )> {
        let certs = Self::load_certs_from_path(&self.cert.pem_path)?;
        let private_key = Self::load_private_key_from_path(&self.cert.key_path)?;
        Ok((certs, private_key))
    }

    /// Builds a root certificate store from CA certificates and optionally native system certificates.
    fn build_root_store(&self) -> Result<rustls::RootCertStore> {
        let mut root_store = rustls::RootCertStore::empty();

        if let Some(ca_path) = &self.ca_cert_path {
            let ca_certs = Self::load_certs_from_path(ca_path)?;
            for cert in ca_certs {
                root_store.add(cert)?;
            }
        }

        if self.add_native_ca_certs {
            let native_certs =
                rustls_native_certs::load_native_certs().expect("could not load native certs");
            for cert in native_certs {
                root_store.add(cert)?;
            }
        }

        Ok(root_store)
    }

    fn load_certs_from_path(
        path: &std::path::Path,
    ) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
        Ok(certs)
    }

    fn load_private_key_from_path(
        path: &std::path::Path,
    ) -> Result<rustls::pki_types::PrivateKeyDer<'static>> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let key = rustls_pemfile::private_key(&mut reader)?
            .ok_or_else(|| anyhow::anyhow!("No private key found"))?;
        Ok(key)
    }
}

pub struct ClientConfig {
    pub socketaddr: Vec<SocketAddr>,
    pub tls: Option<TlsCfg>,
    pub role: Role,
    pub reconnect_delay: std::time::Duration,
}

pub struct ServerConfig {
    pub socketaddr: Vec<SocketAddr>,
    pub tls: Option<TlsCfg>,
    pub role: Role,
}
