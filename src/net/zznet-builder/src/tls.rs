//! TLS helpers for client and server certificate handling.

/// Construct a `zznet_transport_tcp::config::TlsConfig` from file paths.
pub fn to_transport_tls_config(
    cert_path: &str,
    key_path: &str,
    ca_cert_path: Option<&str>,
    server_name: String,
) -> zznet_transport_tcp::config::TlsConfig {
    use std::path::PathBuf;
    use zznet_transport_tcp::config::{TlsCertAndKey, TlsConfig as TransportTlsConfig};

    let cert = TlsCertAndKey {
        pem_path: PathBuf::from(cert_path),
        key_path: PathBuf::from(key_path),
    };
    let ca = ca_cert_path.map(PathBuf::from);

    TransportTlsConfig {
        cert,
        ca_cert_path: ca,
        add_native_ca_certs: false,
        server_name,
    }
}
