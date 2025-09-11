use crate::client_holder::ClientHolder;
use crate::database_client::{DatabaseClient, DatabaseClientTrait};
use log::{info, warn};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Notify, mpsc};

/// A task that relentlessly and resiliently provides healthy database connections.
pub struct ConnectionManager {
    /// The address of the database gRPC server.
    addr: String,
    /// The authentication token for this collector.
    auth_token: String,
    /// The channel to send new `DatabaseClient` handles to.
    client_tx: mpsc::Sender<Arc<dyn DatabaseClientTrait>>,
    client_holder: Option<ClientHolder>,
    /// A channel used by the `CollectorService` to signal that we need to
    /// create a new connection because the previous session has ended.
    reconnect_notify: Arc<Notify>,
}

impl ConnectionManager {
    /// Creates a new `ConnectionManager`.
    pub fn new(
        addr: String,
        auth_token: String,
        client_tx: mpsc::Sender<Arc<dyn DatabaseClientTrait>>,
        reconnect_notify: Arc<Notify>,
        client_holder: Option<ClientHolder>,
    ) -> Self {
        Self {
            addr,
            auth_token,
            client_tx,
            reconnect_notify,
            client_holder,
        }
    }

    /// Runs the `ConnectionManager`'s infinite connect/retry loop.
    pub async fn run(self) {
        info!("ConnectionManager started.");
        loop {
            info!("Attempting to connect to database at {}...", self.addr);
            match DatabaseClient::connect(self.addr.clone(), self.auth_token.clone()).await {
                Ok(client) => {
                    info!("Successfully connected to database.");
                    // Update optional ClientHolder for consumers that prefer it
                    if let Some(holder) = &self.client_holder {
                        holder.set(client.clone()).await;
                    }
                    if self.client_tx.send(client).await.is_err() {
                        // The receiver was dropped, which means the CollectorService has shut down.
                        // We can exit the loop.
                        info!("Client channel closed. ConnectionManager shutting down.");
                        break;
                    }
                    // Wait for the CollectorService to signal that the session has ended
                    // and we need to create a new connection.
                    self.reconnect_notify.notified().await;
                    info!("Session ended. Reconnecting...");
                }
                Err(e) => {
                    warn!("Failed to connect to database: {e}. Retrying in 2 seconds...");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
}
