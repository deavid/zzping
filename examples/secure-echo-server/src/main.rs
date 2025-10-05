use std::io::{self, BufReader};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use rustls::{Certificate, PrivateKey, ServerConfig, RootCertStore, AllowAnyAuthenticatedClient};
use rustls_pemfile::{certs, rsa_private_keys};
use zzping_auth::config::AclConfig;
use zzping_auth::acl::AclManager;
use x509_parser::prelude::*;

fn load_certs(path: &str) -> io::Result<Vec<Certificate>> {
    let f = std::fs::File::open(path)?;
    let mut reader = BufReader::new(f);
    let certs = certs(&mut reader).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "failed to parse certs"))?;
    Ok(certs.into_iter().map(Certificate).collect())
}

fn load_private_key(path: &str) -> io::Result<PrivateKey> {
    let f = std::fs::File::open(path)?;
    let mut reader = BufReader::new(f);
    let mut keys = rsa_private_keys(&mut reader).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "failed to parse private key"))?;
    if keys.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "no private keys found"));
    }
    Ok(PrivateKey(keys.remove(0)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Load ACL configuration
    let cfg = AclConfig::from_file("examples/secure-echo-server/acl.toml")?;
    let acl = cfg.into_acl_manager()?;
    let acl = Arc::new(acl);

    // Load server cert and key from test_certs
    let certs = load_certs("test_certs/collector.pem")?;
    let key = load_private_key("test_certs/collector.key")?;

    // Load CA to verify client certs
    let ca_certs = load_certs("test_certs/ca.pem")?;
    let mut root_store = RootCertStore::empty();
    for c in ca_certs.iter() {
        root_store.add(c).map_err(|_| anyhow::anyhow!("failed to add ca cert"))?;
    }

    let verifier = AllowAnyAuthenticatedClient::new(root_store);

    let mut server_config = ServerConfig::builder()
        .with_safe_defaults()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .map_err(|e| anyhow::anyhow!("failed to build server config: {}", e))?;

    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let listener = TcpListener::bind("127.0.0.1:9009").await?;
    log::info!("Secure-echo-server listening on 127.0.0.1:9009 (mTLS)");

    // simple shutdown flag for tests
    let running = Arc::new(AtomicBool::new(true));

    while running.load(Ordering::Relaxed) {
        let (socket, addr) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let acl = acl.clone();

        tokio::spawn(async move {
            match acceptor.accept(socket).await {
                Ok(mut tls_stream) => {
                    // Try to get peer cert
                    let peer_certs = tls_stream.get_ref().1.peer_certificates();
                    if peer_certs.is_none() || peer_certs.as_ref().unwrap().is_empty() {
                        let _ = tls_stream.write_all(b"DENIED: no client cert\n").await;
                        return;
                    }

                    let der = peer_certs.unwrap()[0].as_ref();
                    // Parse DER to extract CN and SAN (first DNS)
                    match X509Certificate::from_der(der) {
                        Ok((_, cert)) => {
                            let common_name = cert
                                .subject()
                                .iter_common_name()
                                .next()
                                .and_then(|cn| cn.as_str().ok())
                                .unwrap_or("")
                                .to_string();

                            let san_username = cert
                                .extensions()
                                .iter()
                                .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
                                .and_then(|ext| x509_parser::extensions::SubjectAlternativeName::from_der(ext.value).ok())
                                .and_then(|(_, san)| san.general_names.iter().find_map(|name| match name { x509_parser::extensions::GeneralName::DNSName(d) => Some(d.to_string()), _ => None }))
                                .unwrap_or_default();

                            let identity = if san_username.is_empty() {
                                common_name.clone()
                            } else {
                                format!("{}@{}", san_username, common_name)
                            };

                            log::info!("Incoming TLS connection from {} identity={}", addr, identity);

                            if acl.is_authorized(&identity) {
                                let _ = tls_stream.write_all(b"WELCOME\n").await;
                                // Echo loop
                                let mut buf = [0u8; 1024];
                                loop {
                                    match tls_stream.read(&mut buf).await {
                                        Ok(0) | Err(_) => break,
                                        Ok(n) => {
                                            if tls_stream.write_all(&buf[..n]).await.is_err() {
                                                break;
                                            }
                                        }
                                    }
                                }
                            } else {
                                let _ = tls_stream.write_all(b"DENIED\n").await;
                            }
                        }
                        Err(_) => {
                            let _ = tls_stream.write_all(b"DENIED: bad cert\n").await;
                        }
                    }
                }
                Err(e) => {
                    log::warn!("TLS accept error: {}", e);
                }
            }
        });
    }

    Ok(())
}
