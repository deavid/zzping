// Minimal server example demonstrating the ZzNet server component.
// Usage: cargo run --bin zznet-server -- <listen_addr>

use std::net::SocketAddr;
use zznet::component::{ZzNetBuilder, ZzNetConfig, ZzNetServerApi};
use zznet::connection::ServerConfig;
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

    let config = ZzNetConfig::Server(ServerConfig {
        socketaddr: vec![addr],
        tls: None, // TLS is not set up for this example
        role: Role::Database,
    });

    let handle = ZzNetBuilder::new(config).start().await?;

    log::info!("Server component started. Listening for 'test-room'...");

    let mut room_listener = handle.listen_for_room("test-room".to_string()).await?;

    while let Some((client_id, mut room)) = room_listener.recv().await {
        log::info!("Client {client_id} connected and opened 'test-room'");
        tokio::spawn(async move {
            while let Ok(Some(msg)) = room.recv().await {
                log::info!(
                    "Client {client_id} sent: {}",
                    String::from_utf8_lossy(&msg)
                );
                room.send(msg).await.unwrap(); // Echo back
            }
            log::info!("Client {client_id} disconnected.");
        });
    }

    handle.shutdown().await?;
    Ok(())
}
