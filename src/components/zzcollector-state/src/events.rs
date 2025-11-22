//! Event definitions for CState component
//!
//! Events are published by the MainActor when state changes occur.
//! NetworkActors subscribe to these events and forward them to their peers.

use actix::Message;

/// Events published by CStateActor
#[derive(Clone, Debug, Message)]
#[rtype(result = "()")]
pub enum CStateEvent {
    /// Heartbeat should be sent to all connected peers
    HeartbeatTick {
        /// The collector ID
        collector_id: String,
        /// Uptime in seconds
        uptime_secs: u64,
        /// Total pings sent
        pings_sent: u64,
        /// Total pings received
        pings_received: u64,
        /// Total batches sent
        batches_sent: u64,
        /// Last config update timestamp
        last_config_update_ms: u64,
        /// Connection nonce for this session
        connection_nonce: u64,
    },
}
