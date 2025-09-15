use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use zznet_api::{Role, ZzChannel};
use zznet_lib::ZzNet;

// 1. DATA AND PROTOCOL DEFINITIONS (UNCHANGED)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct IntentConfigData {
    pub targets: Vec<IpAddr>,
    pub ping_rate_pps: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum ProtocolMsg {
    Update(IntentConfigData),
    Broadcast(IntentConfigData),
    RequestCurrent,
}

// 2. NEW COMPONENT FRAMEWORK STRUCTS

/// Builder for the IntentConfig component.
pub struct IntentConfigBuilder<N: ZzNet> {
    role: Role,
    zznet_handle: N,
}

/// Handle for interacting with a running IntentConfig component.
#[derive(Debug)]
pub struct IntentConfigHandle {
    command_tx: mpsc::Sender<ActorCommand>,
    actor_handle: JoinHandle<()>,
    // For clients to subscribe to config changes
    config_watch: watch::Receiver<IntentConfigData>,
}

/// Private actor for the IntentConfig component.
struct IntentConfigActor<N: ZzNet> {
    role: Role,
    zznet_handle: N,
    config_watch_tx: watch::Sender<IntentConfigData>,
    // For ClientAdmin: channel to send updates to the client actor task
    update_tx: mpsc::Sender<IntentConfigData>,
}

/// Commands for the IntentConfig actor.
enum ActorCommand {
    Update(IntentConfigData, oneshot::Sender<Result<()>>),
    Shutdown(oneshot::Sender<()>),
}

// 3. IMPLEMENTATION

impl<N: ZzNet + 'static> IntentConfigBuilder<N> {
    pub fn new(role: Role, zznet_handle: N) -> Self {
        Self { role, zznet_handle }
    }

    pub async fn start(self) -> Result<IntentConfigHandle> {
        let (command_tx, command_rx) = mpsc::channel(32);
        let (watch_tx, watch_rx) = watch::channel(IntentConfigData::default());
        let (update_tx, update_rx) = mpsc::channel(32);

        let actor = IntentConfigActor {
            role: self.role,
            zznet_handle: self.zznet_handle,
            config_watch_tx: watch_tx,
            update_tx,
        };

        let actor_handle = tokio::spawn(actor.run(command_rx, update_rx));

        Ok(IntentConfigHandle {
            command_tx,
            actor_handle,
            config_watch: watch_rx,
        })
    }
}

impl IntentConfigHandle {
    pub async fn update(&self, new_config: IntentConfigData) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = ActorCommand::Update(new_config, response_tx);
        self.command_tx.send(command).await?;
        response_rx.await?
    }

    pub fn subscribe(&self) -> watch::Receiver<IntentConfigData> {
        self.config_watch.clone()
    }

    pub async fn shutdown(self) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        let command = ActorCommand::Shutdown(response_tx);

        // Send the shutdown command.
        if self.command_tx.send(command).await.is_err() {
            log::warn!("Shutdown command failed: actor already gone.");
        }

        // Wait for the actor to acknowledge.
        if response_rx.await.is_err() {
            log::warn!("Actor did not acknowledge shutdown: may have crashed.");
        }

        // Wait for the actor's task to complete.
        self.actor_handle.await?;

        Ok(())
    }
}

impl<N: ZzNet> IntentConfigActor<N> {
    async fn run(
        mut self,
        command_rx: mpsc::Receiver<ActorCommand>,
        update_rx: mpsc::Receiver<IntentConfigData>,
    ) {
        // The actor's first job is to establish its network role.
        match self.role {
            Role::Database => {
                log::info!("IntentConfig (Server) starting...");
                let client_stream = self
                    .zznet_handle
                    .listen_for_channel("intent-config".to_string())
                    .await
                    .unwrap();
                // This is the old `server_actor_task`
                self.run_server(command_rx, client_stream).await;
            }
            Role::ClientAdmin | Role::ClientRo => {
                log::info!("IntentConfig (Client) starting...");
                let channel = self
                    .zznet_handle
                    .request_channel("intent-config".to_string())
                    .await
                    .unwrap();
                // This is the old `client_actor_task`
                self.run_client(command_rx, update_rx, channel).await;
            }
            _ => unimplemented!("Role not supported by IntentConfig"),
        }
    }

