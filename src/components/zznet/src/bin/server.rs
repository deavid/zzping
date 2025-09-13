// Minimal tokio + tokio-rustls server example for testing TLS listeners.
// Uses workspace dependencies: tokio, rustls, tokio-rustls.
// This example expects server.pem and server.key (PEM) to exist in the current working directory.

use std::sync::Arc;
use std::{fs::File, io::BufReader, net::SocketAddr};

use log::info;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use rmp_serde::decode;
use zznet::proto;
use zznet::proto::frame::read_frame;

fn load_certs(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let certs = rustls_pemfile::certs(&mut reader);
    let mut out = vec![];
    for cert in certs {
        out.push(cert?);
    }
    Ok(out)
}

fn load_private_key(path: &str) -> anyhow::Result<PrivateKeyDer<'static>> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    // Try pkcs8 first, then rsa
    if let Some(Ok(key)) = rustls_pemfile::pkcs8_private_keys(&mut reader).next() {
        return Ok(key.into());
    }

    // Rewind and try rsa
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    if let Some(Ok(key)) = rustls_pemfile::rsa_private_keys(&mut reader).next() {
        return Ok(key.into());
    }
    Err(anyhow::anyhow!("no private key found in {}", path))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    // Simple CLI-like defaults
    let addr: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8443".to_string())
        .parse()?;
    let cert_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "certs/server.pem".to_string());
    let key_path = std::env::args()
        .nth(3)
        .unwrap_or_else(|| "certs/server.key".to_string());

    let certs = load_certs(&cert_path)?;
    let key = load_private_key(&key_path)?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind(addr).await?;
    println!("Listening on {}", listener.local_addr()?);

    loop {
        let (stream, peer) = listener.accept().await?;
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            match acceptor.accept(stream).await {
                Ok(tls_stream) => {
                    println!("TLS connection from {}", peer);
                    // tls_stream.get_ref().1.peer_certificates
                    if let Err(e) = process_client_connection(tls_stream).await {
                        log::error!("Error from client: {}", e);
                    }
                }
                Err(e) => eprintln!("TLS accept error: {}", e),
            }
        });
    }
}

async fn process_client_connection(
    mut stream: impl AsyncReadExt + AsyncWriteExt + std::marker::Unpin,
) -> anyhow::Result<()> {
    let frame = read_frame(&mut stream).await?;
    info!("frame: {:?}", frame);
    match decode::from_slice::<proto::Hello>(&frame) {
        Ok(hello) => {
            info!("Received ClientHello: {:?}", hello);
            // Send a simple response
            let response = b"Hello from server\n";
            let _ = stream.write_all(response).await;
        }
        Err(e) => info!("Deserialization error: {}", e),
    }
    Ok(())
}
