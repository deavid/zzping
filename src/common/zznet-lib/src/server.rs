// server.rs
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use zznet::connection::ServerConfig;
use zznet::connection_manager::{Channel, ConnectionCommand, ConnectionEvent};
use zznet::runtime::server::ServerRuntime;
use zznet_api::ZzChannel;

pub(crate) fn start_runtime(
    config: ServerConfig,
    listeners: Arc<Mutex<HashMap<String, mpsc::Sender<(u64, Box<dyn ZzChannel>)>>>>,
    next_client_id: Arc<Mutex<u64>>,
) {
    tokio::spawn(async move {
        let server_runtime = ServerRuntime::new(config);
        let mut connection_stream = match server_runtime.run().await {
            Ok(stream) => Box::pin(stream),
            Err(e) => {
                log::error!("Server runtime failed to start: {}", e);
                return;
            }
        };

        while let Some(Ok((connection, event_rx))) = connection_stream.next().await {
            let client_id = {
                let mut id = next_client_id.lock().unwrap();
                *id += 1;
                *id
            };
            let command_tx = connection.command_sender();
            let listeners_clone = Arc::clone(&listeners);
            handle_connection_events(client_id, command_tx, event_rx, listeners_clone);
        }
    });
}

fn handle_connection_events(
    client_id: u64,
    command_tx: mpsc::Sender<ConnectionCommand>,
    mut event_rx: mpsc::Receiver<ConnectionEvent>,
    listeners: Arc<Mutex<HashMap<String, mpsc::Sender<(u64, Box<dyn ZzChannel>)>>>>,
) {
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let ConnectionEvent::ChannelOpened { name, id, receiver } = event;
            let channel = Channel {
                id,
                command_tx: command_tx.clone(),
                rx: receiver,
            };

            let listener_tx = {
                let listeners = listeners.lock().unwrap();
                listeners.get(&name).cloned()
            };

            if let Some(listener_tx) = listener_tx {
                if listener_tx
                    .send((client_id, Box::new(channel)))
                    .await
                    .is_err()
                {
                    log::warn!("A listener for channel '{}' was dropped.", name);
                }
            }
        }
    });
}
