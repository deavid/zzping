use crate::service::DatabaseMessage;
use std::time::Duration;
use zznet_builder::ServerBuilder;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_transport_tcp::config::TlsConfig;
use zzping_auth::AuthRole;

/// DatabaseNetwork manages the server-side connection using ServerBuilder.
///
/// This abstracts away manual accept loop handling and provides actor-based connection lifecycle management.
pub struct DatabaseNetwork {
    bind_addr: String,
    tls_config: Option<TlsConfig>,
    connection_manager: actix::Addr<ConnectionManager<DatabaseMessage, AuthRole>>,
    handshake_timeout: Duration,
}

impl DatabaseNetwork {
    /// Create a new DatabaseNetwork that will use ServerBuilder for accepting connections.
    ///
    /// # Arguments
    /// * `bind_addr` - the address to bind to
    /// * `tls` - optional TLS configuration
    /// * `connection_manager` - the actix address of the ConnectionManager actor
    /// * `handshake_timeout` - timeout for the handshake protocol
    pub fn new(
        bind_addr: &str,
        tls: Option<TlsConfig>,
        connection_manager: actix::Addr<ConnectionManager<DatabaseMessage, AuthRole>>,
        handshake_timeout: Duration,
    ) -> Self {
        Self {
            bind_addr: bind_addr.to_string(),
            tls_config: tls,
            connection_manager,
            handshake_timeout,
        }
    }

    /// Start the server using ServerBuilder to accept incoming connections.
    ///
    /// This spawns a ServerActor that listens for connections and automatically
    /// spawns HelloActors for each accepted connection.
    pub async fn run(&self) -> Result<(), String> {
        let builder = ServerBuilder::<DatabaseMessage, AuthRole>::new()
            .bind(&self.bind_addr)
            .as_role(AuthRole::Database)
            .offer_rooms(vec!["intent-config".to_string()])
            .handshake_timeout(self.handshake_timeout);

        let builder = if let Some(tls) = &self.tls_config {
            builder.with_tls(tls.clone())
        } else {
            builder
        };

        // The start() method returns an Addr to the ServerActor, which we don't need
        // to hold onto - the actor is now running and managing connections automatically
        builder
            .with_connection_manager(self.connection_manager.clone())
            .start()
            .await
            .map(|_| ())
            .map_err(|e| format!("ServerBuilder failed: {:?}", e))
    }
}
