// client.rs
use crate::facade::InternalEvent;
use anyhow::{anyhow, Result};
use futures::StreamExt;
use tokio::sync::{mpsc, oneshot};
use zznet::connection::ClientConfig;
use zznet::runtime::client::ClientRuntime;

pub(crate) fn spawn_connection_manager(
    config: ClientConfig,
    internal_tx: mpsc::Sender<InternalEvent>,
    ready_tx: oneshot::Sender<Result<()>>,
) {
    tokio::spawn(async move {
        let client_runtime = ClientRuntime::new(config);
        let mut connection_stream = Box::pin(client_runtime.connections());

        // Handle the first connection separately to signal readiness.
        if let Some(first_connection_result) = connection_stream.next().await {
            match first_connection_result {
                Ok(conn) => {
                    log::info!("Client connection established.");
                    // Signal readiness
                    if ready_tx.send(Ok(())).is_err() {
                        log::error!("Readiness channel receiver dropped before signal was sent.");
                        return; // Actor is gone, no point in continuing.
                    }
                    // Send the connection to the actor.
                    if internal_tx.send(InternalEvent::NewClientConnection(conn)).await.is_err() {
                        log::error!("Actor receiver dropped. Shutting down connection manager.");
                        return;
                    }
                }
                Err(e) => {
                    log::error!("Client failed to establish initial connection: {}", e);
                    let _ = ready_tx.send(Err(anyhow!(
                        "Client failed to establish initial connection: {}",
                        e
                    )));
                    return; // Failed to start up.
                }
            }
        } else {
            // Stream ended without a single connection attempt.
            let _ = ready_tx.send(Err(anyhow!("Client connection stream ended prematurely.")));
            return;
        }

        // Loop for subsequent connections (e.g., after reconnects).
        while let Some(connection_result) = connection_stream.next().await {
            match connection_result {
                Ok(conn) => {
                    log::info!("Client re-established connection.");
                    if internal_tx.send(InternalEvent::NewClientConnection(conn)).await.is_err() {
                        log::error!("Actor receiver dropped. Shutting down connection manager.");
                        break;
                    }
                }
                Err(e) => {
                    log::warn!("Client connection error during reconnect: {}", e);
                    // We don't shut down here, as the runtime will keep trying.
                }
            }
        }
        log::info!("Client connection manager shutting down.");
    });
}
