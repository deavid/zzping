//! Contains the private implementation of the IntentConfigActor, including its
//! state and message handling logic.

use crate::messages::{GetCurrentConfig, IntentConfigData, Subscribe, Unsubscribe, UpdateConfig};
use crate::network_messages::IntentConfigMessage;
use crate::permission_wrapper::PermissionWrapper;
use crate::permissions::PermissionCheck;
use crate::role::IntentConfigRole;
use actix::ResponseFuture;
use actix::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;
use zznet_auth::role::ApplicationRole;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::RoomId;

/// The IntentConfigActor stores the current configuration and manages subscribers.
/// This struct is the private state of our component.
///
/// # ⚠️ Security Warning (Phase 2)
///
/// Accepts `RequestConfigChange` from ANY peer without auth checks.
/// See crate-level docs for full security warning and requirements.
pub struct IntentConfigActor<T: ApplicationRole + std::fmt::Debug> {
    current_config: IntentConfigData,
    subscribers: HashMap<usize, Recipient<IntentConfigData>>,
    next_id: usize,

    /// Role configuration (Collector or Database)
    role: IntentConfigRole,

    /// SessionManager for network communication (Phase 3)
    session_manager: Option<Rc<SessionManager<IntentConfigMessage, PermissionWrapper<T>>>>,
}

impl<T: ApplicationRole + std::fmt::Debug> Clone for IntentConfigActor<T> {
    fn clone(&self) -> Self {
        Self {
            current_config: self.current_config.clone(),
            subscribers: self.subscribers.clone(),
            next_id: self.next_id,
            role: self.role.clone(),
            session_manager: self.session_manager.clone(),
        }
    }
}

impl<T: ApplicationRole + std::fmt::Debug> Default for IntentConfigActor<T> {
    fn default() -> Self {
        Self::new_with_role(IntentConfigRole::default())
    }
}

// Provide PermissionCheck implementation for the concrete IntentConfigPermission
// so that tests and SessionManager integration using the concrete enum work.
impl PermissionCheck<crate::permissions::IntentConfigPermission>
    for IntentConfigActor<crate::permissions::IntentConfigPermission>
{
    fn has_update_permission(&self, role: &crate::permissions::IntentConfigPermission) -> bool {
        *role == crate::permissions::IntentConfigPermission::UpdateConfig
    }

    fn has_receive_permission(&self, role: &crate::permissions::IntentConfigPermission) -> bool {
        *role == crate::permissions::IntentConfigPermission::ReceiveConfigUpdates
    }

    fn to_string(&self, role: &crate::permissions::IntentConfigPermission) -> String {
        format!("{:?}", role)
    }
}

impl<T: ApplicationRole + std::fmt::Debug> IntentConfigActor<T> {
    /// Create a new IntentConfigActor with the specified role
    pub fn new_with_role(role: IntentConfigRole) -> Self {
        Self {
            current_config: IntentConfigData::default(),
            subscribers: HashMap::new(),
            next_id: 0,
            role,
            session_manager: None,
        }
    }

    /// Get the current role
    pub fn role(&self) -> &IntentConfigRole {
        &self.role
    }

    /// Set the SessionManager for network communication
    pub fn set_session_manager(
        &mut self,
        session_manager: Rc<SessionManager<IntentConfigMessage, PermissionWrapper<T>>>,
    ) {
        self.session_manager = Some(session_manager);
    }

