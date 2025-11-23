//! Defines the message types that the IntentConfigActor can handle.

use crate::events::IntentConfigEvent;
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// The core data structure holding the configuration state.
/// This is also used as a broadcast message to subscribers.
#[derive(Message, Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[rtype(result = "()")]
pub struct IntentConfigData {
    /// IP targets to ping.
    pub targets: Vec<IpAddr>,
    /// Ping rate in packets per second.
    pub ping_rate_pps: u64,
}

impl IntentConfigData {
    /// Validate that the configuration is reasonable for operation.
    ///
    /// Returns an error if the configuration is invalid.
    pub fn validate(&self) -> Result<(), String> {
        // Ping rate must be positive
        if self.ping_rate_pps == 0 {
            return Err("ping_rate_pps must be greater than 0".to_string());
        }

        // Should have at least one target
        if self.targets.is_empty() {
            return Err("targets list cannot be empty".to_string());
        }

        // Ping rate shouldn't be unreasonably high (basic sanity check)
        if self.ping_rate_pps > 1000000 {
            return Err("ping_rate_pps seems unreasonably high (> 1M pps)".to_string());
        }

        Ok(())
    }
}

/// A command message sent to the actor to update the configuration.
/// Includes the peer ID that requested the config change.
#[derive(Message)]
#[rtype(result = "()")]
pub struct UpdateConfig {
    /// The new configuration data
    pub data: IntentConfigData,
    /// The ID of the peer that requested the change
    pub peer_id: Option<u64>,
}

/// A command message for another actor to subscribe to config updates.
/// The recipient's address for receiving broadcasts is included.
#[derive(Message, Hash, PartialEq, Eq)]
#[rtype(result = "usize")] // Returns the subscription ID
pub struct Subscribe {
    /// Address to receive configuration update broadcasts.
    pub recipient: Recipient<IntentConfigData>,
}

/// A command message to unsubscribe from config updates using a subscription ID.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Unsubscribe(pub usize);

/// A command message to get the current configuration state.
#[derive(Message)]
#[rtype(result = "IntentConfigData")]
pub struct GetCurrentConfig;

/// A command message to get the event bus sender for networking
#[derive(Message)]
#[rtype(result = "tokio::sync::broadcast::Sender<IntentConfigEvent>")]
pub struct GetEventBus;

/// Health information for the IntentConfigActor.
#[derive(Message, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[rtype(result = "IntentConfigHealth")]
pub struct GetHealth;

/// A compact health struct exposing basic counters and last activity.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct IntentConfigHealth {
    /// Number of known subscribers
    pub subscriber_count: usize,
    /// Number of successful broadcasts attempted since actor start
    pub successful_broadcasts: u64,
    /// Number of broadcast failures observed since actor start
    pub failed_broadcasts: u64,
    /// Timestamp (unix millis) of last broadcast attempt, 0 if none
    pub last_broadcast_ms: u128,
}
