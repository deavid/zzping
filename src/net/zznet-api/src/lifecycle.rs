//! Lifecycle management for network connections.
//!
//! This module provides generic functions for maintaining and serving connections
//! across different transport implementations. The logic handles reconnection policies,
//! backoff strategies, and connection lifecycle management.

use crate::messages::AcceptTransport;
use crate::transport::{TransportClient, TransportServer};
use actix::Recipient;
use std::time::Duration;
use tracing::{error, info};

/// Configuration for client reconnection behavior.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Delay between reconnection attempts.
    pub retry_delay: Duration,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            retry_delay: Duration::from_secs(5),
        }
    }
}

/// Maintain a persistent connection to a server, automatically reconnecting on failure.
///
/// This function spawns a background task that continuously attempts to connect to a
/// configured server using the provided TransportClient. When a connection is established,
/// it sends an `AcceptTransport` message to the recipient and waits for the connection to
/// die before retrying with the configured delay.
///
/// # Arguments
/// * `client` - The transport client implementation to use for connections
/// * `recipient` - The actor that will receive `AcceptTransport` messages for new connections
/// * `config` - Configuration for reconnection behavior (retry delay, etc.)
///
/// # Example
/// ```ignore
/// let client = TcpTransportClient::new("127.0.0.1:9000".to_string(), None)?;
/// let config = ReconnectConfig {
///     retry_delay: Duration::from_secs(5),
/// };
/// maintain_connection(client, recipient, config);
/// ```
pub fn maintain_connection<C: TransportClient + 'static>(
    client: C,
    recipient: Recipient<AcceptTransport>,
    config: ReconnectConfig,
) {
    tokio::spawn(async move {
        loop {
            match client.connect().await {
                Ok(connection) => {
                    info!("Connection established, sending to recipient");

                    let peer_addr = connection.peer_addr.clone();
                    let peer_identity = connection.peer_identity.clone();
                    let tx = connection.tx;
                    let rx = connection.rx;
                    let watcher = connection.watcher;
                    let msg = AcceptTransport {
                        tx,
                        rx,
                        peer_addr,
                        peer_identity,
                    };
                    if recipient.send(msg).await.is_err() {
                        error!("Recipient closed, stopping maintain loop");
                        break;
                    }

                    // Wait for the connection to die
                    watcher.await;
                    info!("Connection died, will reconnect");
                }
                Err(e) => {
                    error!("Connection failed: {}", e);
                }
            }

            info!("Reconnecting in {:?}", config.retry_delay);
            tokio::time::sleep(config.retry_delay).await;
        }
    });
}

/// Serve incoming connections and send them to a recipient.
///
/// This function spawns a background task that runs the accept loop on the provided
/// TransportServer. When a connection is accepted, it sends an `AcceptTransport` message
/// to the recipient. If the recipient closes, the accept loop terminates.
///
/// # Arguments
/// * `server` - The transport server implementation to use for accepting connections
/// * `recipient` - The actor that will receive `AcceptTransport` messages for accepted connections
///
/// # Example
/// ```ignore
/// let server = TcpTransportServer::new("0.0.0.0:9001", None).await?;
/// serve_connections(server, recipient);
/// ```
pub fn serve_connections<S: TransportServer + 'static>(
    mut server: S,
    recipient: Recipient<AcceptTransport>,
) {
    tokio::spawn(async move {
        loop {
            match server.accept().await {
                Ok(connection) => {
                    info!("Accepted connection from {}", connection.peer_addr);

                    let peer_addr = connection.peer_addr.clone();
                    let peer_identity = connection.peer_identity.clone();
                    let tx = connection.tx;
                    let rx = connection.rx;
                    let msg = AcceptTransport {
                        tx,
                        rx,
                        peer_addr,
                        peer_identity,
                    };
                    if recipient.send(msg).await.is_err() {
                        error!("Recipient closed, stopping accept loop");
                        break;
                    }
                }
                Err(e) => {
                    error!("Accept error: {}", e);
                    // Brief pause before retrying
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });
}
