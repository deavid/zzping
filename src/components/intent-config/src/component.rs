use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use tokio::sync::{mpsc, oneshot, watch};
use zzchorale::{create_channel, spawn_component, ComponentHandle};
use zznet::component::{ZzNetClientApi, ZzNetServerApi};
use zznet_api::{ClientId, Role, ZzRoom};

// 1. DATA AND PROTOCOL DEFINITIONS
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct IntentConfigData {
    pub targets: Vec<IpAddr>,
    pub ping_rate_pps: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum ProtocolMsg {
    Update(IntentConfigData),
    Broadcast(IntentConfigData),
}

// 2. COMPONENT FRAMEWORK STRUCTS

pub struct IntentConfigBuilder<N: ZzNetServerApi + ZzNetClientApi + Clone> {
    role: Role,
    zznet_handle: N,
}

struct IntentConfigComponent<N: ZzNetServerApi + ZzNetClientApi + Clone> {
    role: Role,
    zznet_handle: N,
    config_watch_tx: watch::Sender<IntentConfigData>,
    update_tx: mpsc::Sender<IntentConfigData>,
    update_rx: Option<mpsc::Receiver<IntentConfigData>>, // To be consumed in on_start
}

pub enum IntentConfigCommand {
    Update(IntentConfigData, oneshot::Sender<Result<()>>),
    Subscribe(oneshot::Sender<watch::Receiver<IntentConfigData>>),
}

// 3. IMPLEMENTATION

impl<N: ZzNetServerApi + ZzNetClientApi + Clone + Send + Sync + 'static> IntentConfigBuilder<N> {
    pub fn new(role: Role, zznet_handle: N) -> Self {
        Self { role, zznet_handle }
    }

    pub async fn start(self) -> Result<ComponentHandle<IntentConfigCommand>> {
        let (watch_tx, _) = watch::channel(IntentConfigData::default());
        let (update_tx, update_rx) = mpsc::channel(32);

        let component = IntentConfigComponent {
            role: self.role,
            zznet_handle: self.zznet_handle,
            config_watch_tx: watch_tx,
            update_tx,
            update_rx: Some(update_rx), // Pass receiver to the component
        };

        let (command_tx, command_rx) = create_channel();
        let (handle, readiness) = spawn_component(component, command_tx, command_rx);
        readiness.await?;
        Ok(handle)
    }
}

#[async_trait]
impl<N: ZzNetServerApi + ZzNetClientApi + Clone + Send + Sync + 'static> zzchorale::Component
    for IntentConfigComponent<N>
{
    type Command = IntentConfigCommand;

    async fn on_start(&mut self) -> Result<()> {
        let update_rx = self.update_rx.take().expect("on_start called only once");
        match self.role {
            Role::Database => {
                log::info!("IntentConfig (Server) starting network task...");
                let client_stream = self
                    .zznet_handle
                    .listen_for_room("intent-config".to_string())
                    .await?;
                let watch_tx = self.config_watch_tx.clone();
                tokio::spawn(server_network_task(watch_tx, client_stream));
            }
            Role::ClientAdmin | Role::ClientRo => {
                log::info!("IntentConfig (Client) starting network task...");
                let zznet_handle = self.zznet_handle.clone();
                let watch_tx = self.config_watch_tx.clone();
                tokio::spawn(async move {
                    let channel = zznet_handle.get_room("intent-config").await.unwrap();
                    client_network_task(watch_tx, update_rx, channel).await;
                });
            }
            _ => return Err(anyhow!("Role not supported by IntentConfig")),
        };
        Ok(())
    }

    async fn handle_command(&mut self, command: Self::Command) -> Result<()> {
        match command {
            IntentConfigCommand::Update(data, response_tx) => {
                let res = if self.role == Role::ClientAdmin {
                    self.update_tx.send(data).await.map_err(|e| anyhow!(e))
                } else {
                    Err(anyhow!("Only ClientAdmin can update config"))
                };
                let _ = response_tx.send(res);
            }
            IntentConfigCommand::Subscribe(response_tx) => {
                let _ = response_tx.send(self.config_watch_tx.subscribe());
            }
        }
        Ok(())
    }
}

