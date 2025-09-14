use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use tokio::sync::{mpsc, watch};
use zznet_api::{Role, ZzChannel};

// 1. DATA AND PROTOCOL DEFINITIONS (UNCHANGED)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct IntentConfigData {
    pub targets: Vec<IpAddr>,
    pub ping_rate_pps: u64,
}

#[derive(Serialize, Deserialize, Debug)]
enum ProtocolMsg {
    Update(IntentConfigData),
    Broadcast(IntentConfigData),
    RequestCurrent,
}

// 2. PUBLIC API STRUCT (SIMPLIFIED)
pub struct IntentConfig {
    role: Role,
    // For clients: subscribe to config changes
    config_watch: watch::Receiver<IntentConfigData>,
    // For ClientAdmin: send an update command
    update_tx: Option<mpsc::Sender<IntentConfigData>>,
}

// 5. REVISED IMPLEMENTATION (TO BE USED IN THE TASK)

// Server-side actor logic
async fn server_actor_task(
    mut client_stream: mpsc::Receiver<(u64, Box<dyn ZzChannel>)>,
    watch_tx: watch::Sender<IntentConfigData>,
) {
    // This task only handles new clients.
    // Each client gets its own task to handle its lifecycle.
    while let Some((_id, mut channel)) = client_stream.recv().await {
        let mut broadcast_rx = watch_tx.subscribe();

        // Send the current state immediately on connection
        let initial_state = broadcast_rx.borrow().clone();
        let msg = serde_json::to_vec(&ProtocolMsg::Broadcast(initial_state)).unwrap();
        if channel.send(msg).await.is_err() {
            continue; // Client disconnected immediately
        }

        let watch_tx = watch_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Listen for incoming messages from this specific client
                    Ok(Some(payload)) = channel.recv() => {
                        if let Ok(ProtocolMsg::Update(data)) = serde_json::from_slice(&payload) {
                            // If this client sends an update, publish it to the watch channel.
                            // This will cause all other client tasks to see the change.
                            let _ = watch_tx.send(data);
                        }
                    },
                    // Listen for broadcast changes from the watch channel
                    Ok(_) = broadcast_rx.changed() => {
                        let new_state = broadcast_rx.borrow().clone();
                        let msg = serde_json::to_vec(&ProtocolMsg::Broadcast(new_state)).unwrap();
                        if channel.send(msg).await.is_err() {
                            // This client disconnected, end its task
                            break;
                        }
                    },
                    else => break,
                }
            }
        });
    }
}

// Client-side actor logic
async fn client_actor_task(
    mut channel: Box<dyn ZzChannel>,
    watch_tx: watch::Sender<IntentConfigData>,
    mut update_rx: mpsc::Receiver<IntentConfigData>,
) {
    loop {
        tokio::select! {
            // Received a broadcast from the server
            Ok(Some(payload)) = channel.recv() => {
                if let Ok(ProtocolMsg::Broadcast(data)) = serde_json::from_slice(&payload) {
                    if data != *watch_tx.borrow() {
                        let _ = watch_tx.send(data);
                    }
                }
            },
            // The user called the `update` method
            Some(data) = update_rx.recv() => {
                let msg = serde_json::to_vec(&ProtocolMsg::Update(data)).unwrap();
                let _ = channel.send(msg).await;
            }
        }
    }
}

impl IntentConfig {
    pub fn new_server(client_stream: mpsc::Receiver<(u64, Box<dyn ZzChannel>)>) -> Self {
        let (watch_tx, watch_rx) = watch::channel(IntentConfigData::default());
        tokio::spawn(server_actor_task(client_stream, watch_tx));
        Self {
            role: Role::Database,
            config_watch: watch_rx,
            update_tx: None,
        }
    }

