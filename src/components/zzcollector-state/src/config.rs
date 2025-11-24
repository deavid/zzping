//! Configuration for CState component.
//!
//! Replaces the deprecated `CStateRole` enum with fine-grained configuration
//! properties that express *what* the component does rather than *who* it is.

/// Fine-grained configuration for CState actor.
///
/// Instead of thinking in terms of roles (Collector, Database, Admin), we configure
/// the component with specific capabilities:
/// - Should it send heartbeats?
/// - Should it track collector states?
/// - Should it provide query interface?
///
/// This follows SOLID principles: components know about their own capabilities,
/// not about application-level roles.
#[derive(Clone, Debug, PartialEq)]
pub struct CStateConfig {
    /// If set, send heartbeats with this collector ID.
    ///
    /// The actor will periodically send heartbeat messages with this identifier
    /// so the database can track its health.
    pub collector_id: Option<String>,

    /// Heartbeat interval in milliseconds.
    ///
    /// Only used when `collector_id` is Some. Determines how often to send heartbeats.
    pub heartbeat_interval_ms: u64,

    /// Whether to track collector states.
    ///
    /// When true, the actor receives heartbeat messages from collectors
    /// and maintains state about their health.
    pub track_collectors: bool,

    /// Timeout in milliseconds for marking collectors as stale.
    ///
    /// Only used when `track_collectors` is true.
    pub stale_timeout_ms: u64,

    /// Maximum number of collectors to track (None = unlimited).
    ///
    /// Only used when `track_collectors` is true.
    pub max_collectors: Option<usize>,

    /// Whether to provide query interface for collector state.
    ///
    /// When true, the actor processes `Query` messages about collector states.
    pub allow_queries: bool,
}

impl CStateConfig {
    /// Create a configuration for the collector role:
    /// - Sends heartbeats with given ID
    /// - Does not track other collectors
    /// - Does not provide queries
    pub fn for_collector(collector_id: String, heartbeat_interval_ms: u64) -> Self {
        Self {
            collector_id: Some(collector_id),
            heartbeat_interval_ms,
            track_collectors: false,
            stale_timeout_ms: 0,
            max_collectors: None,
            allow_queries: false,
        }
    }

    /// Create a configuration for the database role:
    /// - Does not send heartbeats
    /// - Tracks collector states
    /// - Provides query interface
    pub fn for_database(stale_timeout_ms: u64, max_collectors: Option<usize>) -> Self {
        Self {
            collector_id: None,
            heartbeat_interval_ms: 0,
            track_collectors: true,
            stale_timeout_ms,
            max_collectors,
            allow_queries: true,
        }
    }

    /// Create a configuration for an admin/query-only role:
    /// - Does not send heartbeats
    /// - Does not track
    /// - Provides query interface
    pub fn for_admin() -> Self {
        Self {
            collector_id: None,
            heartbeat_interval_ms: 0,
            track_collectors: false,
            stale_timeout_ms: 0,
            max_collectors: None,
            allow_queries: true,
        }
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.collector_id.is_some() && self.heartbeat_interval_ms == 0 {
            return Err("collector_id is set but heartbeat_interval_ms is 0".to_string());
        }
        if self.track_collectors && self.stale_timeout_ms == 0 {
            return Err("track_collectors is true but stale_timeout_ms is 0".to_string());
        }
        Ok(())
    }
}

impl Default for CStateConfig {
    /// Default to admin configuration (safe, query-only).
    fn default() -> Self {
        Self::for_admin()
    }
}
