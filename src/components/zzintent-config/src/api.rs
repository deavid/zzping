//! Defines the public API trait for interacting with a running IntentConfigActor.

use crate::actor::IntentConfigActor;
use crate::messages::{GetCurrentConfig, IntentConfigData, Subscribe, Unsubscribe, UpdateConfig};
use actix::prelude::*;
use anyhow::{Result, anyhow};

/// The public API for the IntentConfig component.
///
/// This trait provides a clean, method-based interface for interacting with the
/// actor, hiding the underlying message types from the consumer.
#[async_trait::async_trait]
pub trait IntentConfigApi {
    /// Updates the configuration. This is a "tell" (fire-and-forget) operation.
    fn update_config(&self, config: IntentConfigData);

    /// Gets the current configuration state. This is an "ask" operation.
    async fn get_current_config(&self) -> Result<IntentConfigData>;

    /// Subscribes to configuration updates. This is an "ask" operation that
    /// returns a unique subscription ID for later unsubscribing.
    async fn subscribe(&self, recipient: Recipient<IntentConfigData>) -> Result<usize>;

    /// Unsubscribes from configuration updates using a subscription ID.
    fn unsubscribe(&self, id: usize);
}

/// Implements the public API for the actor's handle (`Addr`).
/// This is where we translate the clean method calls into actual Actix messages.
#[async_trait::async_trait]
impl IntentConfigApi for Addr<IntentConfigActor> {
    /// Submits a configuration update asynchronously (fire-and-forget).
    fn update_config(&self, config: IntentConfigData) {
        // `do_send` is used for "tell" patterns where no response is needed.
        self.do_send(UpdateConfig {
            data: config,
            peer_id: None,
        });
    }

    /// Requests the current configuration state, awaiting the response.
    async fn get_current_config(&self) -> Result<IntentConfigData> {
        // `send` is used for "ask" patterns. It returns a Future that resolves
        // with the result from the actor's handler.
        self.send(GetCurrentConfig)
            .await
            .map_err(|e| anyhow!("Failed to send GetCurrentConfig message: {}", e))
    }

    /// Registers a recipient for future updates. Returns a subscription ID.
    async fn subscribe(&self, recipient: Recipient<IntentConfigData>) -> Result<usize> {
        // `send` is used for "ask" patterns. It returns a Future that resolves
        // with the result from the actor's handler.
        self.send(Subscribe { recipient })
            .await
            .map_err(|e| anyhow!("Failed to send Subscribe message: {}", e))
    }

    /// Cancels a subscription using its ID.
    fn unsubscribe(&self, id: usize) {
        self.do_send(Unsubscribe(id));
    }
}
