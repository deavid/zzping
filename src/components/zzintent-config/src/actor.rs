//! Contains the private implementation of the IntentConfigActor, including its
//! state and message handling logic.

use crate::events::IntentConfigEvent;
use crate::messages::{
    GetCurrentConfig, GetHealth, IntentConfigData, IntentConfigHealth, Subscribe, Unsubscribe,
    UpdateConfig,
};
use actix::prelude::*;
use std::collections::HashMap;

/// The IntentConfigActor stores the current configuration and manages subscribers.
/// This struct is the private state of our component.
pub struct IntentConfigActor {
    current_config: IntentConfigData,
    subscribers: HashMap<usize, Recipient<IntentConfigData>>,
    next_id: usize,

    /// Component configuration (what capabilities are enabled)
    config: crate::config::IntentConfigConfig,

    /// Event bus for broadcasting config changes to all NetworkActors
    event_tx: tokio::sync::broadcast::Sender<IntentConfigEvent>,
}

impl Default for IntentConfigActor {
    fn default() -> Self {
        Self::new(crate::config::IntentConfigConfig::default())
    }
}

impl IntentConfigActor {
    /// Create a new IntentConfigActor from a config
    pub fn new(config: crate::config::IntentConfigConfig) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            current_config: IntentConfigData::default(),
            subscribers: HashMap::new(),
            next_id: 0,
            config,
            event_tx,
        }
    }

    /// Get the component configuration
    pub fn get_config(&self) -> &crate::config::IntentConfigConfig {
        &self.config
    }

    /// Get the event bus sender for subscribing NetworkActors
    pub fn event_bus(&self) -> tokio::sync::broadcast::Sender<IntentConfigEvent> {
        self.event_tx.clone()
    }

    /// The logic to broadcast the current configuration to all subscribers.
    fn broadcast_config(&mut self) {
        // Local synchronous broadcasts to registered subscribers.
        let mut sent = 0u64;
        for (id, recipient) in &self.subscribers {
            log::info!("Broadcasting update to subscriber {}", id);
            // `do_send` is a "tell" or fire-and-forget send. It does not wait for a response.
            recipient.do_send(self.current_config.clone());
            sent += 1;
        }
        log::debug!("Broadcast completed to {} subscribers", sent);
    }

    /// Send ConfigUpdate to all connected peers via network (Database role only)
    /// Convenience method for types that implement PermissionCheck
    fn send_config_update_to_peers(&self, ctx: &mut Context<Self>) {
        self.send_config_update_to_peers_impl(ctx);
    }

    /// Send ConfigUpdate to all connected peers via event bus (Database role only)
    fn send_config_update_to_peers_impl(&self, _ctx: &mut Context<Self>) {
        // Only Database role should send ConfigUpdate (check persist_config)
        if !self.config.persist_config {
            return;
        }

        log::debug!("Publishing config change event");
        let _ = self.event_tx.send(IntentConfigEvent::ConfigChanged(
            self.current_config.clone(),
        ));
    }

    /// Persist the current configuration to disk (Database role only)
    fn persist_config(&self) -> Result<(), std::io::Error> {
        let config_path = match &self.config.config_file_path {
            Some(path) => path,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Collector role cannot persist config",
                ));
            }
        };

        log::info!("Persisting config to {:?}", config_path);

        let config_string = ron::ser::to_string_pretty(&self.current_config, Default::default())
            .map_err(std::io::Error::other)?;

        let temp_path = config_path.with_extension("tmp");
        std::fs::write(&temp_path, config_string)?;
        std::fs::rename(&temp_path, config_path)?;

        log::info!("Config successfully persisted");
        Ok(())
    }
}

/// This is the boilerplate that officially makes the struct an Actix Actor.
impl Actor for IntentConfigActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Context<Self>) {
        log::info!("IntentConfigActor has started.");

        // On startup as Database: attempt to load existing config; regardless of
        // load outcome, ensure a canonical `intent.ron` exists by persisting the
        // current_config (loaded or default) to disk. This guarantees that a
        // DB started with no file will create it, and a DB started with an
        // existing file will overwrite it with validated/canonical RON.
        if let Some(config_path) = &self.config.config_file_path.clone() {
            let mut loaded_cfg: Option<IntentConfigData> = None;

            if config_path.exists() {
                match std::fs::read_to_string(config_path) {
                    Ok(s) => match ron::de::from_str::<IntentConfigData>(&s) {
                        Ok(cfg) => match cfg.validate() {
                            Ok(()) => {
                                log::info!(
                                    "Loaded and validated persisted IntentConfig from {}",
                                    config_path.display()
                                );
                                // Log the concrete values we parsed so it's obvious
                                // what the database will use (targets + rate)
                                log::info!(
                                    "Parsed IntentConfig: targets={:?}, ping_rate_pps={} ",
                                    cfg.targets,
                                    cfg.ping_rate_pps
                                );
                                loaded_cfg = Some(cfg);
                            }
                            Err(validation_error) => {
                                log::error!(
                                    "Loaded config from {} is invalid: {}. Using default config.",
                                    config_path.display(),
                                    validation_error
                                );
                            }
                        },
                        Err(e) => log::warn!("Failed to parse persisted IntentConfig: {}", e),
                    },
                    Err(e) => log::warn!("Failed to read persisted IntentConfig file: {}", e),
                }
            } else {
                log::info!(
                    "No existing IntentConfig file at {}, will create default.",
                    config_path.display()
                );
            }

            if let Some(cfg) = loaded_cfg {
                self.current_config = cfg.clone();
            }

            self.broadcast_config();

            // Always attempt to persist the current (canonical) config to disk.
            // Log errors but do not prevent the actor from starting.
            if let Err(e) = self.persist_config() {
                log::error!(
                    "Failed to persist IntentConfig to {}: {}",
                    config_path.display(),
                    e
                );
            } else {
                // If persist succeeded, log the canonical config we wrote.
                log::info!(
                    "Startup persisted canonical IntentConfig: targets={:?}, ping_rate_pps={}",
                    self.current_config.targets,
                    self.current_config.ping_rate_pps
                );
            }
        }
        // Collector role should NOT proactively query peers on startup. Instead,
        // Database actors are responsible for sending ConfigUpdate/CurrentConfig
        // when their rooms become available. This avoids unnecessary traffic and
        // relies on the database to push state when it has joined rooms.
    }
}