async fn server_network_task(
    watch_tx: watch::Sender<IntentConfigData>,
    mut client_stream: mpsc::Receiver<(ClientId, Box<dyn ZzRoom>)>,
) {
    while let Some((_id, mut channel)) = client_stream.recv().await {
        let mut broadcast_rx = watch_tx.subscribe();
        let watch_tx_clone = watch_tx.clone();

        tokio::spawn(async move {
            let initial_state = broadcast_rx.borrow().clone();
            let msg = serde_json::to_vec(&ProtocolMsg::Broadcast(initial_state)).unwrap();
            if channel.send(msg).await.is_err() {
                return;
            }

            loop {
                tokio::select! {
                    Ok(Some(payload)) = channel.recv() => {
                        if let Ok(ProtocolMsg::Update(data)) = serde_json::from_slice(&payload) {
                            let _ = watch_tx_clone.send(data);
                        }
                    },
                    Ok(_) = broadcast_rx.changed() => {
                        let new_state = broadcast_rx.borrow().clone();
                        let msg = serde_json::to_vec(&ProtocolMsg::Broadcast(new_state)).unwrap();
                        if channel.send(msg).await.is_err() { break; }
                    },
                    else => break,
                }
            }
        });
    }
}

async fn client_network_task(
    watch_tx: watch::Sender<IntentConfigData>,
    mut update_rx: mpsc::Receiver<IntentConfigData>,
    mut channel: Box<dyn ZzRoom>,
) {
    loop {
        tokio::select! {
            Ok(Some(payload)) = channel.recv() => {
                if let Ok(ProtocolMsg::Broadcast(data)) = serde_json::from_slice(&payload) {
                    if data != *watch_tx.borrow() {
                        let _ = watch_tx.send(data);
                    }
                }
            },
            Some(data) = update_rx.recv() => {
                let msg = serde_json::to_vec(&ProtocolMsg::Update(data)).unwrap();
                let _ = channel.send(msg).await;
            },
            else => break,
        }
    }
}

#[async_trait]
pub trait IntentConfigApi {
    async fn update(&self, new_config: IntentConfigData) -> Result<()>;
    async fn subscribe(&self) -> Result<watch::Receiver<IntentConfigData>>;
}