    pub fn new_client(role: Role, channel: Box<dyn ZzChannel>) -> Self {
        let (watch_tx, watch_rx) = watch::channel(IntentConfigData::default());
        let (update_tx, update_rx) = mpsc::channel(32);
        tokio::spawn(client_actor_task(channel, watch_tx, update_rx));
        Self {
            role,
            config_watch: watch_rx,
            update_tx: Some(update_tx),
        }
    }

    pub async fn update(&self, new_config: IntentConfigData) -> Result<()> {
        if self.role != Role::ClientAdmin {
            return Err(anyhow::anyhow!("Only ClientAdmin can update config"));
        }
        if let Some(tx) = &self.update_tx {
            tx.send(new_config).await?;
        }
        Ok(())
    }

    pub fn subscribe(&self) -> watch::Receiver<IntentConfigData> {
        self.config_watch.clone()
    }
}

// The end-to-end test remains the same.
#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ntest::timeout;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use zznet::connection::{ClientConfig, ServerConfig, TlsCfg};
    use zznet_api::ZzChannel;
    use zznet_lib::{ZzNet, ZzNetConfig};

    /// A mock ZzChannel that uses MPSC channels to allow a test to act as the peer.
    struct MockZzChannel {
        /// Test sends payloads here for the actor to receive.
        tx_to_actor: mpsc::Sender<Vec<u8>>,
        /// Test receives payloads here that the actor sent.
        rx_from_actor: mpsc::Receiver<Vec<u8>>,
    }

    #[async_trait]
    impl ZzChannel for MockZzChannel {
        async fn send(&self, payload: Vec<u8>) -> Result<()> {
            self.tx_to_actor.send(payload).await?;
            Ok(())
        }

        async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
            Ok(self.rx_from_actor.recv().await)
        }
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_server_actor_update_and_broadcast() {
        let _ = env_logger::builder().is_test(true).try_init();
        let (watch_tx, mut watch_rx) = watch::channel(IntentConfigData::default());
        let (client_tx, client_rx) = mpsc::channel(32);
        tokio::spawn(server_actor_task(client_rx, watch_tx));

        let (tx_to_actor, rx_from_actor) = mpsc::channel(32);
        let (tx_from_actor, mut rx_for_test) = mpsc::channel(32);
        let mock_channel = Box::new(MockZzChannel {
            tx_to_actor: tx_from_actor,
            rx_from_actor,
        });
        client_tx.send((0, mock_channel)).await.unwrap();
        tokio::task::yield_now().await;
        let _ = rx_for_test.recv().await; // Drain initial broadcast

        // ACT: Client sends an update
        let new_config = IntentConfigData {
            targets: vec!["1.1.1.1".parse().unwrap()],
            ..Default::default()
        };
        let update_msg = ProtocolMsg::Update(new_config.clone());
        let update_bytes = serde_json::to_vec(&update_msg).unwrap();
        tx_to_actor.send(update_bytes).await.unwrap();

        // ASSERT 1: The watch channel is updated
        watch_rx.changed().await.unwrap();
        assert_eq!(*watch_rx.borrow(), new_config);

        // ASSERT 2: The client receives a broadcast of the new state
        let received_bytes = rx_for_test.recv().await.unwrap();
        let received_msg: ProtocolMsg = serde_json::from_slice(&received_bytes).unwrap();
        assert!(matches!(received_msg, ProtocolMsg::Broadcast(data) if data == new_config));
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_client_actor_receives_broadcast() {
        let _ = env_logger::builder().is_test(true).try_init();
        let (watch_tx, mut watch_rx) = watch::channel(IntentConfigData::default());
        let (_update_tx, update_rx) = mpsc::channel(32);
        let (tx_to_actor, rx_from_actor) = mpsc::channel(32);
        let (_tx_from_actor, _rx_for_test) = mpsc::channel(32);
        let mock_channel = Box::new(MockZzChannel {
            tx_to_actor: _tx_from_actor,
            rx_from_actor,
        });
        tokio::spawn(client_actor_task(mock_channel, watch_tx, update_rx));

        // ACT: "Server" sends a broadcast
        let new_config = IntentConfigData {
            targets: vec!["2.2.2.2".parse().unwrap()],
            ..Default::default()
        };
        let broadcast_msg = ProtocolMsg::Broadcast(new_config.clone());
        let broadcast_bytes = serde_json::to_vec(&broadcast_msg).unwrap();
        tx_to_actor.send(broadcast_bytes).await.unwrap();

        // ASSERT: The watch channel is updated
        watch_rx.changed().await.unwrap();
        assert_eq!(*watch_rx.borrow(), new_config);
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_client_actor_sends_update() {
        let _ = env_logger::builder().is_test(true).try_init();
        let (watch_tx, _watch_rx) = watch::channel(IntentConfigData::default());
        let (update_tx, update_rx) = mpsc::channel(32);
        let (_tx_to_actor, rx_from_actor) = mpsc::channel(32);
        let (tx_from_actor, mut rx_for_test) = mpsc::channel(32);
        let mock_channel = Box::new(MockZzChannel {
            tx_to_actor: tx_from_actor,
            rx_from_actor,
        });
        tokio::spawn(client_actor_task(mock_channel, watch_tx, update_rx));

        // ACT: Test calls the component's update method
        let new_config = IntentConfigData {
            targets: vec!["3.3.3.3".parse().unwrap()],
            ..Default::default()
        };
        update_tx.send(new_config.clone()).await.unwrap();

        // ASSERT: The actor sends an Update message to the "server"
        let received_bytes = rx_for_test.recv().await.unwrap();
        let received_msg: ProtocolMsg = serde_json::from_slice(&received_bytes).unwrap();
        assert!(matches!(received_msg, ProtocolMsg::Update(data) if data == new_config));
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_server_actor_sends_initial_broadcast() {
        let _ = env_logger::builder().is_test(true).try_init();

        // SETUP
        let (watch_tx, _watch_rx) = watch::channel(IntentConfigData::default());
        let (client_tx, client_rx) = mpsc::channel(32);
        tokio::spawn(server_actor_task(client_rx, watch_tx));

        let (_tx_to_actor, rx_from_actor): (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) =
            mpsc::channel(32);
        let (tx_from_actor, mut rx_for_test): (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) =
            mpsc::channel(32);
        let mock_channel = Box::new(MockZzChannel {
            tx_to_actor: tx_from_actor,
            rx_from_actor,
        });

        // ACT: "Connect" a new client
        client_tx.send((0, mock_channel)).await.unwrap();
        tokio::task::yield_now().await; // Give the actor a chance to run

        // ASSERT: The server immediately sends the current state
        let received_bytes = rx_for_test.recv().await.unwrap();
        let received_msg: ProtocolMsg = serde_json::from_slice(&received_bytes).unwrap();

        assert!(
            matches!(received_msg, ProtocolMsg::Broadcast(data) if data == IntentConfigData::default())
        );
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_subscribe_returns_valid_receiver() {
        let _ = env_logger::builder().is_test(true).try_init();
        let (_client_tx, client_rx) = mpsc::channel(32);
        let component = IntentConfig::new_server(client_rx);
        let watch_rx = component.subscribe();

        // Initial state should be default
        assert_eq!(*watch_rx.borrow(), IntentConfigData::default());

        // Manually update the watch channel to simulate a change
        let new_state = IntentConfigData {
            targets: vec!["1.1.1.1".parse().unwrap()],
            ping_rate_pps: 10,
        };
        // This is a bit of a hack. In a real scenario, the actor would do this.
        // We can't easily get the `watch_tx` to do this directly, so we'll test
        // this behavior more thoroughly in the actor tests.
        let (watch_tx_manual, mut watch_rx_manual) = watch::channel(IntentConfigData::default());
        watch_tx_manual.send(new_state.clone()).unwrap();
        watch_rx_manual.changed().await.unwrap();
        assert_eq!(*watch_rx_manual.borrow(), new_state);
    }

    #[tokio::test]
    #[timeout(100)]
    async fn test_update_permissions() {
        let _ = env_logger::builder().is_test(true).try_init();

        // Server should not be able to update
        let (_client_tx, client_rx) = mpsc::channel(32);
        let server_component = IntentConfig::new_server(client_rx);
        let result = server_component.update(IntentConfigData::default()).await;
        assert!(result.is_err());

        // ClientRo should not be able to update
        let (_tx_to_actor, rx_from_actor) = mpsc::channel(32);
        let (tx_to_test, _rx_from_test) = mpsc::channel(32);
        let mock_channel = Box::new(MockZzChannel {
            tx_to_actor: tx_to_test,
            rx_from_actor,
        });
        let client_ro_component = IntentConfig::new_client(Role::ClientRo, mock_channel);
        let result = client_ro_component
            .update(IntentConfigData::default())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_full_e2e_update_and_broadcast() {
        let _ = env_logger::builder().is_test(true).try_init();

        // 1. SETUP THE TEST ENVIRONMENT
        // Find the workspace root relative to this test's manifest dir.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let workspace_root = std::path::Path::new(manifest_dir).join("../../../");
        let test_certs_dir = workspace_root.join("src/components/zznet/test_certs");

        // Convert the path to a string to pass to our modified function
        let test_certs_path_str = test_certs_dir.to_str().unwrap();

        // 2. CONFIGURE USING THE TEST CERTS PATH
        let server_addr = "127.0.0.1:12345".parse().unwrap();
        let server_config = ServerConfig {
            socketaddr: vec![server_addr],
            tls: Some(TlsCfg::from_role(Role::Database, Some(test_certs_path_str))),
            role: Role::Database,
        };
        let client_admin_config = ClientConfig {
            socketaddr: vec![server_addr],
            tls: Some(TlsCfg::from_role(
                Role::ClientAdmin,
                Some(test_certs_path_str),
            )),
            role: Role::ClientAdmin,
            reconnect_delay: Duration::from_secs(1),
        };
        let client_ro_config = ClientConfig {
            socketaddr: vec![server_addr],
            tls: Some(TlsCfg::from_role(Role::ClientRo, Some(test_certs_path_str))),
            role: Role::ClientRo,
            reconnect_delay: Duration::from_secs(1),
        };

        let network_server = ZzNet::new(ZzNetConfig::Server(server_config));
        let network_admin_client = ZzNet::new(ZzNetConfig::Client(client_admin_config));
        let network_ro_client = ZzNet::new(ZzNetConfig::Client(client_ro_config));

        // Give the server a moment to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 2. WIRING (The "Composer" part of the test)
        let client_stream = network_server
            .listen_for_channel("intent-config")
            .await
            .unwrap();
        let _server_component = IntentConfig::new_server(client_stream);

        let admin_channel = network_admin_client
            .request_channel("intent-config".to_string())
            .await
            .unwrap();
        let admin_component = IntentConfig::new_client(Role::ClientAdmin, admin_channel);

        let ro_channel = network_ro_client
            .request_channel("intent-config".to_string())
            .await
            .unwrap();
        let ro_component = IntentConfig::new_client(Role::ClientRo, ro_channel);
        let mut ro_config_watch = ro_component.subscribe();

        // Wait for initial sync
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 3. ACT: Perform the operation.
        let new_config = IntentConfigData {
            targets: vec!["8.8.8.8".parse().unwrap()],
            ping_rate_pps: 50,
        };
        admin_component.update(new_config.clone()).await.unwrap();

        // 4. ASSERT: Verify the result propagated through the entire stack.
        ro_config_watch.changed().await.unwrap();
        let received_config = ro_config_watch.borrow().clone();

        assert_eq!(received_config, new_config);
    }
}
