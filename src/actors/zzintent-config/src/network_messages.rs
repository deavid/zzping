//! Network protocol messages for IntentConfig component
//!
//! This module defines the typed messages exchanged between IntentConfig actors
//! across the network via ZZNet SessionManager. These messages enable bidirectional
//! communication for configuration management.
//!
//! # Protocol Design
//!
//! The protocol supports two roles:
//! - **Collector**: Reads config from disk, broadcasts updates
//! - **Database**: Receives updates, can query for current state
//!
//! # Message Flow Examples
//!
//! ## Config Update Flow
//! ```text
//! File Change → Collector
//!     ↓ ConfigUpdate
//! SessionManager (room: "intentconfig")
//!     ↓ ConfigUpdate
//! Database → broadcast to local subscribers
//! ```
//!
//! ## Query Flow
//! ```text
//! Database startup
//!     ↓ QueryCurrentConfig
//! SessionManager (room: "intentconfig")
//!     ↓ QueryCurrentConfig
//! Collector
//!     ↓ CurrentConfig
//! SessionManager (room: "intentconfig")
//!     ↓ CurrentConfig
//! Database → update local state
//! ```

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Network protocol messages for IntentConfig component
///
/// These messages are exchanged between Collector and Database processes
/// via ZZNet SessionManager over the "intentconfig" room.
///
/// All messages are strongly typed and transport-agnostic - they never
/// touch bytes or serialization at this layer.
#[derive(Clone, Debug, Message, Serialize, Deserialize, PartialEq)]
#[rtype(result = "()")]
pub enum IntentConfigMessage {
    /// Configuration update from Collector to Database
    ///
    /// Sent whenever the Collector detects a configuration change (e.g., from file).
    /// Database receives this and:
    /// 1. Updates its local state
    /// 2. Broadcasts to local subscribers (MemDB, etc.)
    ///
    /// # Example
    /// ```ignore
    /// IntentConfigMessage::ConfigUpdate {
    ///     targets: vec!["8.8.8.8".parse().unwrap()],
    ///     ping_rate_pps: 100,
    /// }
    /// ```
    ConfigUpdate {
        /// List of IP addresses to ping
        targets: Vec<IpAddr>,
        /// Ping rate in packets per second
        ping_rate_pps: u64,
    },

    /// Request for current configuration from Database to Collector
    ///
    /// Sent by Database when it starts up or wants to refresh its state.
    /// Collector responds with CurrentConfig message.
    ///
    /// # Example
    /// ```ignore
    /// IntentConfigMessage::QueryCurrentConfig
    /// ```
    QueryCurrentConfig,

    /// Response with current configuration from Collector to Database
    ///
    /// Sent in response to QueryCurrentConfig. Contains the Collector's
    /// current configuration state.
    ///
    /// # Example
    /// ```ignore
    /// IntentConfigMessage::CurrentConfig {
    ///     targets: vec!["8.8.8.8".parse().unwrap()],
    ///     ping_rate_pps: 100,
    /// }
    /// ```
    CurrentConfig {
        /// List of IP addresses to ping
        targets: Vec<IpAddr>,
        /// Ping rate in packets per second
        ping_rate_pps: u64,
    },

    /// Heartbeat message (optional, for future use)
    ///
    /// Can be used to verify connection is alive and detect disconnections.
    /// Not yet implemented in handlers, but defined for future use.
    ///
    /// # Example
    /// ```ignore
    /// IntentConfigMessage::Heartbeat
    /// ```
    Heartbeat,

    /// Error response (optional, for future use)
    ///
    /// Sent when an error occurs processing a request. Can be used for
    /// error reporting and debugging.
    ///
    /// # Example
    /// ```ignore
    /// IntentConfigMessage::Error {
    ///     reason: "Invalid configuration format".to_string(),
    /// }
    /// ```
    Error {
        /// Human-readable error description
        reason: String,
    },
}

impl IntentConfigMessage {
    /// Helper to create a ConfigUpdate message
    pub fn config_update(targets: Vec<IpAddr>, ping_rate_pps: u64) -> Self {
        Self::ConfigUpdate {
            targets,
            ping_rate_pps,
        }
    }