// --- Handler Implementations (The Business Logic) ---

/// Handles the `UpdateConfig` message.
impl Handler<UpdateConfig> for IntentConfigActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateConfig, ctx: &mut Context<Self>) -> Self::Result {
        eprintln!("⚙️ UpdateConfig handler called!");
        log::info!(
            "Handling UpdateConfig message from peer {:?}: {:?}",
            msg.peer_id,
            msg.data
        );
        if msg.data != self.current_config {
            self.current_config = msg.data.clone();

            // Broadcast locally to subscribers
            self.broadcast_config();

            // Send to network peers via network layer
            self.send_config_update_to_peers(ctx);
            log::debug!("Config updated locally and sent to network peers");
        }
    }
}

/// Handles the `Subscribe` message.
impl Handler<Subscribe> for IntentConfigActor {
    type Result = usize; // Returns the subscription ID

    fn handle(&mut self, msg: Subscribe, _ctx: &mut Context<Self>) -> Self::Result {
        let id = self.next_id;
        self.next_id += 1;
        log::info!("Adding new subscriber with ID: {}", id);

        // Immediately send the current state to the new subscriber so it's up-to-date.
        msg.recipient.do_send(self.current_config.clone());

        self.subscribers.insert(id, msg.recipient);
        id
    }
}

/// Handles the `Unsubscribe` message.
impl Handler<Unsubscribe> for IntentConfigActor {
    type Result = ();

    fn handle(&mut self, msg: Unsubscribe, _ctx: &mut Context<Self>) {
        log::info!("Removing subscriber with ID: {}", msg.0);
        self.subscribers.remove(&msg.0);
    }
}

/// Handles the `GetCurrentConfig` message.
impl Handler<GetCurrentConfig> for IntentConfigActor {
    type Result = MessageResult<GetCurrentConfig>;

    fn handle(&mut self, _msg: GetCurrentConfig, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!("Returning current config state");
        MessageResult(self.current_config.clone())
    }
}

/// Handles the `GetEventBus` message
impl Handler<crate::messages::GetEventBus> for IntentConfigActor {
    type Result = MessageResult<crate::messages::GetEventBus>;

    fn handle(
        &mut self,
        _msg: crate::messages::GetEventBus,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        MessageResult(self.event_tx.clone())
    }
}

/// Handler for NetworkConfigChangeRequest from NetworkManager
///
/// This message is sent by NetworkManager after it has verified the peer's
/// authorization to change the configuration. The MainActor is responsible for:
/// 1. Validating the new configuration
/// 2. Persisting to disk (Database role only)
/// 3. Broadcasting to local subscribers
/// 4. Requesting network broadcast via NetworkManager
impl Handler<crate::internal_messages::NetworkConfigChangeRequest> for IntentConfigActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        msg: crate::internal_messages::NetworkConfigChangeRequest,
        ctx: &mut Context<Self>,
    ) -> Self::Result {
        if !msg.authorized {
            return Err(format!(
                "Unauthorized config change request from peer {}",
                msg.peer_id
            ));
        }

        log::info!(
            "Processing authorized config change from peer {}: targets={:?}, rate={}",
            msg.peer_id,
            msg.targets,
            msg.ping_rate_pps
        );

        // Create new config
        let new_config = crate::messages::IntentConfigData {
            targets: msg.targets,
            ping_rate_pps: msg.ping_rate_pps,
        };

        // Validate config
        if let Err(e) = new_config.validate() {
            let error = format!("Invalid configuration: {}", e);
            log::error!("{}", error);
            return Err(error);
        }

        // Check if config actually changed
        if new_config == self.current_config {
            log::info!("Config unchanged, no action needed");
            return Ok(());
        }

        // Update current config
        self.current_config = new_config;

        // Persist config (Database role only)
        if let Err(e) = self.persist_config() {
            let error = format!("Failed to persist config: {}", e);
            log::error!("{}", error);
            return Err(error);
        }

        // Broadcast to local subscribers
        self.broadcast_config();

        // Broadcast to network peers (Database role only)
        self.send_config_update_to_peers(ctx);

        log::info!("Config change applied successfully");
        Ok(())
    }
}

/// Handles the `GetHealth` message.
impl Handler<GetHealth> for IntentConfigActor {
    type Result = MessageResult<GetHealth>;

    fn handle(&mut self, _msg: GetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        // Broadcast metrics removed during Room<T> migration
        // Only subscriber count is currently tracked
        let health = IntentConfigHealth {
            subscriber_count: self.subscribers.len(),
            successful_broadcasts: 0,
            failed_broadcasts: 0,
            last_broadcast_ms: 0,
        };
        MessageResult(health)
    }
}
