//! Event definitions for IntentConfig component
//!
//! Events are published by the MainActor when state changes occur.
//! NetworkActors subscribe to these events and forward them to their peers.
//!
//! This provides a zero-maintenance, RAII-based pub/sub mechanism that doesn't
//! require tracking subscribers or manual cleanup.

use crate::messages::IntentConfigData;
use actix::Message;

/// Events published by IntentConfigActor
#[derive(Clone, Debug, Message)]
#[rtype(result = "()")]
pub enum IntentConfigEvent {
    /// Configuration has changed and should be sent to all peers
    ConfigChanged(IntentConfigData),
}
