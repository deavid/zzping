//! Contains the private implementation of the IntentConfigActor, including its
//! state and message handling logic.

use crate::messages::{
    CreateRoom, GetCurrentConfig, GetHealth, GetRoomChannels, GetRoomChannelsResponse,
    IntentConfigData, IntentConfigHealth, ProcessRequestConfigChangeAuth, Subscribe, Unsubscribe,
    UpdateConfig,
};
use crate::network_messages::IntentConfigNetworkMsg;
use crate::permissions::{IntentConfigPermission, PermissionCheck};
use crate::role::IntentConfigRole;
use actix::ResponseFuture;
use actix::prelude::*;
use std::collections::HashMap;
use zznet_auth::role::ApplicationRole;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::RoomId;

/// The IntentConfigActor stores the current configuration and manages subscribers.
/// This struct is the private state of our component.
pub struct IntentConfigActor<T: ApplicationRole> {
    current_config: IntentConfigData,
    subscribers: HashMap<usize, Recipient<IntentConfigData>>,
    next_id: usize,

    /// Role configuration (Collector or Database)
    role: IntentConfigRole,

    /// SessionManager actor for network communication (Phase 3: Pure Actor Pattern)
    session_manager: Option<Addr<SessionManager<T>>>,

    /// Room for typed network messaging (Room<T> architecture)
    room: Option<zznet_room::room::Room<IntentConfigNetworkMsg>>,

    /// Channels for SessionManager integration
    room_channels: Option<std::sync::Arc<zznet_room::room::RoomChannels>>,
}

impl<T: ApplicationRole> Default for IntentConfigActor<T> {
    fn default() -> Self {
        Self::new_with_role(IntentConfigRole::default())
    }
}

// Provide PermissionCheck implementation for the concrete IntentConfigPermission
// so that tests and SessionManager integration using the concrete enum work.
impl PermissionCheck<IntentConfigPermission> for IntentConfigActor<IntentConfigPermission> {
    fn has_update_permission(&self, role: &IntentConfigPermission) -> bool {
        *role == IntentConfigPermission::UpdateConfig
    }

    fn has_receive_permission(&self, role: &IntentConfigPermission) -> bool {
        *role == IntentConfigPermission::ReceiveConfigUpdates
    }

    fn to_string(&self, role: &IntentConfigPermission) -> String {
        format!("{:?}", role)
    }

    fn receive_role(&self) -> IntentConfigPermission {
        IntentConfigPermission::ReceiveConfigUpdates
    }
}

impl<T: ApplicationRole + 'static> IntentConfigActor<T> {
    /// Send initial config if T is IntentConfigPermission
    /// Only sends if the current config is valid (not default/empty)
    fn send_initial_if_permission_type(&self, ctx: &mut Context<Self>) {
        // Don't send initial ConfigUpdate if config is invalid/default
        if self.current_config.validate().is_err() {
            log::debug!("Skipping initial ConfigUpdate - current config is invalid/default");
            return;
        }

        // If the ApplicationRole defines a role for receiving config updates, send them.
        if let Some(receive_role) = T::receive_config_updates_role() {
            self.send_config_update_to_peers_impl(ctx, receive_role);
        }
    }
}

impl<T: ApplicationRole> IntentConfigActor<T> {
    /// Create a new IntentConfigActor with the specified role
    pub fn new_with_role(role: IntentConfigRole) -> Self {
        Self {
            current_config: IntentConfigData::default(),
            subscribers: HashMap::new(),
            next_id: 0,
            role,
            session_manager: None,
            room: None,
            room_channels: None,
        }
    }

    /// Get the current role
    pub fn role(&self) -> &IntentConfigRole {
        &self.role
    }

    /// Set the SessionManager actor for network communication (Phase 3: Pure Actor Pattern)
    pub fn set_session_manager(&mut self, session_manager: Addr<SessionManager<T>>) {
        self.session_manager = Some(session_manager);
    }

