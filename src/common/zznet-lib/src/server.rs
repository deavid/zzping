// server.rs
use crate::facade::InternalEvent;
use anyhow::{anyhow, Result};
use futures::StreamExt;
use tokio::sync::{mpsc, oneshot};
use zznet::connection::ServerConfig;
use zznet::connection_manager::ConnectionEvent;
use zznet::runtime::server::ServerRuntime;

pub(crate) fn spawn_listener(
    config: ServerConfig,
    internal_tx: mpsc::Sender<InternalEvent>,
    ready_tx: oneshot::Sender<Result<()>>,
) {
    tokio::spawn(async move {
        let server_runtime = ServerRuntime::new(config);
        let mut connection_stream = match server_runtime.run().await {
            Ok(stream) => {
                // Successfully bound to port, signal readiness.
                if ready_tx.send(Ok(())).is_err() {
                    log::error!("Actor disappeared before listener could signal readiness.");
                    return;
                }
                Box::pin(stream)
            }
            Err(e) => {
                log::error!("Server runtime failed to start: {}", e);
                let _ = ready_tx.send(Err(anyhow!("Server runtime failed: {}", e)));
                return;
            }
        };

        let mut next_client_id = 0;
        while let Some(Ok((connection, event_rx))) = connection_stream.next().await {
            let client_id = next_client_id;
            next_client_id += 1;

            log::info!("New server connection from client {}", client_id);
            let event = InternalEvent::NewServerConnection {
                client_id,
                connection,
                event_rx,
            };

            if internal_tx.send(event).await.is_err() {
                log::error!("Actor has disappeared, shutting down listener task.");
                break;
            }
        }
        log::info!("Server listener task shutting down.");
    });
}

/// This function is spawned by the ZzNetActor for each new client connection.
/// It is responsible for forwarding all events from that connection to the actor.
pub(crate) fn spawn_per_client_event_handler(
    client_id: u64,
    mut event_rx: mpsc::Receiver<ConnectionEvent>,
    internal_tx: mpsc::Sender<InternalEvent>,
) {
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let internal_event = InternalEvent::ClientEvent { client_id, event };
            if internal_tx.send(internal_event).await.is_err() {
                log::warn!(
                    "Actor has disappeared, shutting down event handler for client {}.",
                    client_id
                );
                break;
            }
        }
        log::info!("Event handler for client {} shutting down.", client_id);
    });
}
