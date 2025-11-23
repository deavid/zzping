//! Provides the public builder for creating and starting the IntentConfigActor.

use crate::actor::IntentConfigActor;
use crate::config::IntentConfigConfig;
use crate::permissions::IntentConfigPermissions;
use actix::prelude::*;
use std::collections::HashMap;
use zznet_router::RouterActor;

/// A builder for the IntentConfig component.
///
/// This is the primary public entry point for creating the actor.
/// It follows the 'Builder -> Start' pattern, ensuring that the actor
/// is constructed and started in a controlled manner.
pub struct IntentConfigBuilder {
    config: IntentConfigConfig,
    router_actor: Option<Addr<RouterActor>>,
    permissions_map: Option<HashMap<String, IntentConfigPermissions>>,
}

impl IntentConfigBuilder {
    /// Create a new builder with default configuration (collector role).
    pub fn new() -> Self {
        Self {
            config: IntentConfigConfig::default(),
            router_actor: None,
            permissions_map: None,
        }
    }
}

impl Default for IntentConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentConfigBuilder {
    /// Set the configuration for this IntentConfig actor.
    ///
    /// Use `IntentConfigConfig::for_database()` or `IntentConfigConfig::for_collector()`
    /// for common configurations.
    pub fn config(mut self, config: IntentConfigConfig) -> Self {
        self.config = config;
        self
    }

    /// Convenience: configure for database role.
    pub fn config_for_database(mut self, config_file_path: std::path::PathBuf) -> Self {
        self.config = IntentConfigConfig::for_database(config_file_path);
        self
    }

    /// Convenience: configure for collector role.
    pub fn config_for_collector(mut self) -> Self {
        self.config = IntentConfigConfig::for_collector();
        self
    }

    /// Set the RouterActor for data-plane operations.
    ///
    /// This is required for the GenericNetworkManager to communicate
    /// with peers for configuration updates.
    pub fn router(mut self, router_actor: Addr<RouterActor>) -> Self {
        self.router_actor = Some(router_actor);
        self
    }

    /// Set the permissions map for role-to-permissions translation.
    ///
    /// This maps role strings to the specific permissions that peers with
    /// those roles should have within this component. If not set, the
    /// NetworkManager will deny all peer connections.
    pub fn permissions_map(
        mut self,
        permissions_map: HashMap<String, IntentConfigPermissions>,
    ) -> Self {
        self.permissions_map = Some(permissions_map);
        self
    }

    /// Get the current configuration
    pub fn get_config(&self) -> &IntentConfigConfig {
        &self.config
    }

    /// Get the permissions map (for testing)
    pub fn get_permissions_map(&self) -> Option<&HashMap<String, IntentConfigPermissions>> {
        self.permissions_map.as_ref()
    }

    /// Starts the IntentConfigActor and returns its address (`Addr`).
    ///
    /// This method consumes the builder (`self`) to ensure it can only be called once.
    /// It internally creates the `IntentConfigActor` and starts it on the
    /// currently running Actix System.
    ///
    /// Builds the three-actor system:
    /// - IntentConfigActor (Main Actor - business logic)
    /// - GenericNetworkManager (Manager Actor - peer lifecycle, using generic machinery)
    /// - IntentConfigNetworkActor (Per-peer translator, created by Manager)
    ///
    /// The GenericNetworkManager owns the RoomActor<T> instances that serialize messages
    /// for each peer and wires them to the translators.
    ///
    /// # Errors
    ///
    /// Returns an error if role validation fails (e.g., Collector with empty file path).
    ///
    /// # Returns
    ///
    /// The returned `Addr` is the handle to the running actor, used for sending messages.
    pub fn start(mut self) -> anyhow::Result<Addr<IntentConfigActor>> {
        // Validate configuration
        self.config
            .validate()
            .map_err(|e| anyhow::anyhow!("Invalid config: {}", e))?;

        // Create and start actor with config
        let actor = IntentConfigActor::new(self.config.clone());
        log::info!(
            "Starting IntentConfigActor via builder. persist_config={}",
            actor.get_config().persist_config
        );

        let event_bus = actor.event_bus();
        let actor_addr = actor.start();

        if let Some(router_actor) = self.router_actor.take() {
            log::info!("Creating GenericNetworkManager for three-actor pattern");

            let permissions_map = self.permissions_map.take().unwrap_or_else(|| {
                log::warn!("No permissions_map provided - all peer connections will be denied");
                HashMap::new()
            });

            let _network_manager = zznet_component::GenericNetworkManager::<
                crate::spec::IntentConfigSpec,
            >::new(
                actor_addr.clone(), router_actor, event_bus, permissions_map
            )
            .start();

            log::info!("✓ Three-actor system initialized (MainActor + GenericNetworkManager)");
        } else {
            log::debug!("No Router - NetworkManager not created (standalone mode)");
        }

        Ok(actor_addr)
    }
}