    /// The logic to broadcast the current configuration to all subscribers.
    fn broadcast_config(&self) {
        for (id, recipient) in &self.subscribers {
            log::info!("Broadcasting update to subscriber {}", id);
            // `do_send` is a "tell" or fire-and-forget send. It does not wait for a response.
            recipient.do_send(self.current_config.clone());
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
            .map_err(std::io::Error::other)
            .unwrap();

        // Write to file atomically (write to temp file, then rename)
        let temp_path = config_path.with_extension("tmp");
        std::fs::write(&temp_path, config_string)?;
        std::fs::rename(&temp_path, config_path)?;

        log::info!("Config successfully persisted");
        Ok(())
    }

    /// Spawn a background task to send the given ConfigUpdate to all collector peers
    /// using the provided SessionManager. Returns a ResponseFuture suitable for
    /// returning from a handler when needed.
    fn send_config_update_to_peers(
        session_manager: Rc<SessionManager<IntentConfigMessage, PermissionWrapper<T>>>,
        config_update: IntentConfigMessage,
    ) -> ResponseFuture<()> {
        Box::pin(async move {
            let room_id = RoomId::from("intent-config");
            let peer_ids = session_manager.peer_ids();
            if peer_ids.is_empty() {
                log::warn!("No peers connected - ConfigUpdate not sent to network");
                return;
            }

            for peer_id in peer_ids {
                if let Some(peer_role) = session_manager.get_peer_role(&peer_id) {
                    // Fallback: use permission string to identify collector-like peers
                    if peer_role.permission.as_str() != "receive-config-updates" {
                        continue;
                    }

                    if let Err(e) = session_manager
                        .send_to_room(&peer_id, &room_id, config_update.clone())
                        .await
                    {
                        log::warn!("Failed to send ConfigUpdate to {}: {}", peer_id, e);
                    } else {
                        log::info!("Sent ConfigUpdate to {}", peer_id);
                    }
                }
            }
        })
    }

    /// Spawn a fire-and-forget task to send an Error message to a specific peer.
    fn spawn_send_error(
        session_manager: Rc<SessionManager<IntentConfigMessage, PermissionWrapper<T>>>,
        peer: String,
        reason: impl Into<String>,
    ) {
        let room_id = RoomId::from("intent-config");
        let em = IntentConfigMessage::error(reason.into());
        actix::spawn(async move {
            if let Err(e) = session_manager
                .send_to_room(
                    &zznet_session::types::PeerId::from(peer.as_str()),
                    &room_id,
                    em,
                )
                .await
            {
                log::warn!("Failed to send Error to {}: {}", peer, e);
            }
        });
    }

    /// Spawn CurrentConfig message to all peers (for responding to queries)
    fn spawn_send_current_config_to_peers(
        session_manager: Rc<SessionManager<IntentConfigMessage, PermissionWrapper<T>>>,
        current_config: IntentConfigMessage,
    ) {
        actix::spawn(async move {
            let room_id = RoomId::from("intent-config");

            for peer_id in session_manager.peer_ids() {
                if let Err(e) = session_manager
                    .send_to_room(&peer_id, &room_id, current_config.clone())
                    .await
                {
                    log::warn!("Failed to send CurrentConfig to {}: {}", peer_id, e);
                } else {
                    log::debug!("Sent CurrentConfig to {}", peer_id);
                }
            }
        });
    }

    /// Spawn initial ConfigUpdate messages to all collector peers (fire-and-forget)
    fn spawn_send_initial_updates(
        session_manager: Rc<SessionManager<IntentConfigMessage, PermissionWrapper<T>>>,
        cfg: IntentConfigData,
    ) {
        let cfg_clone = cfg.clone();
        actix::spawn(async move {
            let room_id = RoomId::from("intent-config");
            let config_update = IntentConfigMessage::ConfigUpdate {
                targets: cfg_clone.targets.clone(),
                ping_rate_pps: cfg_clone.ping_rate_pps,
            };

            for peer_id in session_manager.peer_ids() {
                if let Some(peer_role) = session_manager.get_peer_role(&peer_id) {
                    if peer_role.permission.as_str() != "receive-config-updates" {
                        continue;
                    }

                    if let Err(e) = session_manager
                        .send_to_room(&peer_id, &room_id, config_update.clone())
                        .await
                    {
                        log::warn!("Failed to send initial ConfigUpdate to {}: {}", peer_id, e);
                    } else {
                        log::info!("Sent initial ConfigUpdate to {}", peer_id);
                    }
                }
            }
        });
    }
}

/// This is the boilerplate that officially makes the struct an Actix Actor.
impl<T: ApplicationRole + std::fmt::Debug> Actor for IntentConfigActor<T> {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Context<Self>) {
        log::info!("IntentConfigActor has started.");

