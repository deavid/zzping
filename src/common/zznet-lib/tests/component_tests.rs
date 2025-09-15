use anyhow::Result;
use async_trait::async_trait;
use ntest::timeout;
use tokio::sync::mpsc;
use zznet::connection::ServerConfig;
use zznet_api::{Role, ZzChannel};
use zznet_lib::{ActorCommand, ZzNet, ZzNetBuilder, ZzNetConfig, ZzNetHandle};

#[derive(Debug)]
struct MockChannel;

#[async_trait]
impl ZzChannel for MockChannel {
    async fn send(&self, _payload: Vec<u8>) -> Result<()> {
        Ok(())
    }
    async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

#[tokio::test]
#[timeout(100)]
async fn zznet_lib_actor_lifecycle() {
    let _ = env_logger::builder().is_test(true).try_init();
    log::info!("Testing actor lifecycle: start and shutdown");

    let server_config = ServerConfig {
        socketaddr: vec!["127.0.0.1:0".parse().unwrap()],
        tls: None,
        role: Role::Collector,
    };
    let config = ZzNetConfig::Server(server_config);
    let builder = ZzNetBuilder::new(config);

    // builder.start() now returns a tuple
    let (manager, _handle) = builder.start().await.unwrap();
    // shutdown is called on the manager
    manager.shutdown().await.unwrap();
}

#[tokio::test]
#[timeout(100)]
async fn zznet_lib_handle_api_request_response() {
    let _ = env_logger::builder().is_test(true).try_init();
    log::info!("Testing handle API request/response");

    let (command_tx, mut command_rx) = mpsc::channel(32);

    // Use the test-only constructor for the handle.
    let handle = ZzNetHandle::new(command_tx);

    // Spawn a task to simulate the actor.
    tokio::spawn(async move {
        let received_command = command_rx.recv().await.unwrap();
        if let ActorCommand::RequestChannel { name, response } = received_command {
            assert_eq!(name, "test");
            let dummy_channel = MockChannel;
            let response_result: Result<Box<dyn ZzChannel>> = Ok(Box::new(dummy_channel));
            response.send(response_result).unwrap();
        } else {
            panic!("Received unexpected command");
        }
    });

    // In the main test task, call the handle's API.
    let result = handle.request_channel("test".to_string()).await;

    assert!(result.is_ok());
    let channel = result.unwrap();
    assert!(channel.send(vec![]).await.is_ok());
}
