//! Contains the private implementation of the IntentConfigActor, including its
//! state and message handling logic.

use crate::messages::{IntentConfigData, Subscribe, Unsubscribe, UpdateConfig};
use actix::prelude::*;
use std::collections::HashMap;

/// The IntentConfigActor stores the current configuration and manages subscribers.
/// This struct is the private state of our component.
#[derive(Debug, Default)]
pub struct IntentConfigActor {
    current_config: IntentConfigData,
    subscribers: HashMap<usize, Recipient<IntentConfigData>>,
    next_id: usize,
}

impl IntentConfigActor {
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
}