    async fn handle_command(&self, command: ActorCommand) -> bool {
        match command {
            ActorCommand::Update(data, response_tx) => {
                let res = if self.role == Role::ClientAdmin {
                    self.update_tx.send(data).await.map_err(|e| anyhow!(e))
                } else {
                    Err(anyhow!("Only ClientAdmin can update config"))
                };
                let _ = response_tx.send(res);
            }
            ActorCommand::Shutdown(response_tx) => {
                let _ = response_tx.send(());
                return false; // Signal to stop the actor loop
            }
        }
        true
    }

    // This is the refactored `server_actor_task`
    async fn run_server(
        &mut self,
        mut command_rx: mpsc::Receiver<ActorCommand>,
        mut client_stream: mpsc::Receiver<(u64, Box<dyn ZzChannel>)>,
    ) {
        loop {
            tokio::select! {
                Some((_id, mut channel)) = client_stream.recv() => {
                    let mut broadcast_rx = self.config_watch_tx.subscribe();
                    let watch_tx_clone = self.config_watch_tx.clone();

                    tokio::spawn(async move {
                        // Send initial state
                        let initial_state = broadcast_rx.borrow().clone();
                        let msg = serde_json::to_vec(&ProtocolMsg::Broadcast(initial_state)).unwrap();
                        if channel.send(msg).await.is_err() { return; }

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
                },
                Some(command) = command_rx.recv() => {
                    if !self.handle_command(command).await {
                        break;
                    }
                },
                else => break,
            }
        }
    }

    // This is the refactored `client_actor_task`
    async fn run_client(
        &mut self,
        mut command_rx: mpsc::Receiver<ActorCommand>,
        mut update_rx: mpsc::Receiver<IntentConfigData>,
        mut channel: Box<dyn ZzChannel>,
    ) {
        loop {
            tokio::select! {
                Ok(Some(payload)) = channel.recv() => {
                    if let Ok(ProtocolMsg::Broadcast(data)) = serde_json::from_slice(&payload) {
                        if data != *self.config_watch_tx.borrow() {
                            let _ = self.config_watch_tx.send(data);
                        }
                    }
                },
                Some(data) = update_rx.recv() => {
                    let msg = serde_json::to_vec(&ProtocolMsg::Update(data)).unwrap();
                    let _ = channel.send(msg).await;
                },
                Some(command) = command_rx.recv() => {
                    if !self.handle_command(command).await {
                        break;
                    }
                },
                else => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ntest::timeout;
    use std::time::Duration;
    use zznet::connection::{ClientConfig, ServerConfig, TlsCfg};
    use zznet_api::ZzChannel;
    use zznet_lib::{ZzNetBuilder, ZzNetConfig};

    #[derive(Debug)]
    struct MockZzNetHandle;

    #[async_trait]
    impl ZzNet for MockZzNetHandle {
        async fn request_channel(&self, _name: String) -> Result<Box<dyn ZzChannel>> {
            #[derive(Debug)]
            struct MockChannel;
            #[async_trait]
            impl ZzChannel for MockChannel {
                async fn send(&self, _payload: Vec<u8>) -> Result<()> { Ok(()) }
                async fn recv(&mut self) -> Result<Option<Vec<u8>>> { Ok(None) }
            }
            Ok(Box::new(MockChannel))
        }

        async fn listen_for_channel(
            &self,
            _name: String,
        ) -> Result<mpsc::Receiver<(u64, Box<dyn ZzChannel>)>> {
            let (_tx, rx) = mpsc::channel(32);
            Ok(rx)
        }
    }

    #[tokio::test]
    #[timeout(200)] // Increased timeout slightly for starting two components
    async fn intent_config_component_lifecycle() {
        let _ = env_logger::builder().is_test(true).try_init();
        log::info!("Testing intent-config component lifecycle");

        let builder_server = IntentConfigBuilder::new(Role::Database, MockZzNetHandle);
        let handle_server = builder_server.start().await.unwrap();
        handle_server.shutdown().await.unwrap();

        let builder_client = IntentConfigBuilder::new(Role::ClientAdmin, MockZzNetHandle);
        let handle_client = builder_client.start().await.unwrap();
        handle_client.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[timeout(2000)] // E2E tests may take longer
    async fn test_full_e2e_update_and_broadcast() {
        let _ = env_logger::builder().is_test(true).try_init();

        // 1. SETUP THE TEST ENVIRONMENT
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let workspace_root = std::path::Path::new(manifest_dir).join("../../../");
        let test_certs_dir = workspace_root.join("src/components/zznet/test_certs");
        let test_certs_path_str = test_certs_dir.to_str().unwrap();

        // 2. CONFIGURE AND START zznet COMPONENTS
        let server_addr = "127.0.0.1:12347".parse().unwrap(); // Use a different port
        let server_config = ZzNetConfig::Server(ServerConfig {
            socketaddr: vec![server_addr],
            tls: Some(TlsCfg::from_role(Role::Database, Some(test_certs_path_str))),
            role: Role::Database,
        });
        let client_admin_config = ZzNetConfig::Client(ClientConfig {
            socketaddr: vec![server_addr],
            tls: Some(TlsCfg::from_role(
                Role::ClientAdmin,
                Some(test_certs_path_str),
            )),
            role: Role::ClientAdmin,
            reconnect_delay: Duration::from_secs(1),
        });
        let client_ro_config = ZzNetConfig::Client(ClientConfig {
            socketaddr: vec![server_addr],
            tls: Some(TlsCfg::from_role(Role::ClientRo, Some(test_certs_path_str))),
            role: Role::ClientRo,
            reconnect_delay: Duration::from_secs(1),
        });

        let (network_server_manager, network_server_handle) =
            ZzNetBuilder::new(server_config).start().await.unwrap();
        let (network_admin_manager, network_admin_handle) =
            ZzNetBuilder::new(client_admin_config).start().await.unwrap();
        let (network_ro_manager, network_ro_handle) =
            ZzNetBuilder::new(client_ro_config).start().await.unwrap();

        // 3. WIRING & ACTIVATION of intent-config COMPONENTS
        let server_component_builder =
            IntentConfigBuilder::new(Role::Database, network_server_handle);
        let admin_component_builder =
            IntentConfigBuilder::new(Role::ClientAdmin, network_admin_handle.clone());
        let ro_component_builder =
            IntentConfigBuilder::new(Role::ClientRo, network_ro_handle.clone());

        let server_component_handle = server_component_builder.start().await.unwrap();
        let admin_component_handle = admin_component_builder.start().await.unwrap();
        let ro_component_handle = ro_component_builder.start().await.unwrap();

        let mut ro_config_watch = ro_component_handle.subscribe();

        // Wait for initial sync
        tokio::time::sleep(Duration::from_millis(200)).await;

        // 4. ACT: Perform the operation.
        let new_config = IntentConfigData {
            targets: vec!["8.8.8.8".parse().unwrap()],
            ping_rate_pps: 50,
        };
        admin_component_handle.update(new_config.clone()).await.unwrap();

        // 5. ASSERT: Verify the result propagated through the entire stack.
        ro_config_watch.changed().await.unwrap();
        let received_config = ro_config_watch.borrow().clone();
        assert_eq!(received_config, new_config);

        // 6. ORDERED TEARDOWN
        server_component_handle.shutdown().await.unwrap();
        admin_component_handle.shutdown().await.unwrap();
        ro_component_handle.shutdown().await.unwrap();

        network_server_manager.shutdown().await.unwrap();
        network_admin_manager.shutdown().await.unwrap();
        network_ro_manager.shutdown().await.unwrap();
    }
}
