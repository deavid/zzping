use crate::connection::ConnectionCfg;
use crate::traits::AsyncReadWrite;
use anyhow::Result;
use futures::future::join_all;
use log;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// The primary manager for the server's network listener. It holds the configuration and is responsible for the lifecycle of accepting new clients.
pub struct ServerRuntime {
    config: ConnectionCfg,
}

impl ServerRuntime {
    /// Prepares the runtime with the necessary connection parameters.
    pub fn new(config: ConnectionCfg) -> Self {
        Self { config }
    }

    /// Starts the server and begins accepting client connections. This method takes ownership of the ServerRuntime and runs indefinitely, handling incoming connections and performing optional TLS handshakes. It does not return until the server is shut down, ensuring proper lifecycle management and preventing zombie tasks.
    pub async fn run(self) -> Result<()> {
        let acceptor_opt = self.build_tls_acceptor()?;
        let mut handles = vec![];

        for addr in &self.config.socketaddr {
            let listener = TcpListener::bind(addr).await?;
            let acceptor_opt = acceptor_opt.clone();
            let handle = tokio::spawn(Self::accept_loop(listener, acceptor_opt));
            handles.push(handle);
        }

        // Wait for all listener tasks to complete (which they never do, keeping the server running)
        join_all(handles).await;
        Ok(())
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

    /// Runs the accept loop for a single listener, spawning tasks for each incoming connection.
    async fn accept_loop(listener: TcpListener, acceptor_opt: Option<TlsAcceptor>) {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let acceptor_opt = acceptor_opt.clone();
                    tokio::spawn(async move {
                        match Self::handle_connection(stream, acceptor_opt).await {
                            Ok(stream) => {
                                log::info!("New client connected from {}", addr);
                                // The stream is assigned to a variable that is not used to avoid a warning.
                                // It will be dropped at the end of this block, and the connection will be closed.
                                // This is the correct behavior for now.
                                let _ = stream;
                            }
                            Err(e) => log::warn!("Connection failed for {}: {}", addr, e),
                        }
                    });
                }
                Err(e) => log::warn!("Accept failed: {}", e),
            }
        }
    }

    /// Builds the optional TLS acceptor from the configuration.
    fn build_tls_acceptor(&self) -> Result<Option<TlsAcceptor>> {
        if let Some(tls_cfg) = &self.config.tls {
            let server_config = tls_cfg.build_server_config()?;
            Ok(Some(TlsAcceptor::from(Arc::new(server_config))))
        } else {
            Ok(None)
        }
    }
}
