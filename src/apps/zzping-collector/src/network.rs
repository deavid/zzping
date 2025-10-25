use actix::Addr;
use std::time::Duration;
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::permissions::IntentConfigPermission;
use zznet_builder::ClientBuilder;
use zznet_transport_tcp::config::TlsConfig;
use zzping_auth::AuthRole;

/// CollectorNetwork manages the client-side connection using ClientBuilder.
///
/// This abstracts away manual connection handling and provides automatic reconnection.
pub struct CollectorNetwork {
    remote_addr: String,
    tls_config: Option<TlsConfig>,
    reconnect_delay: Duration,
}

impl CollectorNetwork {
    /// Create a new CollectorNetwork that will use ClientBuilder for connections.
    ///
    /// # Arguments
    /// * `addr` - the server address to connect to
    /// * `tls` - optional TLS configuration
    /// * `reconnect_delay` - delay between reconnection attempts
    pub fn new(addr: &str, tls: Option<TlsConfig>, reconnect_delay: Duration) -> Self {
        Self {
            remote_addr: addr.to_string(),
            tls_config: tls,
            reconnect_delay,
        }
    }

    /// Connect to the server using ClientBuilder with automatic reconnection.
    ///
    /// This spawns a ClientActor that manages the connection lifecycle,
    /// including automatic reconnection on failure. The room handler for
    /// intent-config is registered declaratively via the builder.
    ///
    /// # Arguments
    /// * `intent_addr` - Address of the IntentConfigActor for room handler wiring
    pub async fn connect(
        &self,
        intent_addr: &Addr<IntentConfigActor<IntentConfigPermission>>,
    ) -> Result<(), String> {
        // Integration tests expect a "Connecting to" log line.
        tracing::info!("Connecting to {}", self.remote_addr);

        // Create room handler factory for intent-config
        use std::sync::Arc as StdArc;
        let factory = StdArc::new(crate::room_handlers::IntentConfigRoomHandlerFactory::new(
            intent_addr.clone(),
        ));

        let builder = ClientBuilder::<AuthRole>::new()
            .connect_to(&self.remote_addr)
            .as_role(AuthRole::Collector)
            .offer_rooms(vec!["intent-config".to_string()])
            .reconnect_delay(self.reconnect_delay)
            // Allow plain-TCP connections (role will come from HELLO message)
            .with_default_authorizer(true)
            .register_room_handler("intent-config", factory);

        let builder = if let Some(tls) = &self.tls_config {
            builder.with_tls(tls.clone())
        } else {
            builder
        };

        builder
            .connect()
            .await
            .map(|_| ())
            .map_err(|e| format!("ClientBuilder failed: {:?}", e))
    }
}
