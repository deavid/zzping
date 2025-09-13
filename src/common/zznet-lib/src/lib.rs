use anyhow::Result;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use zznet::connection::{ClientConfig, ServerConfig};
use zznet::connection_manager::{Channel, Connection, ConnectionEvent};
use zznet::runtime::client::ClientRuntime;
use zznet::runtime::server::ServerRuntime;

/// High-level facade for managing network connections and channels.
/// Provides a simple API for clients and servers to request and listen for channels.
pub struct ZzNet {
    // For client: the current active connection, shared across tasks
    client_connection: Arc<Mutex<Option<Connection>>>,
    // For server: map of channel name to listener senders
    server_listeners: Arc<Mutex<HashMap<String, mpsc::Sender<(u64, Channel)>>>>,
    // Next client ID for server
    next_client_id: Arc<Mutex<u64>>,
}

/// Configuration enum for initializing ZzNet as a client or server.
pub enum ZzNetConfig {
    Client(ClientConfig),
    Server(ServerConfig),
}

impl ZzNet {
    /// Creates a new ZzNet instance based on the provided configuration.
    /// For clients, it starts the connection runtime.
    /// For servers, it starts the server runtime and manages incoming connections.
    pub fn new(config: ZzNetConfig) -> Self {
        match config {
            ZzNetConfig::Client(client_config) => {
                let client_runtime = ClientRuntime::new(client_config);
                let client_connection = Arc::new(Mutex::new(None));
                let client_connection_clone = Arc::clone(&client_connection);
                tokio::spawn(async move {
                    let mut connection_stream = Box::pin(client_runtime.connections());
                    while let Some(Ok(connection)) = connection_stream.next().await {
                        *client_connection_clone.lock().unwrap() = Some(connection);
                    }
                });
                Self {
                    client_connection,
                    server_listeners: Arc::new(Mutex::new(HashMap::new())),
                    next_client_id: Arc::new(Mutex::new(0)),
                }
            }
            ZzNetConfig::Server(server_config) => {
                let server_listeners: Arc<Mutex<HashMap<String, mpsc::Sender<(u64, Channel)>>>> =
                    Arc::new(Mutex::new(HashMap::new()));
                let next_client_id = Arc::new(Mutex::new(0));
                let server_listeners_clone = Arc::clone(&server_listeners);
                let next_client_id_clone = Arc::clone(&next_client_id);
                tokio::spawn(async move {
                    let server_runtime = ServerRuntime::new(server_config);
                    let mut connection_stream = match server_runtime.run().await {
                        Ok(stream) => Box::pin(stream),
                        Err(e) => {
                            log::error!("Server runtime failed to start: {}", e);
                            return;
                        }
                    };

                    while let Some(Ok((connection, mut event_rx))) = connection_stream.next().await
                    {
                        let command_tx = connection.command_sender(); // Get the command sender here
                        let server_listeners = Arc::clone(&server_listeners_clone);
                        let client_id = {
                            let mut id = next_client_id_clone.lock().unwrap();
                            *id += 1;
                            *id
                        };

                        // The connection object is now dropped, we only use its command_tx
                        tokio::spawn(async move {
                            while let Some(event) = event_rx.recv().await {
                                let ConnectionEvent::ChannelOpened { name, id, receiver } = event;
                                // Construct the channel correctly using the cloned command_tx
                                let channel = Channel {
                                    id,
                                    command_tx: command_tx.clone(),
                                    rx: receiver,
                                };

                                let listener_tx = {
                                    let listeners = server_listeners.lock().unwrap();
                                    listeners.get(&name).cloned()
                                };

                                if let Some(listener_tx) = listener_tx {
                                    if listener_tx.send((client_id, channel)).await.is_err() {
                                        log::warn!(
                                            "A listener for channel '{}' was dropped.",
                                            name
                                        );
                                    }
                                }
                            }
                        });
                    }
                });
                Self {
                    client_connection: Arc::new(Mutex::new(None)),
                    server_listeners,
                    next_client_id,
                }
            }
        }
    }

    // Server-side API
    /// Listens for channels with the specified name.
    /// Returns a receiver that yields new channels as they are opened by clients.
    pub async fn listen_for_channel(&self, name: &str) -> Result<mpsc::Receiver<(u64, Channel)>> {
        let (tx, rx) = mpsc::channel(32);
        self.server_listeners
            .lock()
            .unwrap()
            .insert(name.to_string(), tx);
        Ok(rx)
    }

    // Client-side API
    /// Requests a new channel with the given name from the server.
    /// Returns the channel once opened.
    pub async fn request_channel(&self, name: String) -> Result<Channel> {
        if let Some(conn) = self.client_connection.lock().unwrap().as_ref() {
            conn.request_channel(name).await
        } else {
            Err(anyhow::anyhow!("No active connection"))
        }
    }
}
