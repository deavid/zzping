use crate::facade::InternalEvent;
use futures::StreamExt;
use tokio::sync::mpsc;
use zznet::connection::ServerConfig;
use zznet::connection_manager::ConnectionEvent;
use zznet::runtime::server::ServerRuntime;

// The listener no longer signals readiness. The actor does.
pub(crate) fn spawn_listener(
    config: ServerConfig,
    internal_tx: mpsc::Sender<InternalEvent>,
) {
    tokio::spawn(async move {
        let server_runtime = ServerRuntime::new(config);
        let mut connection_stream = match server_runtime.run().await {
            Ok(stream) => {
                log::info!("Server listener started successfully.");
                Box::pin(stream)
            }
            Err(e) => {
                // If the server fails to bind, we just log and terminate the task.
                // The actor itself will still be running and can be interacted with.
                log::error!("Server runtime failed to start, cannot accept connections: {e}");
                return;
            }
        };

        let mut next_client_id = 0;
        while let Some(Ok((connection, event_rx))) = connection_stream.next().await {
            let client_id = next_client_id;
            next_client_id += 1;

            log::info!("New server connection from client {client_id}");
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
                    "Actor has disappeared, shutting down event handler for client {client_id}."
                );
                break;
            }
        }
        log::info!("Event handler for client {client_id} shutting down.");
    });
}
