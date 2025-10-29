//! Provides the public builder for creating and starting the IntentConfigActor.

use crate::actor::IntentConfigActor;
use crate::config::IntentConfigConfig;
use actix::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use zznet_api::{MessageRouter, PeerRegistry};

/// A builder for the IntentConfig component.
///
/// This is the primary public entry point for creating the actor.
/// It follows the 'Builder -> Start' pattern, ensuring that the actor
/// is constructed and started in a controlled manner.
pub struct IntentConfigBuilder {
    config: IntentConfigConfig,
    peer_registry: Option<Arc<dyn PeerRegistry>>,
    message_router: Option<Arc<dyn MessageRouter>>,
    /// Per-peer broadcast timeout used when sending messages via PeerManager
    broadcast_timeout: Duration,
}

impl IntentConfigBuilder {
    /// Create a new builder with default configuration (collector role).
    pub fn new() -> Self {
        Self {
            config: IntentConfigConfig::default(),
            peer_registry: None,
            message_router: None,
            broadcast_timeout: Duration::from_millis(500),
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

    /// Set the PeerRegistry for network operations (Phase 7.2)
    pub fn peer_manager(mut self, peer_registry: Arc<dyn PeerRegistry>) -> Self {
        self.peer_registry = Some(peer_registry);
        self
    }

    /// Set the MessageRouter for message routing (Phase 7.2)
    pub fn router(mut self, message_router: Arc<dyn MessageRouter>) -> Self {
        self.message_router = Some(message_router);
        self
    }

    /// Set the per-peer broadcast timeout used when sending network messages.
    /// Default is 500ms.
    pub fn broadcast_timeout(mut self, timeout: Duration) -> Self {
        self.broadcast_timeout = timeout;
        self
    }

    /// Get the current configuration
    pub fn get_config(&self) -> &IntentConfigConfig {
        &self.config
    }

    /// Starts the IntentConfigActor and returns its address (`Addr`).
    ///
    /// This method consumes the builder (`self`) to ensure it can only be called once.
    /// It internally creates the `IntentConfigActor` and starts it on the
    /// currently running Actix System.
    ///
    /// Phase 7.2: Now creates three-actor system:
    /// - IntentConfigActor (Main Actor - business logic)
    /// - IntentConfigNetworkManager (Manager Actor - peer lifecycle)
    /// - IntentConfigNetworkActor (Network Actor - per-peer, created by Manager)
    ///
    /// NetworkManager uses PeerManagerActor directly for authorization and channel access.
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

        // Start the main actor first (needed for NetworkManager creation)
        let actor_addr = actor.start();

        // Phase 7.2: Create NetworkManager if we have PeerRegistry and MessageRouter
        if let (Some(peer_registry), Some(message_router)) =
            (self.peer_registry.take(), self.message_router.take())
        {
            log::info!("Creating IntentConfigNetworkManager for three-actor pattern");

            // Create a dummy broadcast receiver for PeerLifecycleEvents
            // This will be replaced when PeerManager provides subscribe_events()
            let (_tx, rx) = tokio::sync::broadcast::channel(100);

            let network_manager = crate::network_manager::IntentConfigNetworkManager::new(
                actor_addr.clone(),
                rx,
                peer_registry,
                message_router,
            )
            .start();

            // Wire NetworkManager back to MainActor
            actor_addr.do_send(crate::internal_messages::SetNetworkManager {
                network_manager: network_manager.clone(),
            });

            log::info!("✓ Three-actor system initialized (MainActor + NetworkManager)");
        } else {
            log::debug!(
                "No PeerRegistry/MessageRouter - NetworkManager not created (standalone mode)"
            );
        }

        Ok(actor_addr)
    }
}