        // On startup as Database: if a config file exists, load it and broadcast
        match &self.role {
            IntentConfigRole::Database { config_file_path } if config_file_path.exists() => {
                match std::fs::read_to_string(config_file_path) {
                    Ok(s) => match ron::de::from_str::<IntentConfigData>(&s) {
                        Ok(cfg) => {
                            // Validate the loaded config
                            match cfg.validate() {
                                Ok(()) => {
                                    log::info!(
                                        "Loaded and validated persisted IntentConfig from {}",
                                        config_file_path.display()
                                    );
                                    self.current_config = cfg.clone();
                                    // Broadcast locally so subscribers get initial state
                                    self.broadcast_config();

                                    // If a SessionManager is configured, try sending initial
                                    // ConfigUpdate to Collector peers using helper
                                    if let Some(session_manager) = &self.session_manager {
                                        let session_manager = Rc::clone(session_manager);
                                        Self::spawn_send_initial_updates(session_manager, cfg);
                                    }
                                }
                                Err(validation_error) => {
                                    log::error!(
                                        "Loaded config from {} is invalid: {}. Using default config.",
                                        config_file_path.display(),
                                        validation_error
                                    );
                                    // Keep default config, don't broadcast invalid config
                                }
                            }
                        }
                        Err(e) => log::warn!("Failed to parse persisted IntentConfig: {}", e),
                    },
                    Err(e) => log::warn!("Failed to read persisted IntentConfig file: {}", e),
                }
            }
            _ => {}
        }
    }
}

// --- Handler Implementations (The Business Logic) ---

/// Handles the `UpdateConfig` message.
impl<T: ApplicationRole + std::fmt::Debug> Handler<UpdateConfig> for IntentConfigActor<T> {
    type Result = ();

    fn handle(&mut self, msg: UpdateConfig, _ctx: &mut Context<Self>) -> Self::Result {
        log::info!("Handling UpdateConfig message: {:?}", msg.0);
        if msg.0 != self.current_config {
            self.current_config = msg.0;
            self.broadcast_config();
        }
    }
}

/// Handles the `Subscribe` message.
impl<T: ApplicationRole + std::fmt::Debug> Handler<Subscribe> for IntentConfigActor<T> {
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
impl<T: ApplicationRole + std::fmt::Debug> Handler<Unsubscribe> for IntentConfigActor<T> {
    type Result = ();

    fn handle(&mut self, msg: Unsubscribe, _ctx: &mut Context<Self>) {
        log::info!("Removing subscriber with ID: {}", msg.0);
        self.subscribers.remove(&msg.0);
    }
}

/// Handles the `GetCurrentConfig` message.
impl<T: ApplicationRole + std::fmt::Debug> Handler<GetCurrentConfig> for IntentConfigActor<T> {
    type Result = MessageResult<GetCurrentConfig>;

