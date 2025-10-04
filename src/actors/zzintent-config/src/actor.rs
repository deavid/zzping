//! Contains the private implementation of the IntentConfigActor, including its
//! state and message handling logic.

use crate::messages::{IntentConfigData, Subscribe, Unsubscribe, UpdateConfig};
use crate::network_messages::IntentConfigMessage;
use crate::role::IntentConfigRole;
use actix::prelude::*;
use std::collections::HashMap;

/// The IntentConfigActor stores the current configuration and manages subscribers.
/// This struct is the private state of our component.
#[derive(Debug)]
pub struct IntentConfigActor {
    current_config: IntentConfigData,
    subscribers: HashMap<usize, Recipient<IntentConfigData>>,
    next_id: usize,

    /// Role configuration (Collector or Database)
    role: IntentConfigRole,
}

impl Default for IntentConfigActor {
    fn default() -> Self {
        Self::new_with_role(IntentConfigRole::default())
    }
}

impl IntentConfigActor {
    /// Create a new IntentConfigActor with the specified role
    pub fn new_with_role(role: IntentConfigRole) -> Self {
        Self {
            current_config: IntentConfigData::default(),
            subscribers: HashMap::new(),
            next_id: 0,
            role,
        }
    }

    /// Get the current role
    pub fn role(&self) -> &IntentConfigRole {
        &self.role
    }

    /// The logic to broadcast the current configuration to all subscribers.
    fn broadcast_config(&self) {
        for (id, recipient) in &self.subscribers {
            log::info!("Broadcasting update to subscriber {}", id);
            // `do_send` is a "tell" or fire-and-forget send. It does not wait for a response.
            recipient.do_send(self.current_config.clone());
        }
    }
}

/// This is the boilerplate that officially makes the struct an Actix Actor.
impl Actor for IntentConfigActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Context<Self>) {
        log::info!("IntentConfigActor has started.");
    }
}

// --- Handler Implementations (The Business Logic) ---

/// Handles the `UpdateConfig` message.
impl Handler<UpdateConfig> for IntentConfigActor {
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

// --- Network Handler Implementation ---

/// Handles `IntentConfigMessage` from the network.
///
/// Role-based behavior:
/// - **Collector**: Responds to queries with current config, ignores incoming config updates
/// - **Database**: Accepts config updates, can query collectors
impl Handler<IntentConfigMessage> for IntentConfigActor {
    type Result = ();