#[async_trait]
impl IntentConfigApi for ComponentHandle<IntentConfigCommand> {
    async fn update(&self, new_config: IntentConfigData) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = IntentConfigCommand::Update(new_config, response_tx);
        self.command_tx.send(command).await?;
        response_rx.await?
    }

    async fn subscribe(&self) -> Result<watch::Receiver<IntentConfigData>> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = IntentConfigCommand::Subscribe(response_tx);
        self.command_tx.send(command).await?;
        Ok(response_rx.await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ntest::timeout;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use zznet::component::{ZzNetBuilder, ZzNetConfig};
    use zznet::connection::{ClientConfig, ServerConfig};

    #[derive(Clone)]
    struct MockZzNetApi;

    #[async_trait]
    impl ZzNetServerApi for MockZzNetApi {
        async fn listen_for_room(
            &self,
            _name: String,
        ) -> Result<mpsc::Receiver<(ClientId, Box<dyn ZzRoom>)>> {
            let (tx, rx) = mpsc::channel(1);
            drop(tx);
            Ok(rx)
        }
    }

    #[async_trait]
    impl ZzNetClientApi for MockZzNetApi {
        async fn get_room(&self, _name: &str) -> Result<Box<dyn ZzRoom>> {
            #[derive(Debug)]
            struct MockChannel;
            #[async_trait]
            impl ZzRoom for MockChannel {
                async fn send(&self, _payload: Vec<u8>) -> Result<()> {
                    Ok(())
                }
                async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
                    Ok(None)
                }
            }
            Ok(Box::new(MockChannel))
        }
    }

    #[tokio::test]
    #[timeout(200)]
    async fn intent_config_component_lifecycle() {
        let _ = env_logger::builder().is_test(true).try_init();
        log::info!("Testing intent-config component lifecycle");

        let builder_server = IntentConfigBuilder::new(Role::Database, MockZzNetApi);
        let handle_server = builder_server.start().await.unwrap();
        handle_server.shutdown().await.unwrap();

        let builder_client = IntentConfigBuilder::new(Role::ClientAdmin, MockZzNetApi);
        let handle_client = builder_client.start().await.unwrap();
        handle_client.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[timeout(500)]
    #[ignore]
    async fn test_full_e2e_update_and_broadcast() {
        let _ = env_logger::builder().is_test(true).try_init();

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let workspace_root = std::path::Path::new(manifest_dir).join("../../../");
        let test_certs_dir = workspace_root.join("src/components/zznet/test_certs");
        let test_certs_path_str = test_certs_dir.to_str().unwrap();

        let server_addr = "127.0.0.1:12351".parse().unwrap();
        let server_config = ZzNetConfig::Server(ServerConfig {
            socketaddr: vec![server_addr],
            tls: Some(zznet::connection::TlsCfg::from_role(
                Role::Database,
                Some(test_certs_path_str),
            )),
            role: Role::Database,
        });
        let client_admin_config = ZzNetConfig::Client(ClientConfig {
            socketaddr: vec![server_addr],
            tls: Some(zznet::connection::TlsCfg::from_role(
                Role::ClientAdmin,
                Some(test_certs_path_str),
            )),
            role: Role::ClientAdmin,
            reconnect_delay: Duration::from_secs(1),
            rooms_to_open: vec!["intent-config".to_string()],
        });
        let client_ro_config = ZzNetConfig::Client(ClientConfig {
            socketaddr: vec![server_addr],
            tls: Some(zznet::connection::TlsCfg::from_role(
                Role::ClientRo,
                Some(test_certs_path_str),
            )),
            role: Role::ClientRo,
            reconnect_delay: Duration::from_secs(1),
            rooms_to_open: vec!["intent-config".to_string()],
        });

        let network_server_handle = ZzNetBuilder::new(server_config).start().await.unwrap();
        let network_admin_handle = ZzNetBuilder::new(client_admin_config).start().await.unwrap();
        let network_ro_handle = ZzNetBuilder::new(client_ro_config).start().await.unwrap();

        let server_component_builder =
            IntentConfigBuilder::new(Role::Database, network_server_handle.clone());
        let admin_component_builder =
            IntentConfigBuilder::new(Role::ClientAdmin, network_admin_handle.clone());
        let ro_component_builder =
            IntentConfigBuilder::new(Role::ClientRo, network_ro_handle.clone());

        let server_component_handle = server_component_builder.start().await.unwrap();
        let admin_component_handle = admin_component_builder.start().await.unwrap();
        let ro_component_handle = ro_component_builder.start().await.unwrap();

        let mut ro_config_watch = ro_component_handle.subscribe().await.unwrap();

        tokio::time::sleep(Duration::from_millis(500)).await;

        let new_config = IntentConfigData {
            targets: vec!["8.8.8.8".parse().unwrap()],
            ping_rate_pps: 50,
        };
        admin_component_handle.update(new_config.clone()).await.unwrap();

        ro_config_watch.changed().await.unwrap();
        let received_config = ro_config_watch.borrow().clone();
        assert_eq!(received_config, new_config);

        server_component_handle.shutdown().await.unwrap();
        admin_component_handle.shutdown().await.unwrap();
        ro_component_handle.shutdown().await.unwrap();

        network_server_handle.shutdown().await.unwrap();
        network_admin_handle.shutdown().await.unwrap();
        network_ro_handle.shutdown().await.unwrap();
    }
}
