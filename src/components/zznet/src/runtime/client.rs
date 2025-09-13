use crate::connection::ConnectionCfg;
use crate::traits::AsyncReadWrite;
use anyhow::Result;
use log;
use rustls::pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// The primary state machine for a client's network connection. It holds the configuration and provides the interface to connect to the server.
pub struct ClientRuntime {
    config: ConnectionCfg,
}

impl ClientRuntime {
    /// Prepares the runtime with the necessary connection parameters.
    pub fn new(config: ConnectionCfg) -> Self {
        Self { config }
    }

    /// The main entry point for initiating a connection. It abstracts the complexity of iterating through potential server addresses and performing the TCP connection, optionally with TLS handshake. The caller can await this method to get a ready-to-use stream.
    pub async fn connect(&self) -> Result<Box<dyn AsyncReadWrite + Send + Unpin>> {
        for addr in &self.config.socketaddr {
            match TcpStream::connect(addr).await {
                Ok(stream) => {
                    if let Some(tls_cfg) = &self.config.tls {
                        match tls_cfg.build_client_config() {
                            Ok(client_config) => {
                                let connector = TlsConnector::from(Arc::new(client_config));
                                let domain = ServerName::try_from(tls_cfg.common_name.clone())
                                    .map_err(|e| anyhow::anyhow!("Invalid server name: {}", e))?;
                                match connector.connect(domain, stream).await {
                                    Ok(tls_stream) => return Ok(Box::new(tls_stream)),
                                    Err(e) => {
                                        log::warn!("TLS handshake failed for {}: {}", addr, e)
                                    }
                                }
                            }
                            Err(e) => log::warn!("Failed to build TLS config for {}: {}", addr, e),
                        }
                    } else {
                        return Ok(Box::new(stream));
                    }
                }
                Err(e) => log::warn!("TCP connection failed for {}: {}", addr, e),
            }
        }
        Err(anyhow::anyhow!("Failed to connect to any server address"))
    }
}
