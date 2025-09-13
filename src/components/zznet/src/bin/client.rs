// Minimal rustls client example that connects to a TLS server, sends a message and prints the reply.
// Usage: cargo run --example rustls_client -- <addr> <server_name>

use rmp_serde::encode;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use std::fs;
use std::io::Cursor;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use zznet::proto::frame::write_frame;

use zznet::proto;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8443".to_string());
    let server_name_string = "zzping";

    // Build default root store from system roots (if available) or empty
    let mut root_store = RootCertStore::empty();
    let native_certs =
        rustls_native_certs::load_native_certs().expect("could not load native certs");
    for cert in native_certs {
        root_store.add(cert)?;
    }

    // Load custom CA certificate
    let ca_cert_data = fs::read("certs/ca.pem").expect("could not read ca.pem");
    let mut reader = Cursor::new(&ca_cert_data);
    for cert in rustls_pemfile::certs(&mut reader) {
        root_store.add(cert?)?;
    }

    // As a fallback, do not verify (for local testing). In production you must verify.
    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(config));

    let mut addrs = addr.to_socket_addrs()?;
    let socket_addr = addrs
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid addr"))?;

    let stream = TcpStream::connect(socket_addr).await?;
    let server_name = ServerName::try_from(server_name_string)?.to_owned();
    let mut tls = connector.connect(server_name, stream).await?;

    let hello = proto::Hello {
        role: proto::Role::Collector,
    };
    // Serialize the hello struct to MessagePack bytes
    let serialized_hello =
        encode::to_vec(&hello).map_err(|e| anyhow::anyhow!("Failed to serialize hello: {}", e))?;

    // Send the serialized data over TLS
    write_frame(&mut tls, &serialized_hello).await?;

    let mut buf = Vec::new();
    tls.read_to_end(&mut buf).await.ok();
    println!("response: {}", String::from_utf8_lossy(&buf));

    Ok(())
}
