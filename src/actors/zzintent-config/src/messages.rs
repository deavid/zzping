//! Defines the message types that the IntentConfigActor can handle.

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// The core data structure holding the configuration state.
/// This is also used as a broadcast message to subscribers.
#[derive(Message, Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[rtype(result = "()")]
pub struct IntentConfigData {
    pub targets: Vec<IpAddr>,
    pub ping_rate_pps: u64,
}

/// A command message sent to the actor to update the configuration.
#[derive(Message)]
#[rtype(result = "()")]
pub struct UpdateConfig(pub IntentConfigData);

/// A command message for another actor to subscribe to config updates.
/// The recipient's address for receiving broadcasts is included.
#[derive(Message, Hash, PartialEq, Eq)]
#[rtype(result = "usize")] // Returns the subscription ID
pub struct Subscribe {
    pub recipient: Recipient<IntentConfigData>,
}

/// A command message to unsubscribe from config updates using a subscription ID.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Unsubscribe(pub usize);
