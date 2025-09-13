// facade.rs
use crate::{client, config::ZzNetConfig, server};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use zznet::connection_manager::Connection;
use zznet_api::ZzChannel;

pub struct ZzNet {
    pub(crate) client_connection: Arc<Mutex<Option<Connection>>>,
    pub(crate) server_listeners: Arc<Mutex<HashMap<String, mpsc::Sender<(u64, Box<dyn ZzChannel>)>>>>,
    pub(crate) next_client_id: Arc<Mutex<u64>>,
}

impl ZzNet {
    pub fn new(config: ZzNetConfig) -> Self {
        let net = Self {
            client_connection: Arc::new(Mutex::new(None)),
            server_listeners: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: Arc::new(Mutex::new(0)),
        };

        match config {
            ZzNetConfig::Client(client_config) => {
                client::start_runtime(client_config, Arc::clone(&net.client_connection));
            }
            ZzNetConfig::Server(server_config) => {
                server::start_runtime(
                    server_config,
                    Arc::clone(&net.server_listeners),
                    Arc::clone(&net.next_client_id),
                );
            }
        }
        net
    }

    pub async fn listen_for_channel(&self, name: &str) -> Result<mpsc::Receiver<(u64, Box<dyn ZzChannel>)>> {
        let (tx, rx) = mpsc::channel(32);
        self.server_listeners
            .lock()
            .unwrap()
            .insert(name.to_string(), tx);
        Ok(rx)
    }

    pub async fn request_channel(&self, name: String) -> Result<Box<dyn ZzChannel>> {
        if let Some(conn) = self.client_connection.lock().unwrap().as_ref() {
            let channel = conn.request_channel(name).await?;
            Ok(Box::new(channel))
        } else {
            Err(anyhow::anyhow!("No active connection"))
        }
    }
}
