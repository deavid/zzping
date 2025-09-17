use crate::connection::ServerConfig;
use crate::connection_manager::{Connection, ConnectionEvent};
use crate::traits::AsyncReadWrite;
use anyhow::Result;
use async_stream::stream;
use futures::Stream;
use log;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;

/// Manages server listeners and client connection acceptance.
pub struct ServerRuntime {
    config: ServerConfig,
}

impl ServerRuntime {
    /// Creates a new server runtime ready to accept connections.
    pub fn new(config: ServerConfig) -> Self {
        Self { config }
    }

    /// Provides a stream of accepted client connections with TLS support.
    pub async fn run(
        self,
    ) -> Result<impl Stream<Item = Result<(Connection, mpsc::Receiver<ConnectionEvent>)>>> {
        let addrs = self.config.socketaddr.to_vec();
        let config = self.config;
        let (tx, rx) = mpsc::channel(32);
        let acceptor_opt = Self::build_tls_acceptor(&config)?;

        let mut handles = vec![];
        for addr in addrs {
            match TcpListener::bind(&addr).await {
                Ok(listener) => {
                    let tx = tx.clone();
                    let acceptor_opt = acceptor_opt.clone();
                    let handle = tokio::spawn(async move {
                        Self::accept_loop(listener, acceptor_opt, tx).await;
                    });
                    handles.push(handle);
                }
                Err(e) => {
                    log::warn!("Failed to bind to {addr}: {e}");
                }
            }
        }

        if handles.is_empty() {
            return Err(anyhow::anyhow!(
                "Failed to bind to any of the provided addresses"
            ));
        }

        Ok(stream! {
            let mut rx = rx;
            while let Some(result) = rx.recv().await {
                yield result;
            }
        })
    }

    /// Handles optional TLS handshake for incoming connections.
    async fn handle_connection(
        stream: tokio::net::TcpStream,
        acceptor: Option<TlsAcceptor>,
    ) -> Result<Box<dyn AsyncReadWrite + Send + Unpin>> {
        let Some(acceptor) = acceptor else {
            return Ok(Box::new(stream));
        };
        let tls_stream = acceptor.accept(stream).await?;
        Ok(Box::new(tls_stream))
    }

    /// Continuously accepts and processes new client connections.
    async fn accept_loop(
        listener: TcpListener,
        acceptor_opt: Option<TlsAcceptor>,
        tx: mpsc::Sender<Result<(Connection, mpsc::Receiver<ConnectionEvent>)>>,
    ) {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let acceptor_opt = acceptor_opt.clone();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        match Self::handle_connection(stream, acceptor_opt).await {
                            Ok(stream) => {
                                log::info!("New client connected from {addr}");
                                let (event_tx, event_rx) = mpsc::channel(32);
                                let connection = Connection::new(stream, event_tx);
                                let _ = tx.send(Ok((connection, event_rx))).await;
                            }
                            Err(e) => {
                                log::warn!("Connection failed for {addr}: {e}");
                                let _ = tx.send(Err(e)).await;
                            }
                        }
                    });
                }
                Err(e) => {
                    log::warn!("Accept failed: {e}");
                    let _ = tx.send(Err(anyhow::anyhow!("Accept failed: {e}"))).await;
                }
            }
        }
    }

    /// Builds TLS acceptor if configured, enabling secure client connections.
    fn build_tls_acceptor(config: &ServerConfig) -> Result<Option<TlsAcceptor>> {
        if let Some(tls_cfg) = &config.tls {
            let server_config = tls_cfg.build_server_config()?;
            Ok(Some(TlsAcceptor::from(Arc::new(server_config))))
        } else {
            Ok(None)
        }
    }
}
