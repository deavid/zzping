use std::sync::Arc;
use zznet_api::error::TransportError;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_transport_tcp::client::TcpTransportClient;
use zznet_transport_tcp::config::TlsConfig;

pub struct CollectorNetwork {
    client: TcpTransportClient,
    // connection_manager: ConnectionManager<...>,
}

impl CollectorNetwork {
    pub fn new(addr: &str, tls: Option<TlsConfig>) -> Result<Self, TransportError> {
        let client = if let Some(cfg) = tls {
            TcpTransportClient::with_tls(addr.to_string(), cfg)?
        } else {
            TcpTransportClient::plain(addr.to_string())
        };

        Ok(Self { client })
    }

    pub async fn connect(&mut self) -> Result<(), TransportError> {
        let mut transport = self.client.connect().await?;
        // For now, just keep the connection alive; ConnectionManager should handle handshake
        loop {
            match transport.recv().await {
                Ok(Some(_)) => {
                    // received frames - in future forward to ConnectionManager
                }
                Ok(None) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }
}