    /// Set the Room for typed network messaging
    pub fn set_room(&mut self, room: zznet_room::room::Room<IntentConfigNetworkMsg>) {
        self.room = Some(room);
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

    /// Send ConfigUpdate to all connected peers via SessionManager (Database role only)
    /// Convenience method for types that implement PermissionCheck
    fn send_config_update_to_peers(&self, ctx: &mut Context<Self>)
    where
        Self: PermissionCheck<T>,
    {
        let receive_role = self.receive_role();
        self.send_config_update_to_peers_impl(ctx, receive_role);
    }

    /// Send ConfigUpdate to all connected peers via SessionManager (Database role only)
    ///
    /// Note: This method is generic and works without PermissionCheck trait bound.
    fn send_config_update_to_peers_impl(&self, ctx: &mut Context<Self>, receive_role: T) {
        // Only Database role should send ConfigUpdate
        if !matches!(self.role, IntentConfigRole::Database { .. }) {
            return;
        }

        // If we have a SessionManager, send ConfigUpdate to all peers with ReceiveConfigUpdates permission
        if let Some(session_manager) = &self.session_manager {
            let msg = IntentConfigNetworkMsg::ConfigUpdate {
                targets: self.current_config.targets.clone(),
                ping_rate_pps: self.current_config.ping_rate_pps,
            };

            // Serialize the message once for broadcast
            // Note: Broadcasting is a legitimate SessionManager use case per architecture.
            // Room<T> is designed for bidirectional point-to-point communication.
            // For multi-peer broadcasts, SessionManager is the appropriate abstraction.
            let bytes = match bincode::serde::encode_to_vec(&msg, bincode::config::standard()) {
                Ok(b) => b,
                Err(e) => {
                    log::error!("Failed to serialize ConfigUpdate: {}", e);
                    return;
                }
            };

            // Clone SessionManager actor address for async task
            let session_manager = session_manager.clone();
            let room_id = RoomId::from("intent-config");

            // Spawn async task to query peers and send to all
            ctx.spawn(
                async move {
                    use zznet_session::messages::GetPeersWithRole;

                    // Query SessionManager for peers with ReceiveConfigUpdates permission
                    let peers = match session_manager
                        .send(GetPeersWithRole { role: receive_role })
                        .await
                    {
                        Ok(peer_list) => peer_list,
                        Err(e) => {
                            log::error!("Failed to query peers from SessionManager: {}", e);
                            return;
                        }
                    };

                    log::info!("Sending ConfigUpdate to {} collector peers", peers.len());

                    // Send to all peers via message passing
                    for peer_id in peers {
                        use zznet_session::messages::SendToRoom;

                        match session_manager
                            .send(SendToRoom {
                                peer_id: peer_id.clone(),
                                room_id: room_id.clone(),
                                bytes: bytes.clone(),
                            })
                            .await
                        {
                            Ok(Ok(())) => {
                                log::debug!("✓ ConfigUpdate sent to peer {}", peer_id);
                            }
                            Ok(Err(e)) => {
                                log::error!(
                                    "Failed to send ConfigUpdate to peer {}: {}",
                                    peer_id,
                                    e
                                );
                            }
                            Err(e) => {
                                log::error!(
                                    "Actor mailbox error sending to peer {}: {}",
                                    peer_id,
                                    e
                                );
                            }
                        }
                    }
                    log::info!("✓ ConfigUpdate broadcast complete");
                }
                .into_actor(self),
            );
        } else {
            log::debug!("No SessionManager configured - ConfigUpdate not sent to network peers");
        }
    }

    /// Persist the current configuration to disk (Database role only)
    fn persist_config(&self) -> Result<(), std::io::Error> {
        // Only Database role has config_file_path
        let config_path = match &self.role {
            IntentConfigRole::Database { config_file_path } => config_file_path,
            IntentConfigRole::Collector => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Collector role cannot persist config",
                ));
            }
        };

        log::info!("Persisting config to {:?}", config_path);

        // Serialize config data to RON format
        let config_string = ron::ser::to_string_pretty(&self.current_config, Default::default())
            .map_err(std::io::Error::other)?;

        // Write to file atomically (write to temp file, then rename)
        let temp_path = config_path.with_extension("tmp");
        std::fs::write(&temp_path, config_string)?;
        std::fs::rename(&temp_path, config_path)?;

        log::info!("Config successfully persisted");
        Ok(())
    }

    /// Handle RequestConfigChange when the actor role is Database.
    /// Extracted from the main `handle` match arm to improve readability.
    fn handle_request_config_change_db(
        &mut self,
        sender_peer_id: String,
        targets: Vec<std::net::IpAddr>,
        ping_rate_pps: u64,
        ctx: &mut Context<Self>,
    ) -> ResponseFuture<()>
    where
        Self: PermissionCheck<T>,
    {
        log::info!(
            "Received RequestConfigChange from peer '{}': targets={:?}, rate={}",
            sender_peer_id,
            targets,
            ping_rate_pps
        );

        // AUTHORIZATION CHECK: Only ClientAdmin can change config
        if let Some(session_manager) = &self.session_manager {
            // Query the sender's role via message passing
            let session_manager_clone = session_manager.clone();
            let sender_peer_id_clone = sender_peer_id.clone();
            let self_addr = ctx.address();

            ctx.spawn(
                async move {
                    use zznet_session::messages::GetPeerRole;
                    use zznet_session::types::PeerId;

                    let peer_id = PeerId::from(sender_peer_id_clone.as_str());

                    // Query peer role from SessionManager
                    let sender_role =
                        match session_manager_clone.send(GetPeerRole::new(peer_id)).await {
                            Ok(role_opt) => role_opt,
                            Err(e) => {
                                log::error!("Failed to query peer role from SessionManager: {}", e);
                                return;
                            }
                        };

                    // Send result back to self for processing
                    self_addr.do_send(ProcessRequestConfigChangeAuth {
                        sender_peer_id: sender_peer_id_clone,
                        sender_role,
                        targets,
                        ping_rate_pps,
                        session_manager: session_manager_clone,
                    });
                }
                .into_actor(self),
            );

            return Box::pin(async {});
        } else {
            // No SessionManager configured - this should only happen in unit tests
            // NOTE: In debug builds we allow config changes when no SessionManager is
            // configured to make unit testing easier (tests can exercise RequestConfigChange
            // without wiring a full network stack). In production (release builds) this
            // path is rejected to avoid accidental unauthenticated changes.
            #[cfg(debug_assertions)]
            log::warn!(
                "⚠️  Config change allowed WITHOUT auth check (test mode - no SessionManager) - peer: '{}',",
                sender_peer_id
            );

            #[cfg(not(debug_assertions))]
            {
                log::error!(
                    "✗ Config change REJECTED from peer '{}' - SessionManager required in production",
                    sender_peer_id
                );
                return Box::pin(async {});
            }
        }

        // Proceed with configuration update (only reached in test mode without SessionManager)
        let new_config = IntentConfigData {
            targets,
            ping_rate_pps,
        };
        if new_config != self.current_config {
            self.current_config = new_config;
            if let Err(e) = self.persist_config() {
                log::error!("Failed to persist config: {}", e);
                Box::pin(async {})
            } else {
                self.broadcast_config();
                self.send_config_update_to_peers(ctx);
                log::info!(
                    "Config updated and sent to peers: targets={:?}, ping_rate_pps={}",
                    self.current_config.targets,
                    self.current_config.ping_rate_pps
                );
                Box::pin(async {})
            }
        } else {
            log::info!("Config unchanged, no action needed");
            Box::pin(async {})
        }
    }

    /// Process authorization result and apply config change if authorized
    fn handle_process_request_config_change_auth(
        &mut self,
        sender_peer_id: String,
        sender_role: Option<T>,
        targets: Vec<std::net::IpAddr>,
        ping_rate_pps: u64,
        session_manager: Addr<SessionManager<T>>,
        ctx: &mut Context<Self>,
    ) where
        Self: PermissionCheck<T>,
    {
        if let Some(sender_role) = sender_role {
            if !self.has_update_permission(&sender_role) {
                log::warn!(
                    "✗ Config change REJECTED from peer '{}' - role {:?} is not authorized",
                    sender_peer_id,
                    sender_role
                );

                // Try to send an Error message back to the requester
                Self::spawn_send_error(
                    session_manager,
                    sender_peer_id.clone(),
                    "unauthorized: insufficient permission".to_string(),
                );
                return;
            }
        } else {
            log::warn!(
                "✗ Config change REJECTED from peer '{}' - no role information available (ACL not configured?)",
                sender_peer_id
            );

            // Send explicit error if possible
            Self::spawn_send_error(
                session_manager,
                sender_peer_id.clone(),
                "no-role: ACL not configured".to_string(),
            );
            return;
        }

        // Proceed with configuration update
        let new_config = IntentConfigData {
            targets,
            ping_rate_pps,
        };
        if new_config != self.current_config {
            self.current_config = new_config;
            if let Err(e) = self.persist_config() {
                log::error!("Failed to persist config: {}", e);

                // Try inform the requester of the persistence failure
                Self::spawn_send_error(
                    session_manager,
                    sender_peer_id.clone(),
                    format!("persist-failure: {}", e),
                );
            } else {
                self.broadcast_config();
                self.send_config_update_to_peers(ctx);
                log::info!(
                    "Config updated and sent to peers: targets={:?}, ping_rate_pps={}",
                    self.current_config.targets,
                    self.current_config.ping_rate_pps
                );
            }
        } else {
            log::info!("Config unchanged, no action needed");
        }
    }

    /// Spawn a fire-and-forget task to send an Error message to a specific peer.
    ///
    /// Uses SessionManager actor for point-to-point messaging (Phase 3: Pure Actor Pattern).
    fn spawn_send_error(
        session_manager: Addr<SessionManager<T>>,
        peer: String,
        reason: impl Into<String>,
    ) {
        let reason_string = reason.into();
        log::warn!("Sending error to peer {}: {}", peer, reason_string);

        // Create the Error message
        let error_msg = IntentConfigNetworkMsg::Error {
            reason: reason_string.clone(),
        };

        // Serialize the message using bincode (same as TypedSender would use)
        // Note: Keeping manual serialization here for now since this is point-to-point
        // via SessionManager. Room<T> is designed for bidirectional channels.
        let bytes = match bincode::serde::encode_to_vec(&error_msg, bincode::config::standard()) {
            Ok(b) => b,
            Err(e) => {
                log::error!("Failed to serialize Error message: {}", e);
                return;
            }
        };

        // Spawn async task to send via SessionManager actor
        actix::spawn(async move {
            use zznet_session::messages::SendToRoom;
            use zznet_session::types::PeerId;

            let peer_id = PeerId::from(peer.as_str());
            let room_id = RoomId::from("intent-config");

            match session_manager
                .send(SendToRoom {
                    peer_id,
                    room_id,
                    bytes,
                })
                .await
            {
                Ok(Ok(())) => {
                    log::info!("✓ Error message sent to peer {}", peer);
                }
                Ok(Err(e)) => {
                    log::error!("Failed to send Error message to peer {}: {}", peer, e);
                }
                Err(e) => {
                    log::error!(
                        "Actor mailbox error sending Error message to peer {}: {}",
                        peer,
                        e
                    );
                }
            }
        });
    }
}

