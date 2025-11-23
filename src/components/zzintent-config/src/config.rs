//! Configuration for IntentConfig component.
//!
//! Replaces the deprecated `IntentConfigRole` enum with fine-grained configuration
//! properties that express *what* the component does rather than *who* it is.

use std::path::PathBuf;

/// Fine-grained configuration for IntentConfig actor.
///
/// Instead of thinking in terms of roles (Database vs. Collector), we configure
/// the component with specific capabilities and behaviors:
/// - Should it persist configuration to disk?
/// - Should it accept configuration change requests?
/// - Should it broadcast updates to peers?
///
/// This follows SOLID principles: components know about their own capabilities,
/// not about application-level roles.
#[derive(Clone, Debug, PartialEq)]
pub struct IntentConfigConfig {
    /// Whether to persist configuration to disk.
    ///
    /// When true, the actor will load initial configuration from `config_file_path`
    /// and persist any updates to that file.
    pub persist_config: bool,

    /// Path to the configuration file (only meaningful if `persist_config` is true).
    pub config_file_path: Option<PathBuf>,

    /// Whether to accept configuration change requests from peers.
    ///
    /// When true, the actor processes `RequestConfigChange` messages
    /// and updates its internal state.
    pub accept_config_changes: bool,

    /// Whether to send configuration updates to peers.
    ///
    /// When true, the actor broadcasts configuration updates to all connected peers.
    pub broadcast_config_updates: bool,
}

impl IntentConfigConfig {
    /// Create a configuration for the database role:
    /// - Persists config to disk
    /// - Accepts config changes
    /// - Broadcasts updates
    pub fn for_database(config_file_path: PathBuf) -> Self {
        Self {
            persist_config: true,
            config_file_path: Some(config_file_path),
            accept_config_changes: true,
            broadcast_config_updates: true,
        }
    }

    /// Create a configuration for the collector role:
    /// - Does not persist
    /// - Does not accept changes
    /// - Does not broadcast
    pub fn for_collector() -> Self {
        Self {
            persist_config: false,
            config_file_path: None,
            accept_config_changes: false,
            broadcast_config_updates: false,
        }
    }

    /// Validate the configuration.
    ///
    /// Returns an error if the configuration is invalid (e.g., persist_config is true
    /// but config_file_path is None).
    pub fn validate(&self) -> Result<(), String> {
        if self.persist_config && self.config_file_path.is_none() {
            return Err("persist_config is true but config_file_path is None".to_string());
        }
        if self.persist_config {
            let path = self.config_file_path.as_ref().unwrap();
            if path.as_os_str().is_empty() {
                return Err("config_file_path is empty".to_string());
            }
        }
        Ok(())
    }
}

impl Default for IntentConfigConfig {
    /// Default to collector configuration (safe, no persistence required).
    fn default() -> Self {
        Self::for_collector()
    }
}
