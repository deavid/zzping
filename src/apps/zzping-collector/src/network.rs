use zznet_api::error::TransportError;
use zznet_api::transport::TransportClient as _; // bring connect() into scope
use zznet_hello::connection_manager::{ConnectionManager, HandleTransport};
use zznet_transport_tcp::client::TcpTransportClient;
use zznet_transport_tcp::config::TlsConfig;
use zzping_auth::AuthRole;

/// CollectorNetwork now holds a client and the ConnectionManager actor address.
pub struct CollectorNetwork {
    client: TcpTransportClient,
    connection_manager: actix::Addr<
        ConnectionManager<zzintent_config::network_messages::IntentConfigNetworkMsg, AuthRole>,
    >,
}

impl CollectorNetwork {
    /// Create a new CollectorNetwork.
    ///
    /// `addr` is the server address to connect to, `tls` is optional TLS configuration,
    /// `connection_manager` is the actix address of the ConnectionManager actor.
    pub fn new(
        addr: &str,
        tls: Option<TlsConfig>,
        connection_manager: actix::Addr<
            ConnectionManager<zzintent_config::network_messages::IntentConfigNetworkMsg, AuthRole>,
        >,
    ) -> Result<Self, TransportError> {
        let client = if let Some(cfg) = tls {
            TcpTransportClient::with_tls(addr.to_string(), cfg)?
        } else {
            TcpTransportClient::plain(addr.to_string())
        };

        Ok(Self {
            client,
            connection_manager,
        })
    }

    /// Connect to the server, hand the resulting transport to ConnectionManager and return.
    ///
    /// This function does not block the process; ConnectionManager owns the transport afterwards.
    pub async fn connect(&mut self) -> Result<(), TransportError> {
        let transport = self.client.connect().await?;

        // Build a simple HelloConfig - applications can extend if needed
        let config = zznet_hello::actor::HelloConfig::default();

        // Send transport to ConnectionManager actor which will spawn HelloActor and manage lifecycle
        self.connection_manager
            .try_send(HandleTransport { transport, config })
            .map_err(|e| {
                TransportError::IoError(format!(
                    "Failed to send transport to ConnectionManager: {}",
                    e
                ))
            })?;

        // If ConnectionManager takes ownership, we can return successfully. Keep process alive as needed by service.
        Ok(())
    }
}
