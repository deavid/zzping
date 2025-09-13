// client.rs
use futures::StreamExt;
use std::sync::{Arc, Mutex};
use zznet::connection::ClientConfig;
use zznet::connection_manager::Connection;
use zznet::runtime::client::ClientRuntime;

pub(crate) fn start_runtime(
    config: ClientConfig,
    connection: Arc<Mutex<Option<Connection>>>,
) {
    tokio::spawn(async move {
        let client_runtime = ClientRuntime::new(config);
        let mut connection_stream = Box::pin(client_runtime.connections());
        while let Some(Ok(new_connection)) = connection_stream.next().await {
            *connection.lock().unwrap() = Some(new_connection);
        }
    });
}