    /// Helper to create a CurrentConfig message
    pub fn current_config(targets: Vec<IpAddr>, ping_rate_pps: u64) -> Self {
        Self::CurrentConfig {
            targets,
            ping_rate_pps,
        }
    }

    /// Helper to create an Error message
    pub fn error(reason: impl Into<String>) -> Self {
        Self::Error {
            reason: reason.into(),
        }
    }

    /// Check if this message contains configuration data
    pub fn has_config_data(&self) -> bool {
        matches!(self, Self::ConfigUpdate { .. } | Self::CurrentConfig { .. })
    }

    /// Extract config data if this is a ConfigUpdate or CurrentConfig message
    pub fn config_data(&self) -> Option<(Vec<IpAddr>, u64)> {
        match self {
            Self::ConfigUpdate {
                targets,
                ping_rate_pps,
            }
            | Self::CurrentConfig {
                targets,
                ping_rate_pps,
            } => Some((targets.clone(), *ping_rate_pps)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_update_creation() {
        let targets = vec!["8.8.8.8".parse().unwrap(), "1.1.1.1".parse().unwrap()];
        let msg = IntentConfigMessage::config_update(targets.clone(), 100);

        match msg {
            IntentConfigMessage::ConfigUpdate {
                targets: t,
                ping_rate_pps,
            } => {
                assert_eq!(t, targets);
                assert_eq!(ping_rate_pps, 100);
            }
            _ => panic!("Expected ConfigUpdate"),
        }
    }

    #[test]
    fn test_current_config_creation() {
        let targets = vec!["8.8.8.8".parse().unwrap()];
        let msg = IntentConfigMessage::current_config(targets.clone(), 50);

        match msg {
            IntentConfigMessage::CurrentConfig {
                targets: t,
                ping_rate_pps,
            } => {
                assert_eq!(t, targets);
                assert_eq!(ping_rate_pps, 50);
            }
            _ => panic!("Expected CurrentConfig"),
        }
    }

    #[test]
    fn test_error_creation() {
        let msg = IntentConfigMessage::error("Test error");
        match msg {
            IntentConfigMessage::Error { reason } => {
                assert_eq!(reason, "Test error");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_has_config_data() {
        let update = IntentConfigMessage::config_update(vec![], 10);
        assert!(update.has_config_data());

        let current = IntentConfigMessage::current_config(vec![], 10);
        assert!(current.has_config_data());

        let query = IntentConfigMessage::QueryCurrentConfig;
        assert!(!query.has_config_data());

        let heartbeat = IntentConfigMessage::Heartbeat;
        assert!(!heartbeat.has_config_data());

        let error = IntentConfigMessage::error("test");
        assert!(!error.has_config_data());
    }

    #[test]
    fn test_config_data_extraction() {
        let targets = vec!["8.8.8.8".parse().unwrap()];
        let update = IntentConfigMessage::config_update(targets.clone(), 100);

        let (extracted_targets, rate) = update.config_data().unwrap();
        assert_eq!(extracted_targets, targets);
        assert_eq!(rate, 100);

        let query = IntentConfigMessage::QueryCurrentConfig;
        assert!(query.config_data().is_none());
    }

    #[test]
    fn test_message_equality() {
        let msg1 = IntentConfigMessage::config_update(vec!["8.8.8.8".parse().unwrap()], 100);
        let msg2 = IntentConfigMessage::config_update(vec!["8.8.8.8".parse().unwrap()], 100);
        let msg3 = IntentConfigMessage::config_update(vec!["1.1.1.1".parse().unwrap()], 100);

        assert_eq!(msg1, msg2);
        assert_ne!(msg1, msg3);
    }

    #[test]
    fn test_query_and_heartbeat() {
        let query = IntentConfigMessage::QueryCurrentConfig;
        let heartbeat = IntentConfigMessage::Heartbeat;

        // These should be creatable and comparable
        assert_eq!(query, IntentConfigMessage::QueryCurrentConfig);
        assert_eq!(heartbeat, IntentConfigMessage::Heartbeat);
        assert_ne!(query, heartbeat);
    }
}
