use crate::service::CollectorMessage;
use std::time::Duration;
use zznet_builder::ClientBuilder;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_transport_tcp::config::TlsConfig;
use zzping_auth::AuthRole;

/// CollectorNetwork manages the client-side connection using ClientBuilder.
///
/// This abstracts away manual connection handling and provides automatic reconnection.
pub struct CollectorNetwork {
    remote_addr: String,
    tls_config: Option<TlsConfig>,
    connection_manager: actix::Addr<ConnectionManager<CollectorMessage, AuthRole>>,
    reconnect_delay: Duration,
}

impl CollectorNetwork {
    /// Create a new CollectorNetwork that will use ClientBuilder for connections.
    ///
    /// # Arguments
    /// * `addr` - the server address to connect to
    /// * `tls` - optional TLS configuration
    /// * `connection_manager` - the actix address of the ConnectionManager actor
    /// * `reconnect_delay` - delay between reconnection attempts
    pub fn new(
        addr: &str,
        tls: Option<TlsConfig>,
        connection_manager: actix::Addr<ConnectionManager<CollectorMessage, AuthRole>>,
        reconnect_delay: Duration,
    ) -> Self {
        Self {
            remote_addr: addr.to_string(),
            tls_config: tls,
            connection_manager,
            reconnect_delay,
        }
    }

    /// Connect to the server using ClientBuilder with automatic reconnection.
    ///
    /// This spawns a ClientActor that manages the connection lifecycle,
    /// including automatic reconnection on failure.
    pub async fn connect(&self) -> Result<(), String> {
        let builder = ClientBuilder::<CollectorMessage, AuthRole>::new()
            .connect_to(&self.remote_addr)
            .as_role(AuthRole::Collector)
            .offer_rooms(vec!["intent-config".to_string()])
            .reconnect_delay(self.reconnect_delay);

        let builder = if let Some(tls) = &self.tls_config {
            builder.with_tls(tls.clone())
        } else {
            builder
        };

        // The connect() method returns an Addr to the ClientActor, which we don't need
        // to hold onto - the actor is now running and managing the connection automatically
        builder
            .with_connection_manager(self.connection_manager.clone())
            .connect()
            .await
            .map(|_| ())
            .map_err(|e| format!("ClientBuilder failed: {:?}", e))
    }
}