    fn handle(&mut self, msg: IntentConfigMessage, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!("Handling network message: {:?}, role: {:?}", msg, self.role);

        match (&self.role, msg) {
            // --- Collector Role Behavior ---
            (IntentConfigRole::Collector { .. }, IntentConfigMessage::QueryCurrentConfig) => {
                // Collector responds with current config
                log::info!("Collector responding to query with current config");
                // TODO: Send response via SessionManager
                // For now, just log - we'll add SessionManager integration in Phase 4
            }

            (IntentConfigRole::Collector { .. }, IntentConfigMessage::ConfigUpdate { .. }) => {
                // Collector ignores incoming config updates (it's the source)
                log::debug!("Collector ignoring incoming ConfigUpdate");
            }

            (IntentConfigRole::Collector { .. }, IntentConfigMessage::CurrentConfig { .. }) => {
                // Collector ignores CurrentConfig responses (it doesn't query)
                log::debug!("Collector ignoring incoming CurrentConfig");
            }

            // --- Database Role Behavior ---
            (
                IntentConfigRole::Database,
                IntentConfigMessage::ConfigUpdate {
                    targets,
                    ping_rate_pps,
                },
            ) => {
                // Database accepts config updates from collectors
                log::info!(
                    "Database received ConfigUpdate: targets={:?}, pps={}",
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
                // TODO: Send acknowledgment via SessionManager
            }

            (
                IntentConfigRole::Database,
                IntentConfigMessage::CurrentConfig {
                    targets,
                    ping_rate_pps,
                },
            ) => {
                // Database received response to query
                log::info!(
                    "Database received CurrentConfig: targets={:?}, pps={}",
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
            }

            // --- Common Behavior ---
            (_, IntentConfigMessage::Heartbeat) => {
                // Both roles accept heartbeats
                log::debug!("Received heartbeat");
            }

            (_, IntentConfigMessage::Error { reason }) => {
                // Both roles log errors
                log::error!("Received error from peer: {}", reason);
            }

            // --- Invalid combinations ---
            (IntentConfigRole::Database, IntentConfigMessage::QueryCurrentConfig) => {
                // Database shouldn't receive queries (only collectors respond)
                log::warn!("Database received QueryCurrentConfig - invalid for this role");
                // TODO: Send error response via SessionManager
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{Subscribe, Unsubscribe, UpdateConfig};
    use std::time::Duration;

    /// Setup function for logging
    fn setup() {
        let _ = env_logger::builder()
            .is_test(true)
            .filter_level(log::LevelFilter::Debug)
            .try_init();
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
        let actor = IntentConfigActor::default();

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
        let mut actor = IntentConfigActor::default();
        let mut ctx = Context::<IntentConfigActor>::new();
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
        let mut actor = IntentConfigActor::default();
        let mut ctx = Context::<IntentConfigActor>::new();
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
        let mut actor = IntentConfigActor::default();
        let mut ctx = Context::<IntentConfigActor>::new();
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
        let mut actor = IntentConfigActor::default();
        let mut ctx = Context::<IntentConfigActor>::new();
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

    // Test 6: Collector handles QueryCurrentConfig
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_collector_handles_query() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector {
            config_file_path: "/etc/intent.ron".into(),
        };
        let mut actor = IntentConfigActor::new_with_role(role);
        actor.current_config = IntentConfigData {
            targets: vec!["1.1.1.1".parse().unwrap()],
            ping_rate_pps: 42,
        };
        let mut ctx = Context::<IntentConfigActor>::new();

        // ACT
        actor.handle(IntentConfigMessage::QueryCurrentConfig, &mut ctx);

        // ASSERT: Just verify it doesn't panic - response will be added in Phase 4
    }

    // Test 7: Collector ignores ConfigUpdate
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_collector_ignores_config_update() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector {
            config_file_path: "/etc/intent.ron".into(),
        };
        let mut actor = IntentConfigActor::new_with_role(role);
        let original_config = actor.current_config.clone();
        let mut ctx = Context::<IntentConfigActor>::new();

        let msg = IntentConfigMessage::ConfigUpdate {
            targets: vec!["9.9.9.9".parse().unwrap()],
            ping_rate_pps: 999,
        };

        // ACT
        actor.handle(msg, &mut ctx);

        // ASSERT: Collector's config should NOT change
        assert_eq!(actor.current_config, original_config);
    }

    // Test 8: Database accepts ConfigUpdate
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_database_accepts_config_update() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Database;
        let mut actor = IntentConfigActor::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor>::new();

        let msg = IntentConfigMessage::ConfigUpdate {
            targets: vec!["8.8.8.8".parse().unwrap(), "1.1.1.1".parse().unwrap()],
            ping_rate_pps: 123,
        };

        // ACT
        actor.handle(msg, &mut ctx);

        // ASSERT: Database's config should change
        assert_eq!(
            actor.current_config.targets,
            vec![
                "8.8.8.8".parse::<std::net::IpAddr>().unwrap(),
                "1.1.1.1".parse().unwrap()
            ]
        );
        assert_eq!(actor.current_config.ping_rate_pps, 123);
    }

    // Test 9: Database broadcasts on network update
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_database_broadcasts_network_update() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Database;
        let mut actor = IntentConfigActor::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor>::new();

        // Add a subscriber
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let mock_subscriber = MockSubscriber { tx }.start();
        let subscribe_msg = Subscribe {
            recipient: mock_subscriber.recipient(),
        };
        actor.handle(subscribe_msg, &mut ctx);
        // Drain initial config
        rx.recv().await.unwrap();

        let msg = IntentConfigMessage::ConfigUpdate {
            targets: vec!["7.7.7.7".parse().unwrap()],
            ping_rate_pps: 77,
        };

        // ACT
        actor.handle(msg, &mut ctx);

        // ASSERT: Subscriber should receive the new config
        let received_config = tokio::time::timeout(Duration::from_millis(10), rx.recv())
            .await
            .expect("Subscriber did not receive network update")
            .unwrap();
        assert_eq!(
            received_config.targets,
            vec!["7.7.7.7".parse::<std::net::IpAddr>().unwrap()]
        );
        assert_eq!(received_config.ping_rate_pps, 77);
    }

    // Test 10: Database handles CurrentConfig response
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_database_handles_current_config() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Database;
        let mut actor = IntentConfigActor::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor>::new();

        let msg = IntentConfigMessage::CurrentConfig {
            targets: vec!["2.2.2.2".parse().unwrap()],
            ping_rate_pps: 22,
        };

        // ACT
        actor.handle(msg, &mut ctx);

        // ASSERT: Database should update its config
        assert_eq!(
            actor.current_config.targets,
            vec!["2.2.2.2".parse::<std::net::IpAddr>().unwrap()]
        );
        assert_eq!(actor.current_config.ping_rate_pps, 22);
    }

    // Test 11: Both roles handle Error messages
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_error_handling() {
        setup();
        // Test Collector
        let role = IntentConfigRole::Collector {
            config_file_path: "/etc/intent.ron".into(),
        };
        let mut actor = IntentConfigActor::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor>::new();

        let msg = IntentConfigMessage::Error {
            reason: "Test error".to_string(),
        };

        // ACT: Should not panic
        actor.handle(msg.clone(), &mut ctx);

        // Test Database
        let mut actor = IntentConfigActor::new_with_role(IntentConfigRole::Database);
        let mut ctx = Context::<IntentConfigActor>::new();
        actor.handle(msg, &mut ctx);
        // ASSERT: Just verify no panic
    }

    // Test 12: Both roles handle Heartbeat
    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_heartbeat_handling() {
        setup();
        // Test Collector
        let role = IntentConfigRole::Collector {
            config_file_path: "/etc/intent.ron".into(),
        };
        let mut actor = IntentConfigActor::new_with_role(role);
        let mut ctx = Context::<IntentConfigActor>::new();

        actor.handle(IntentConfigMessage::Heartbeat, &mut ctx);

        // Test Database
        let mut actor = IntentConfigActor::new_with_role(IntentConfigRole::Database);
        let mut ctx = Context::<IntentConfigActor>::new();
        actor.handle(IntentConfigMessage::Heartbeat, &mut ctx);
        // ASSERT: Just verify no panic
    }
}
