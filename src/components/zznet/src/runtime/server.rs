use crate::connection::ServerConfig;
use crate::connection_manager::Connection;
use crate::traits::AsyncReadWrite;
use anyhow::Result;
use async_stream::stream;
use futures::Stream;
use log;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;

/// The primary manager for the server's network listener. It holds the configuration and is responsible for the lifecycle of accepting new clients.
pub struct ServerRuntime {
    config: ServerConfig,
}

impl ServerRuntime {
    /// Prepares the runtime with the necessary connection parameters.
    pub fn new(config: ServerConfig) -> Self {
        Self { config }
    }

    /// Returns a stream that yields a new `Connection` object for each successfully accepted client.
    pub async fn run(self) -> Result<impl Stream<Item = Result<Connection>>> {
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
                    log::warn!("Failed to bind to {}: {}", addr, e);
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

    /// Handles the TLS handshake for a single incoming connection, if TLS is configured, and returns a ready-to-use stream.
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

    /// Runs the accept loop for a single listener, sending accepted connections to the channel.
    async fn accept_loop(
        listener: TcpListener,
        acceptor_opt: Option<TlsAcceptor>,
        tx: mpsc::Sender<Result<Connection>>,
    ) {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let acceptor_opt = acceptor_opt.clone();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        match Self::handle_connection(stream, acceptor_opt).await {
                            Ok(stream) => {
                                log::info!("New client connected from {}", addr);
                                let _ = tx.send(Ok(Connection::new(stream))).await;
                            }
                            Err(e) => {
                                log::warn!("Connection failed for {}: {}", addr, e);
                                let _ = tx.send(Err(e)).await;
                            }
                        }
                    });
                }
                Err(e) => {
                    log::warn!("Accept failed: {}", e);
                    let _ = tx.send(Err(anyhow::anyhow!("Accept failed: {}", e))).await;
                }
            }
        }
    }

    /// Builds the optional TLS acceptor from the configuration.
    fn build_tls_acceptor(config: &ServerConfig) -> Result<Option<TlsAcceptor>> {
        if let Some(tls_cfg) = &config.tls {
            let server_config = tls_cfg.build_server_config()?;
            Ok(Some(TlsAcceptor::from(Arc::new(server_config))))
        } else {
            Ok(None)
        }
    }
}