    fn handle(&mut self, _msg: GetCurrentConfig, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!("Returning current config state");
        MessageResult(self.current_config.clone())
    }
}

// --- Network Handler Implementation ---

/// Handles `IntentConfigMessage` from the network.
///
/// Role-based behavior:
/// - **Collector**: Responds to queries with current config, ignores incoming config updates
/// - **Database**: Accepts config updates, can query collectors
impl<
    T: ApplicationRole + 'static + Send + Clone + std::fmt::Debug + PartialEq + Eq + std::hash::Hash,
> Handler<IntentConfigMessage> for IntentConfigActor<T>
where
    Self: PermissionCheck<T>,
{
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: IntentConfigMessage, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!("Handling network message: {:?}, role: {:?}", msg, self.role);

        match (&self.role, msg) {
            // --- Database Role Behavior (SENDER) ---
            (
                IntentConfigRole::Database { .. },
                IntentConfigMessage::RequestConfigChange {
                    sender_peer_id,
                    targets,
                    ping_rate_pps,
                },
            ) => {
                log::info!(
                    "Received RequestConfigChange from peer '{}': targets={:?}, rate={}",
                    sender_peer_id,
                    targets,
                    ping_rate_pps
                );

                // AUTHORIZATION CHECK: Only ClientAdmin can change config
                // Note: If no SessionManager is configured (e.g., unit tests), allow the request
                // for backward compatibility
                if let Some(session_manager) = &self.session_manager {
                    // Look up the sender's role
                    let sender_role = session_manager.get_peer_role(
                        &zznet_session::types::PeerId::from(sender_peer_id.as_str()),
                    );

                    if let Some(sender_role) = sender_role {
                        if !self.has_update_permission(&sender_role.permission) {
                            log::warn!(
                                "✗ Config change REJECTED from peer '{}' - role {:?} is not authorized",
                                sender_peer_id,
                                sender_role
                            );

                            // Try to send an Error message back to the requester
                            Self::spawn_send_error(
                                Rc::clone(session_manager),
                                sender_peer_id.clone(),
                                "unauthorized: insufficient permission".to_string(),
                            );

                            return Box::pin(async {});
                        }
                    } else {
                        log::warn!(
                            "✗ Config change REJECTED from peer '{}' - no role information available (ACL not configured?)",
                            sender_peer_id
                        );

                        // Send explicit error if possible
                        Self::spawn_send_error(
                            Rc::clone(session_manager),
                            sender_peer_id.clone(),
                            "no-role: ACL not configured".to_string(),
                        );

                        return Box::pin(async {});
                    }
                } else {
                    // No SessionManager configured - this should only happen in unit tests
                    // Production deployments MUST configure SessionManager for security
                    //
                    // NOTE: This debug/release behavior is tested in integration tests.
                    // Tests rely on permissive debug-mode behavior for ergonomics.
                    // See CONTRIBUTING.md for details on running tests.
                    #[cfg(debug_assertions)]
                    log::warn!(
                        "⚠️  Config change allowed WITHOUT auth check (test mode - no SessionManager) - peer: '{}',",
                        sender_peer_id
                    );

                    #[cfg(not(debug_assertions))]
                    {
                        // In release builds, reject requests without SessionManager
                        log::error!(
                            "✗ Config change REJECTED from peer '{}' - SessionManager required in production",
                            sender_peer_id
                        );
                        return Box::pin(async {});
                    }
                }

                // Proceed with configuration update (existing code continues...)
                let new_config = IntentConfigData {
                    targets,
                    ping_rate_pps,
                };
                if new_config != self.current_config {
                    self.current_config = new_config;
                    if let Err(e) = self.persist_config() {
                        log::error!("Failed to persist config: {}", e);

                        // Try inform the requester of the persistence failure
                        if let Some(session_manager) = &self.session_manager {
                            Self::spawn_send_error(
                                Rc::clone(session_manager),
                                sender_peer_id.clone(),
                                format!("persist-failure: {}", e),
                            );
                        }

                        Box::pin(async {})
                    } else {
                        self.broadcast_config();
                        // Send ConfigUpdate to each Collector via SessionManager (Phase 3)
                        if let Some(session_manager) = &self.session_manager {
                            let session_manager = Rc::clone(session_manager);
                            let config_update = IntentConfigMessage::ConfigUpdate {
                                targets: self.current_config.targets.clone(),
                                ping_rate_pps: self.current_config.ping_rate_pps,
                            };
                            Self::send_config_update_to_peers(session_manager, config_update)
                        } else {
                            log::warn!(
                                "No SessionManager configured - ConfigUpdate not sent to network"
                            );
                            Box::pin(async {})
                        }
                    }
                } else {
                    log::info!("Config unchanged, no action needed");
                    Box::pin(async {})
                }
            }

            // --- Collector Role Behavior (RECEIVER) ---
            (IntentConfigRole::Collector, IntentConfigMessage::RequestConfigChange { .. }) => {
                // Collector ignores RequestConfigChange (only Database handles admin requests)
                log::debug!("Collector ignoring RequestConfigChange - not an admin endpoint");
                Box::pin(async {})
            }

            (
                IntentConfigRole::Collector,
                IntentConfigMessage::ConfigUpdate {
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
                    self.current_config = new_config;
                    self.broadcast_config();
                }
                Box::pin(async {})
            }

            // --- Database receiving ConfigUpdate (invalid) ---
            (IntentConfigRole::Database { .. }, IntentConfigMessage::ConfigUpdate { .. }) => {
                log::warn!(
                    "Database received ConfigUpdate - invalid for this role (Database should send, not receive)"
                );
                Box::pin(async {})
            }

            // --- QueryCurrentConfig handling ---
            // Database responds with current config, Collector rejects
            (IntentConfigRole::Database { .. }, IntentConfigMessage::QueryCurrentConfig) => {
                log::info!("Database received QueryCurrentConfig - responding with current config");
                if let Some(session_manager) = &self.session_manager {
                    // Find the peer that sent this query to respond to them
                    // Note: We don't have direct access to sender peer ID here, so we broadcast
                    // In a real implementation, we'd need to track the sender
                    let current_config = IntentConfigMessage::CurrentConfig {
                        targets: self.current_config.targets.clone(),
                        ping_rate_pps: self.current_config.ping_rate_pps,
                    };
                    Self::spawn_send_current_config_to_peers(
                        Rc::clone(session_manager),
                        current_config,
                    );
                }
                Box::pin(async {})
            }

            (IntentConfigRole::Collector, IntentConfigMessage::QueryCurrentConfig) => {
                log::warn!("Collector received QueryCurrentConfig - invalid for this role");
                if let Some(session_manager) = &self.session_manager {
                    Self::spawn_send_error(
                        Rc::clone(session_manager),
                        "unknown".to_string(), // We don't have sender info in this context
                        "invalid-role: Collector cannot respond to config queries".to_string(),
                    );
                }
                Box::pin(async {})
            }

            // --- CurrentConfig handling ---
            // Database accepts (for recovery), Collector rejects
            (
                IntentConfigRole::Database { .. },
                IntentConfigMessage::CurrentConfig {
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

            (IntentConfigRole::Collector, IntentConfigMessage::CurrentConfig { .. }) => {
                log::warn!("Collector received CurrentConfig - invalid for this role");
                if let Some(session_manager) = &self.session_manager {
                    Self::spawn_send_error(
                        Rc::clone(session_manager),
                        "unknown".to_string(),
                        "invalid-role: Collector should not send config responses".to_string(),
                    );
                }
                Box::pin(async {})
            }

            // --- Heartbeat handling ---
            // Both roles accept heartbeats (keepalive mechanism)
            (_, IntentConfigMessage::Heartbeat) => {
                log::debug!("Received Heartbeat - connection is alive");
                // Could respond with Heartbeat if we want bidirectional keepalive
                Box::pin(async {})
            }

            // --- Error handling ---
            // Both roles can receive error messages
            (_, IntentConfigMessage::Error { reason }) => {
                log::warn!("Received error from peer: {}", reason);
                // Log the error - in a real implementation, might trigger recovery logic
                Box::pin(async {})
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{Subscribe, Unsubscribe, UpdateConfig};
    use serde::{Deserialize, Serialize};
    use std::sync::Once;
    use std::time::Duration;
    use zznet_auth::error::AuthError;

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
                Self::Admin => "admin",
                Self::User => "user",
            }
        }

        fn can_connect_to(&self, _target: &Self) -> bool {
            true
        }

        fn can_access_room(&self, _room_name: &str) -> bool {
            true
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

    // Test 5: Interaction Scenario (`Update` triggers `Broadcast`)
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_update_broadcasts_to_subscriber() {
        setup();
        // ARRANGE
        let mut actor = IntentConfigActor::<MockRole>::default();
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();
        // Create and register a mock subscriber
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let mock_subscriber = MockSubscriber { tx }.start();
        let subscribe_msg = Subscribe {
            recipient: mock_subscriber.recipient(),
        };
        actor.handle(subscribe_msg, &mut ctx);
        // Drain the initial config broadcast so our channel is empty
        rx.recv().await.unwrap();

        // ARRANGE: Create the new config for the update
        let new_config = IntentConfigData {
            targets: vec!["8.8.8.8".parse().unwrap()],
            ping_rate_pps: 50,
        };
        let update_msg = UpdateConfig(new_config.clone());

        // ACT
        actor.handle(update_msg, &mut ctx);

        // ASSERT: Check that the subscriber received the NEW config
        let received_config = tokio::time::timeout(Duration::from_millis(10), rx.recv())
            .await
            .expect("Subscriber did not receive updated config in time")
            .unwrap();
        assert_eq!(received_config, new_config);
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
            .handle(IntentConfigMessage::QueryCurrentConfig, &mut ctx)
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

        let msg = IntentConfigMessage::ConfigUpdate {
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

        let msg = IntentConfigMessage::ConfigUpdate {
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

        let msg = IntentConfigMessage::CurrentConfig {
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

        let msg = IntentConfigMessage::RequestConfigChange {
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

        let msg = IntentConfigMessage::RequestConfigChange {
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

        let msg = IntentConfigMessage::RequestConfigChange {
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

        let msg = IntentConfigMessage::Error {
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

        let _ = actor.handle(IntentConfigMessage::Heartbeat, &mut ctx).await;

        // Test Database
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path,
        };
        let mut actor = IntentConfigActor::<MockRole>::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor<MockRole>>::new();
        let _ = actor.handle(IntentConfigMessage::Heartbeat, &mut ctx).await;
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
}
