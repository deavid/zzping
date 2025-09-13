// Minimal server example using ServerRuntime for accepting connections.

use futures::StreamExt;
use std::net::SocketAddr;
use zznet::connection::{ServerConfig, TlsCfg};
use zznet::runtime::server::ServerRuntime;
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

    let config = ServerConfig {
        socketaddr: vec![addr],
        tls: Some(TlsCfg::from_role(Role::Database)),
        role: Role::Database,
    };
    let server = ServerRuntime::new(config);
    let connection_stream = server.run().await?;
    tokio::pin!(connection_stream);

    while let Some(connection_result) = connection_stream.next().await {
        match connection_result {
            Ok(_) => {
                // Connection actor is already spawned in Connection::new
            }
            Err(e) => {
                log::warn!("Connection error: {}", e);
            }
        }
    }

    Ok(())
}
