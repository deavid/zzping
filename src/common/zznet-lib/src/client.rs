use crate::facade::{ActorCommand, InternalEvent};
use futures::StreamExt;
use tokio::sync::mpsc;
use zznet::connection::ClientConfig;
use zznet::runtime::client::ClientRuntime;

// The connection manager no longer signals readiness. The actor does.
pub(crate) fn spawn_connection_manager(
    config: ClientConfig,
    command_tx: mpsc::Sender<ActorCommand>,
) {
    tokio::spawn(async move {
        let client_runtime = ClientRuntime::new(config);
        let mut connection_stream = Box::pin(client_runtime.connections());

        log::info!("Client connection manager started. Waiting for connections...");

        // Loop for initial connection and subsequent reconnects.
        while let Some(connection_result) = connection_stream.next().await {
            match connection_result {
                Ok(conn) => {
                    log::info!("Client connection established or re-established.");
                    // Send the new connection to the actor.
                    let event = InternalEvent::NewClientConnection(conn);
                    if command_tx.send(ActorCommand::Internal(event)).await.is_err() {
                        log::error!("Actor receiver dropped. Shutting down connection manager.");
                        break; // Actor is gone, no point in continuing.
                    }
                }
                Err(e) => {
                    log::warn!("Client connection error: {e}. Runtime will continue retrying.",);
                    // We don't shut down here, as the runtime will keep trying.
                }
            }
        }
        log::info!("Client connection manager shutting down because the connection stream ended.");
    });
}
