// Minimal client example using ClientRuntime for resilient connections.
// Usage: cargo run --bin zznet-client -- <addr>

use futures::StreamExt;
use rmp_serde::encode;
use std::net::SocketAddr;
use tokio::time::Duration;
use zznet::connection::{ClientConfig, TlsCfg};
use zznet::proto;
use zznet::proto::messages::{ControlMsg, Frame};
use zznet::runtime::client::ClientRuntime;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let addr_str = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8443".to_string());
    let addr: SocketAddr = addr_str.parse()?;

    let config = ClientConfig {
        socketaddr: vec![addr],
        tls: Some(TlsCfg::from_role(proto::hello::Role::Collector)),
        role: proto::hello::Role::Collector,
        reconnect_delay: Duration::from_secs(5),
    };
    let client = ClientRuntime::new(config);
    let connection_stream = client.connections();
    tokio::pin!(connection_stream);

    while let Some(connection_result) = connection_stream.next().await {
        match connection_result {
            Ok(connection) => {
                // tokio::spawn(connection.run()); // Now spawned in new

                let hello = proto::hello::Hello {
                    role: proto::hello::Role::Collector,
                };
                let frame = Frame::Control(ControlMsg::Hello(hello));
                let serialized_frame = encode::to_vec(&frame)
                    .map_err(|e| anyhow::anyhow!("Failed to serialize hello: {}", e))?;
                if let Err(e) = connection.send_frame(serialized_frame).await {
                    log::warn!("Failed to send hello: {}", e);
                }
            }
            Err(e) => {
                log::warn!("Connection error: {}", e);
            }
        }
    }

    Ok(())
}
