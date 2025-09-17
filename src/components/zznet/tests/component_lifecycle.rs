use anyhow::Result;
use ntest::timeout;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use zznet::component::{ZzNetBuilder, ZzNetConfig};
use zznet::connection::{ClientConfig, ServerConfig};
use zznet_api::Role;

fn create_server_config() -> ServerConfig {
    ServerConfig {
        socketaddr: vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0)],
        tls: None,
        role: Role::Database,
    }
}

#[tokio::test]
#[timeout(200)]
async fn zznet_component_starts_and_shuts_down() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    let server_config = create_server_config();
    let config = ZzNetConfig::Server(server_config);
    let builder = ZzNetBuilder::new(config);
    let handle = builder.start().await?;
    handle.shutdown().await?;
    Ok(())
}

use zznet::component::ZzNetClientApi;
use zznet::connection_manager::ConnectionEvent;
use tokio::sync::mpsc;

#[tokio::test]
#[timeout(200)]
async fn test_get_room_receives_room_from_mock_connection() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    // 1. Create a mock connection stream
    let (mock_conn_tx, mut mock_conn_rx) =
        mpsc::channel::<Result<(zznet::connection_manager::Connection, mpsc::Receiver<ConnectionEvent>)>>(1);
    let mock_stream = futures::stream::once(async move { mock_conn_rx.recv().await.unwrap() });

    // 2. Create a ZzNetComponent that will use our mock stream
    let room_name = "test-room".to_string();
    let config = ZzNetConfig::Client(ClientConfig {
        socketaddr: vec!["127.0.0.1:1234".parse().unwrap()],
        tls: None,
        role: Role::ClientAdmin,
        reconnect_delay: Duration::from_secs(1),
        rooms_to_open: vec![room_name.clone()],
    });

    // We need to manually construct the component and its parts to inject the mock stream
    let (command_tx, command_rx) = zzchorale::create_channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let handle = zzchorale::ComponentHandle {
        command_tx,
        shutdown_tx: std::sync::Arc::new(tokio::sync::Mutex::new(Some(shutdown_tx))),
        actor_handle: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
    };

    let mut component = zznet::component::ZzNetComponent::new(config, command_rx, handle.clone(), Some(Box::pin(mock_stream)));

    // 3. Start the component's run loop in the background
    let component_handle = handle.clone();
    tokio::spawn(async move {
        component.run(shutdown_rx).await
    });

    // 4. Act: Call get_room
    let get_room_future = component_handle.get_room(&room_name);

    // 5. Manually feed a mock connection and event into the component
    let (event_tx, event_rx) = mpsc::channel(1);
    let mock_connection = zznet::connection_manager::Connection::new(
        Box::new(tokio::io::duplex(1).0),
        mpsc::channel(1).0,
    );
    mock_conn_tx.send(Ok((mock_connection, event_rx))).await?;

    // Give the component a moment to spawn the event forwarder and send RequestRoom
    tokio::time::sleep(Duration::from_millis(10)).await;

    let (room_data_tx, room_data_rx) = mpsc::channel(1);
    event_tx.send(ConnectionEvent::ChannelOpened {
        name: room_name.clone(),
        id: 123,
        receiver: room_data_rx,
    }).await?;

    // 6. Assert: We receive the room from the API call
    let mut room = get_room_future.await?;

    // Optional: Test that the room works
    let test_payload = vec![1, 2, 3];
    room_data_tx.send(test_payload.clone()).await?;
    let received_payload = room.recv().await?.unwrap();
    assert_eq!(received_payload, test_payload);

    handle.shutdown().await?;
    Ok(())
}
