use crate::service::{DatabaseMessage, DatabaseRole};
use zznet_api::error::TransportError;
use zznet_api::transport::TransportServer as _;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_hello::connection_manager::HandleTransport;
use zznet_transport_tcp::config::TlsConfig;
use zznet_transport_tcp::server::TcpTransportServer;

pub struct DatabaseNetwork {
    server: TcpTransportServer,
    connection_manager: actix::Addr<ConnectionManager<DatabaseMessage, DatabaseRole>>,
}

impl DatabaseNetwork {
    pub async fn new(
        bind_addr: &str,
        tls: Option<TlsConfig>,
        connection_manager: actix::Addr<ConnectionManager<DatabaseMessage, DatabaseRole>>,
    ) -> Result<Self, TransportError> {
        let server = if let Some(cfg) = tls {
            TcpTransportServer::with_tls(bind_addr, cfg).await?
        } else {
            TcpTransportServer::plain(bind_addr).await?
        };

        Ok(Self {
            server,
            connection_manager,
        })
    }

    pub async fn run(&mut self) -> Result<(), TransportError> {
        loop {
            let transport = self.server.accept().await?;

            // Create HelloConfig for this connection - application should pass real config
            let config = zznet_hello::actor::HelloConfig::default();

            // Send transport to ConnectionManager actor
            let cm = self.connection_manager.clone();
            cm.try_send(HandleTransport { transport, config })
                .map_err(|e| {
                    TransportError::IoError(format!(
                        "Failed to send transport to ConnectionManager: {}",
                        e
                    ))
                })?;
        }
    }
}