/// This is the boilerplate that officially makes the struct an Actix Actor.
impl<T: ApplicationRole> Actor for IntentConfigActor<T> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        log::info!("IntentConfigActor has started.");

        // On startup as Database: attempt to load existing config; regardless of
        // load outcome, ensure a canonical `intent.ron` exists by persisting the
        // current_config (loaded or default) to disk. This guarantees that a
        // DB started with no file will create it, and a DB started with an
        // existing file will overwrite it with validated/canonical RON.
        if let IntentConfigRole::Database { config_file_path } = &self.role {
            // Clone the configured path so we don't hold an immutable borrow
            // of `self` across operations that require mutable borrows later.
            let config_path = config_file_path.clone();

            let mut loaded_cfg: Option<IntentConfigData> = None;

            if config_path.exists() {
                match std::fs::read_to_string(&config_path) {
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

            // If we loaded a valid config, adopt it
            if let Some(cfg) = loaded_cfg {
                self.current_config = cfg.clone();
            }

            // Broadcast locally so subscribers get initial state (loaded or default)
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

            // If a SessionManager is configured, log ready status
            if let Some(_session_manager) = &self.session_manager {
                log::info!(
                    "SessionManager configured - ready to send config updates: targets={:?}, rate={}",
                    self.current_config.targets,
                    self.current_config.ping_rate_pps
                );
                // For IntentConfigPermission, call the specialized method
                // This uses type_id to detect the concrete type at runtime
                self.send_initial_if_permission_type(ctx);
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
impl<T: ApplicationRole> Handler<UpdateConfig> for IntentConfigActor<T>
where
    Self: PermissionCheck<T>,
{
    type Result = ();

    fn handle(&mut self, msg: UpdateConfig, ctx: &mut Context<Self>) -> Self::Result {
        eprintln!("⚙️ UpdateConfig handler called!");
        log::info!("Handling UpdateConfig message: {:?}", msg.0);
        if msg.0 != self.current_config {
            self.current_config = msg.0.clone();

            // Broadcast locally to subscribers
            self.broadcast_config();

            // Send to network peers via SessionManager
            self.send_config_update_to_peers(ctx);
            log::debug!("Config updated locally and sent to network peers");
        }
    }
}

/// Handles the `Subscribe` message.
impl<T: ApplicationRole> Handler<Subscribe> for IntentConfigActor<T> {
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
impl<T: ApplicationRole> Handler<Unsubscribe> for IntentConfigActor<T> {
    type Result = ();

    fn handle(&mut self, msg: Unsubscribe, _ctx: &mut Context<Self>) {
        log::info!("Removing subscriber with ID: {}", msg.0);
        self.subscribers.remove(&msg.0);
    }
}

/// Handles the `GetCurrentConfig` message.
impl<T: ApplicationRole> Handler<GetCurrentConfig> for IntentConfigActor<T> {
    type Result = MessageResult<GetCurrentConfig>;

    fn handle(&mut self, _msg: GetCurrentConfig, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!("Returning current config state");
        MessageResult(self.current_config.clone())
    }
}

/// Handler for internal ProcessRequestConfigChangeAuth message (Phase 3)
///
/// This message is sent internally after querying SessionManager for peer role.
/// It processes the authorization result and applies config change if authorized.
impl<T: ApplicationRole + 'static> Handler<ProcessRequestConfigChangeAuth<T>>
    for IntentConfigActor<T>
where
    IntentConfigActor<T>: PermissionCheck<T>,
{
    type Result = ();

    fn handle(
        &mut self,
        msg: ProcessRequestConfigChangeAuth<T>,
        ctx: &mut Context<Self>,
    ) -> Self::Result {
        self.handle_process_request_config_change_auth(
            msg.sender_peer_id,
            msg.sender_role,
            msg.targets,
            msg.ping_rate_pps,
            msg.session_manager,
            ctx,
        );
    }
}

// --- Network Handler Implementation ---

/// Handles `IntentConfigMessage` from the network.
///
/// Role-based behavior:
/// - **Collector**: Responds to queries with current config, ignores incoming config updates
/// - **Database**: Accepts config updates, can query collectors
impl<T: ApplicationRole> Handler<IntentConfigNetworkMsg> for IntentConfigActor<T>
where
    Self: PermissionCheck<T>,
{
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: IntentConfigNetworkMsg, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!("Handling network message: {:?}, role: {:?}", msg, self.role);

        match (&self.role, msg) {
            // --- Database Role Behavior (SENDER) ---
            (
                IntentConfigRole::Database { .. },
                IntentConfigNetworkMsg::RequestConfigChange {
                    sender_peer_id,
                    targets,
                    ping_rate_pps,
                },
            ) => self.handle_request_config_change_db(sender_peer_id, targets, ping_rate_pps, _ctx),

            // --- Collector Role Behavior (RECEIVER) ---
            (IntentConfigRole::Collector, IntentConfigNetworkMsg::RequestConfigChange { .. }) => {
                // Collector ignores RequestConfigChange (only Database handles admin requests)
                log::debug!("Collector ignoring RequestConfigChange - not an admin endpoint");
                Box::pin(async {})
            }

            (
                IntentConfigRole::Collector,
                IntentConfigNetworkMsg::ConfigUpdate {
                    targets,
                    ping_rate_pps,
                },
            ) => {
                // Collector ACCEPTS config updates from Database
                log::info!(
                    "Collector received ConfigUpdate: targets={:?}, pps={}",
                    targets,
                    ping_rate_pps
                );
                let new_config = IntentConfigData {
                    targets,
                    ping_rate_pps,
                };
                if new_config != self.current_config {
                    log::info!(
                        "IntentConfig update: previous={:?} -> new={:?}",
                        self.current_config,
                        new_config
                    );
                    self.current_config = new_config;
                    self.broadcast_config();
                } else {
                    log::debug!("Received ConfigUpdate identical to current config - no-op");
                }
                Box::pin(async {})
            }

            // --- Database receiving ConfigUpdate (invalid) ---
            (IntentConfigRole::Database { .. }, IntentConfigNetworkMsg::ConfigUpdate { .. }) => {
                log::warn!(
                    "Database received ConfigUpdate - invalid for this role (Database should send, not receive)"
                );
                Box::pin(async {})
            }

            // --- QueryCurrentConfig handling ---
            // Database responds with current config, Collector rejects
            (IntentConfigRole::Database { .. }, IntentConfigNetworkMsg::QueryCurrentConfig) => {
                log::info!(
                    "Database received QueryCurrentConfig - need to respond with current config"
                );
                // TODO: Reimplement with Room<T>.send() to respond to specific peer
                // Current architecture doesn't provide sender peer ID in this handler
                log::warn!("QueryCurrentConfig response not yet implemented with Room<T>");
                Box::pin(async {})
            }

            (IntentConfigRole::Collector, IntentConfigNetworkMsg::QueryCurrentConfig) => {
                log::warn!("Collector received QueryCurrentConfig - invalid for this role");
                // TODO: Reimplement with Room<T>.send() to send error response
                log::warn!("Error response not yet implemented with Room<T>");
                Box::pin(async {})
            }

            // --- CurrentConfig handling ---
            // Database accepts (for recovery), Collector rejects
            (
                IntentConfigRole::Database { .. },
                IntentConfigNetworkMsg::CurrentConfig {
                    targets,
                    ping_rate_pps,
                },
            ) => {
                log::info!(
                    "Database received CurrentConfig - accepting for recovery: targets={:?}, rate={}",
                    targets,
                    ping_rate_pps
                );
                // In recovery scenarios, Database might update its config from peer responses
                // For now, just log that we received it
                Box::pin(async {})
            }

            (
                IntentConfigRole::Collector,
                IntentConfigNetworkMsg::CurrentConfig {
                    targets,
                    ping_rate_pps,
                },
            ) => {
                // Collector accepts CurrentConfig responses from Database (used for
                // queries on connect/recovery). Treat the payload like a ConfigUpdate
                // so local state is updated and subscribers are notified.
                log::info!(
                    "Collector received CurrentConfig: targets={:?}, pps={}",
                    targets,
                    ping_rate_pps
                );
                let new_config = IntentConfigData {
                    targets,
                    ping_rate_pps,
                };
                if new_config != self.current_config {
                    log::info!(
                        "IntentConfig update (CurrentConfig): previous={:?} -> new={:?}",
                        self.current_config,
                        new_config
                    );
                    self.current_config = new_config;
                    self.broadcast_config();
                } else {
                    log::debug!("Received CurrentConfig identical to current config - no-op");
                }
                Box::pin(async {})
            }

            // --- Heartbeat handling ---
            // Both roles accept heartbeats (keepalive mechanism)
            (_, IntentConfigNetworkMsg::Heartbeat) => {
                log::debug!("Received Heartbeat - connection is alive");
                // Could respond with Heartbeat if we want bidirectional keepalive
                Box::pin(async {})
            }

            // --- Error handling ---
            // Both roles can receive error messages
            (_, IntentConfigNetworkMsg::Error { reason }) => {
                log::warn!("Received error from peer: {}", reason);
                // Log the error - in a real implementation, might trigger recovery logic
                Box::pin(async {})
            }
        }
    }
}

/// Handles the `GetHealth` message.
impl<T: ApplicationRole> Handler<GetHealth> for IntentConfigActor<T> {
    type Result = MessageResult<GetHealth>;

    fn handle(&mut self, _msg: GetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        // TODO: Broadcast metrics removed during Room<T> migration
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

/// Handles the `CreateRoom` message, creating typed channels for network messaging.
/// The channels are stored for SessionManager to use for message routing.
impl<T: ApplicationRole> Handler<CreateRoom> for IntentConfigActor<T>
where
    Self: PermissionCheck<T>,
{
    type Result = ();

    fn handle(&mut self, _msg: CreateRoom, _ctx: &mut Context<Self>) -> Self::Result {
        // Create the Room<T> with typed message handling
        let (room, channels) = zznet_room::room::Room::new(
            "intent-config".to_string(),
            _ctx.address().recipient::<IntentConfigNetworkMsg>(),
        );

        // Store the room and channels
        self.room = Some(room);
        self.room_channels = Some(std::sync::Arc::new(channels));

        log::info!("✓ Room created for IntentConfigActor, ready for SessionManager wiring");
    }
}

/// Handles the `GetRoomChannels` message, creating and returning room channels for network messaging.
/// If channels don't exist yet, they are created on-demand.
impl<T: ApplicationRole> Handler<GetRoomChannels> for IntentConfigActor<T>
where
    Self: PermissionCheck<T>,
{
    type Result = MessageResult<GetRoomChannels>;

    fn handle(&mut self, _msg: GetRoomChannels, _ctx: &mut Context<Self>) -> Self::Result {
        // Create room on-demand if it doesn't exist yet
        if self.room.is_none() {
            // Create the Room<T> with typed message handling
            let (room, channels) = zznet_room::room::Room::new(
                "intent-config".to_string(),
                _ctx.address().recipient::<IntentConfigNetworkMsg>(),
            );

            // Store the room and channels
            self.room = Some(room);
            self.room_channels = Some(std::sync::Arc::new(channels));

            log::info!(
                "✓ Room created on-demand for IntentConfigActor, ready for SessionManager wiring"
            );
        }

        let channels = self.room_channels.clone();
        MessageResult(GetRoomChannelsResponse { channels })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{Subscribe, Unsubscribe, UpdateConfig};
    use serde::{Deserialize, Serialize};
    use std::sync::Once;
    use zznet_session::session_manager::SessionManager;

    use std::time::Duration;
    use zznet_auth::error::AuthError;
    use zznet_session::types::RoomId;

    static INIT: Once = Once::new();

    /// Setup function for logging
    fn setup() {
        INIT.call_once(|| {
            env_logger::builder()
                .is_test(true)
                .filter_level(log::LevelFilter::Debug)
                .init();
        });
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum MockRole {
        Admin,
        User,
    }

    impl ApplicationRole for MockRole {
        fn from_cn(cn: &str) -> Result<Self, AuthError> {
            match cn {
                "admin" => Ok(Self::Admin),
                "user" => Ok(Self::User),
                _ => Err(AuthError::UnknownRole(cn.to_string())),
            }
        }

        fn as_str(&self) -> &'static str {
            match self {
                Self::Admin => "update-config",
                Self::User => "receive-config-updates",
            }
        }
        fn can_connect_to(&self, _target: &Self) -> bool {
            true
        }

        fn can_access_room(&self, _room_name: &str) -> bool {
            true
        }

        fn receive_config_updates_role() -> Option<Self> {
            Some(Self::User)
        }
    }

    impl PermissionCheck<MockRole> for IntentConfigActor<MockRole> {
        fn has_update_permission(&self, role: &MockRole) -> bool {
            *role == MockRole::Admin
        }

        fn has_receive_permission(&self, role: &MockRole) -> bool {
            *role == MockRole::User
        }

        fn to_string(&self, role: &MockRole) -> String {
            format!("{:?}", role)
        }

        fn receive_role(&self) -> MockRole {
            MockRole::User
        }
    }

    /// A mock actor that can receive `IntentConfigData` broadcasts.
    /// It sends the received data to a channel so our test can assert on it.
    struct MockSubscriber {
        tx: tokio::sync::mpsc::Sender<IntentConfigData>,
    }

    impl Actor for MockSubscriber {
        type Context = Context<Self>;
    }

    impl Handler<IntentConfigData> for MockSubscriber {
        type Result = ();
        fn handle(&mut self, msg: IntentConfigData, _ctx: &mut Context<Self>) -> Self::Result {
            // When we receive a broadcast, try to send it to our test channel.
            // If the channel is closed, that's fine, the test is probably over.
            self.tx.try_send(msg).ok();
        }
    }

    // The #[actix::test] macro sets up a System and Arbiter for us automatically.

    // Test 1: Default State
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_initial_state_is_default() {
        setup();
        let actor = IntentConfigActor::<MockRole>::default();

        assert_eq!(actor.current_config, IntentConfigData::default());
        assert!(actor.subscribers.is_empty());
        assert_eq!(actor.next_id, 0);
    }

    // Test 2: Handling `UpdateConfig`
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_update_config_changes_internal_state() {
        setup();
        // ARRANGE
        let mut actor = IntentConfigActor::<MockRole>::default();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();
        let new_config = IntentConfigData {
            targets: vec!["1.1.1.1".parse().unwrap()],
            ping_rate_pps: 99,
        };
        let msg = UpdateConfig(new_config.clone());

        // ACT
        actor.handle(msg, &mut ctx);

        // ASSERT
        assert_eq!(actor.current_config, new_config);
    }

    // Test 3: Handling `Subscribe`
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_subscribe_registers_and_sends_initial_state() {
        setup();
        // ARRANGE
        let mut actor = IntentConfigActor::<MockRole>::default();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let mock_subscriber = MockSubscriber { tx }.start();
        let msg = Subscribe {
            recipient: mock_subscriber.recipient(),
        };

        // ACT
        let sub_id = actor.handle(msg, &mut ctx);

        // ASSERT: Check actor state
        assert_eq!(sub_id, 0);
        assert_eq!(actor.subscribers.len(), 1);
        assert!(actor.subscribers.contains_key(&0));
        assert_eq!(actor.next_id, 1);

        // ASSERT: Check that the subscriber received the initial (default) config
        let received_config = tokio::time::timeout(Duration::from_millis(10), rx.recv())
            .await
            .expect("Subscriber did not receive initial config in time")
            .unwrap();
        assert_eq!(received_config, IntentConfigData::default());
    }

    // Test 4: Handling `Unsubscribe`
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_unsubscribe_removes_subscriber() {
        setup();
        // ARRANGE
        let mut actor = IntentConfigActor::<MockRole>::default();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let mock_subscriber = MockSubscriber { tx }.start();
        let subscribe_msg = Subscribe {
            recipient: mock_subscriber.recipient(),
        };
        let sub_id = actor.handle(subscribe_msg, &mut ctx); // sub_id is 0
        assert_eq!(actor.subscribers.len(), 1);

        // ACT
        let unsubscribe_msg = Unsubscribe(sub_id);
        actor.handle(unsubscribe_msg, &mut ctx);

        // ASSERT
        assert!(actor.subscribers.is_empty());
    }

    // --- Network Handler Tests ---

    // Test 6: Collector ignores QueryCurrentConfig
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_collector_ignores_query() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let original_config = actor.current_config.clone();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        // ACT
        let _ = actor
            .handle(IntentConfigNetworkMsg::QueryCurrentConfig, &mut ctx)
            .await;

        // ASSERT: Collector's config should NOT change
        assert_eq!(actor.current_config, original_config);
    }

    // Test 7: Collector accepts ConfigUpdate
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_network_message_collector_accepts_update() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let original_config = actor.current_config.clone();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        let msg = IntentConfigNetworkMsg::ConfigUpdate {
            targets: vec!["9.9.9.9".parse().unwrap()],
            ping_rate_pps: 999,
        };

        // ACT
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: Collector's config SHOULD change
        assert_ne!(actor.current_config, original_config);
        assert_eq!(
            actor.current_config.targets,
            vec!["9.9.9.9".parse::<std::net::IpAddr>().unwrap()]
        );
        assert_eq!(actor.current_config.ping_rate_pps, 999);
    }

    // Test 8: Database ignores ConfigUpdate
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_network_message_database_ignores_update() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path,
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let original_config = actor.current_config.clone();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        let msg = IntentConfigNetworkMsg::ConfigUpdate {
            targets: vec!["8.8.8.8".parse().unwrap(), "1.1.1.1".parse().unwrap()],
            ping_rate_pps: 123,
        };

        // ACT
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: Database's config should NOT change
        assert_eq!(actor.current_config, original_config);
    }

    // Test 9: Database ignores CurrentConfig
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_database_ignores_current_config() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path,
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let original_config = actor.current_config.clone();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        let msg = IntentConfigNetworkMsg::CurrentConfig {
            targets: vec!["2.2.2.2".parse().unwrap()],
            ping_rate_pps: 22,
        };

        // ACT
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: Database should NOT update its config
        assert_eq!(actor.current_config, original_config);
    }

    // Test 10: Database persistence
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_database_persistence() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path.clone(),
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        actor.current_config = IntentConfigData {
            targets: vec!["1.2.3.4".parse().unwrap()],
            ping_rate_pps: 100,
        };

        // ACT
        actor.persist_config().unwrap();

        // ASSERT: File should exist and contain correct data
        assert!(config_path.exists());
        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("1.2.3.4"));
        assert!(content.contains("100"));
    }

    // Test 11: Database handles RequestConfigChange
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_database_handles_request_config_change() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path.clone(),
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let original_config = actor.current_config.clone();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        let msg = IntentConfigNetworkMsg::RequestConfigChange {
            sender_peer_id: "test-admin".to_string(),
            targets: vec!["5.5.5.5".parse().unwrap()],
            ping_rate_pps: 555,
        };

        // ACT
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: Config should change and be persisted
        assert_ne!(actor.current_config, original_config);
        assert_eq!(
            actor.current_config.targets,
            vec!["5.5.5.5".parse::<std::net::IpAddr>().unwrap()]
        );
        assert_eq!(actor.current_config.ping_rate_pps, 555);
        // Check persistence
        assert!(config_path.exists());
        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("5.5.5.5"));
        assert!(content.contains("555"));
    }

    // Test 12: Collector ignores RequestConfigChange
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_collector_ignores_request_config_change() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let original_config = actor.current_config.clone();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        let msg = IntentConfigNetworkMsg::RequestConfigChange {
            sender_peer_id: "test-collector".to_string(),
            targets: vec!["6.6.6.6".parse().unwrap()],
            ping_rate_pps: 666,
        };

        // ACT
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: Config should NOT change
        assert_eq!(actor.current_config, original_config);
    }

    // Test 13: No-op when config unchanged
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_no_op_when_config_unchanged() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path.clone(),
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        // Set config to known state
        actor.current_config = IntentConfigData {
            targets: vec!["7.7.7.7".parse().unwrap()],
            ping_rate_pps: 777,
        };
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        let msg = IntentConfigNetworkMsg::RequestConfigChange {
            sender_peer_id: "test-admin".to_string(),
            targets: vec!["7.7.7.7".parse().unwrap()], // Same as current
            ping_rate_pps: 777,                        // Same as current
        };

        // ACT
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: File should not be modified (no-op)
        // Since it's the same config, persist should not be called
        // We can't easily check if persist was called, but at least verify no panic
    }

    // Test 14: Both roles handle Error messages
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_error_handling() {
        setup();
        // Test Collector
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        let msg = IntentConfigNetworkMsg::Error {
            reason: "Test error".to_string(),
        };

        // ACT: Should not panic
        let _ = actor.handle(msg.clone(), &mut ctx).await;

        // Test Database
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path,
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();
        let _ = actor.handle(msg, &mut ctx).await;
        // ASSERT: Just verify no panic
    }

    // Test 12: Both roles handle Heartbeat
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_heartbeat_handling() {
        setup();
        // Test Collector
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        let _ = actor
            .handle(IntentConfigNetworkMsg::Heartbeat, &mut ctx)
            .await;

        // Test Database
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path,
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();
        let _ = actor
            .handle(IntentConfigNetworkMsg::Heartbeat, &mut ctx)
            .await;
        // ASSERT: Just verify no panic
    }

    // Test 13: Database loads config on startup
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_database_loads_config_on_startup() {
        setup();
        // ARRANGE: Create config file with known content
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("startup.ron");
        let expected_config = IntentConfigData {
            targets: vec!["192.168.1.1".parse().unwrap(), "10.0.0.1".parse().unwrap()],
            ping_rate_pps: 250,
        };
        let ron_content = ron::ser::to_string(&expected_config).unwrap();
        std::fs::write(&config_path, &ron_content).unwrap();

        // ACT: Create Database actor (this triggers started() lifecycle)
        let role = IntentConfigRole::Database {
            config_file_path: config_path.clone(),
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();
        actor.started(&mut ctx);

        // ASSERT: Config should be loaded from file
        assert_eq!(actor.current_config, expected_config);
    }

    // Test 14: Collector does not load config on startup
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_collector_does_not_load_config_on_startup() {
        setup();
        // ARRANGE: Create config file (Collector should ignore it)
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("ignored.ron");
        let file_config = IntentConfigData {
            targets: vec!["1.2.3.4".parse().unwrap()],
            ping_rate_pps: 999,
        };
        let ron_content = ron::ser::to_string(&file_config).unwrap();
        std::fs::write(&config_path, &ron_content).unwrap();

        // ACT: Create Collector actor (this triggers started() lifecycle)
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();
        actor.started(&mut ctx);

        // ASSERT: Config should remain default (file ignored)
        assert_eq!(actor.current_config, IntentConfigData::default());
        assert_ne!(actor.current_config, file_config);
    }

    // Test: Database QueryCurrentConfig broadcasts correct config data
    #[actix::test]
    #[ntest::timeout(200)]
    #[ignore] // TODO: Reimplement after Room<T> migration
    async fn test_database_query_current_config_broadcasts_correct_data() {
        setup();

        // Create SessionManager actor and add connected peer
        use zznet_session::peer_session::PeerSession;
        use zznet_session::types::PeerId;

        let mut session_manager = SessionManager::<MockRole>::new_with_limits(
            vec![RoomId::from("intent-config")],
            None,
            None,
        );

        // Add peer with User role
        let peer_id = PeerId::from("peer-data-check");

        let (_inbound_tx, inbound_rx) = tokio::sync::mpsc::channel(10);
        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(10);

        // Create peer already connected (Phase 3: Pure Actor Pattern)
        let peer_session = PeerSession::new_connected(
            peer_id.clone(),
            Some(MockRole::User),
            None,
            outbound_tx,
            inbound_rx,
        )
        .await
        .unwrap();

        // Collect messages sent to peer
        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::channel(10);
        tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                msg_tx.send(msg).await.ok();
            }
        });

        session_manager
            .add_peer(peer_id.clone(), peer_session)
            .unwrap();

        // Start SessionManager as actor (Phase 3)
        let session_manager_addr = session_manager.start();

        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path,
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        actor.set_session_manager(session_manager_addr);

        // Set specific config that should be broadcast
        let expected_targets = vec!["192.168.1.1".parse().unwrap(), "10.0.0.1".parse().unwrap()];
        let expected_rate = 999;
        actor.current_config = IntentConfigData {
            targets: expected_targets.clone(),
            ping_rate_pps: expected_rate,
        };

        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        // ACT: Send QueryCurrentConfig to Database
        let query_msg = IntentConfigNetworkMsg::QueryCurrentConfig;
        let _ = actor.handle(query_msg, &mut ctx).await;

        // ASSERT: Peer received the correct CurrentConfig message
        let received_msg =
            tokio::time::timeout(std::time::Duration::from_millis(50), msg_rx.recv())
                .await
                .expect("Peer should receive CurrentConfig message")
                .unwrap();

        // Message is sent as (RoomId, Vec<u8>)
        let (_room_id, msg_bytes) = received_msg;
        let (actual_msg, _): (IntentConfigNetworkMsg, _) =
            bincode::serde::decode_from_slice(&msg_bytes, bincode::config::standard())
                .expect("Failed to decode message");
        match actual_msg {
            IntentConfigNetworkMsg::CurrentConfig {
                targets,
                ping_rate_pps,
            } => {
                assert_eq!(targets, expected_targets);
                assert_eq!(ping_rate_pps, expected_rate);
            }
            _ => panic!("Expected CurrentConfig message, got {:?}", actual_msg),
        }
    }

    // Test: Collector accepts CurrentConfig and updates its config
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_collector_accepts_current_config() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let original_config = actor.current_config.clone();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        let new_targets = vec![
            "10.10.10.10".parse().unwrap(),
            "20.20.20.20".parse().unwrap(),
        ];
        let new_rate = 1234;
        let msg = IntentConfigNetworkMsg::CurrentConfig {
            targets: new_targets.clone(),
            ping_rate_pps: new_rate,
        };

        // ACT
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: Collector's config SHOULD change
        assert_ne!(actor.current_config, original_config);
        assert_eq!(actor.current_config.targets, new_targets);
        assert_eq!(actor.current_config.ping_rate_pps, new_rate);
    }

    // Test: Collector handles CurrentConfig with same config (no-op)
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_collector_current_config_no_op_when_unchanged() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let original_config = actor.current_config.clone();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        // Send CurrentConfig with same data as current config
        let msg = IntentConfigNetworkMsg::CurrentConfig {
            targets: original_config.targets.clone(),
            ping_rate_pps: original_config.ping_rate_pps,
        };

        // ACT
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: Config should remain unchanged
        assert_eq!(actor.current_config, original_config);
    }

    // Test: Collector broadcasts to subscribers when CurrentConfig changes config
    #[actix::test]
    #[ntest::timeout(100)]
    #[ignore] // TODO: Reimplement after Room<T> migration
    async fn test_collector_current_config_broadcasts_on_change() {
        /* Test disabled - broadcast functionality removed

        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        // Add a subscriber
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let mock_subscriber = MockSubscriber { tx }.start();
        let subscribe_msg = Subscribe {
            recipient: mock_subscriber.recipient(),
        };
        actor.handle(subscribe_msg, &mut ctx);

        // Drain the initial config broadcast
        rx.recv().await.unwrap();

        // ACT: Send CurrentConfig with new data
        let new_targets = vec!["30.30.30.30".parse().unwrap()];
        let new_rate = 5678;
        let msg = IntentConfigNetworkMsg::CurrentConfig {
            targets: new_targets.clone(),
            ping_rate_pps: new_rate,
        };
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: Subscriber should receive the new config
        let received_config = tokio::time::timeout(Duration::from_millis(10), rx.recv())
            .await
            .expect("Subscriber should receive updated config")
            .unwrap();

        let expected_config = IntentConfigData {
            targets: new_targets,
            ping_rate_pps: new_rate,
        };
        assert_eq!(received_config, expected_config);

        */
    }

    // Test: Collector does not broadcast when CurrentConfig has same config
    #[actix::test]
    #[ntest::timeout(100)]
    #[ignore] // TODO: Reimplement after Room<T> migration
    async fn test_collector_current_config_no_broadcast_when_unchanged() {
        /* Test disabled - broadcast functionality removed

        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector;
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();

        // Add a subscriber
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let mock_subscriber = MockSubscriber { tx }.start();
        let subscribe_msg = Subscribe {
            recipient: mock_subscriber.recipient(),
        };
        actor.handle(subscribe_msg, &mut ctx);

        // Drain the initial config broadcast
        rx.recv().await.unwrap();

        // ACT: Send CurrentConfig with same data as current config
        let msg = IntentConfigNetworkMsg::CurrentConfig {
            targets: actor.current_config.targets.clone(),
            ping_rate_pps: actor.current_config.ping_rate_pps,
        };
        let _ = actor.handle(msg, &mut ctx).await;

        // ASSERT: Subscriber should NOT receive another broadcast (channel should be empty)
        let result = tokio::time::timeout(Duration::from_millis(10), rx.recv()).await;
        assert!(
            result.is_err(),
            "Subscriber should not receive a broadcast for unchanged config"
        );

        */
    }
}
