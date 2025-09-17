// Minimal client example demonstrating the ZzNet client component.
// Usage: cargo run --bin zznet-client -- <addr>

use std::net::SocketAddr;
use std::time::Duration;
use zznet::component::{ZzNetBuilder, ZzNetClientApi, ZzNetConfig};
use zznet::connection::ClientConfig;
use zznet_api::Role;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let addr_str = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8443".to_string());
    let addr: SocketAddr = addr_str.parse()?;

    let config = ZzNetConfig::Client(ClientConfig {
        socketaddr: vec![addr],
        tls: None, // TLS is not set up for this example
        role: Role::ClientAdmin,
        reconnect_delay: Duration::from_secs(2),
        rooms_to_open: vec!["test-room".to_string()],
    });

    let handle = ZzNetBuilder::new(config).start().await?;

    log::info!("Client component started. Requesting 'test-room'...");

    let mut room = handle.get_room("test-room").await?;

    log::info!("Got room! Sending a message.");
    room.send(b"hello from client".to_vec()).await?;

    if let Some(reply) = room.recv().await? {
        log::info!("Got reply: {}", String::from_utf8_lossy(&reply));
    }

    handle.shutdown().await?;
    Ok(())
}
