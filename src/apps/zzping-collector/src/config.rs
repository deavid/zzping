//! Configuration structures and loading.

use serde::{Deserialize, Serialize};

/// Collector application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorConfig {
    /// Unique identifier for this collector instance
    pub collector_id: String,

    /// Database connection settings
    pub database_host: String,
    /// Database connection port.
    pub database_port: u16,

    /// TLS configuration for mTLS connection (optional for TCP-only mode)
    pub tls: Option<CollectorTlsConfig>,

    /// Component-specific settings
    pub components: ComponentConfig,

    /// Delay in milliseconds between reconnection attempts (default: 5000ms)
    #[serde(default = "default_reconnect_delay_ms")]
    pub reconnect_delay_ms: u64,
}

fn default_reconnect_delay_ms() -> u64 {
    5000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// TLS configuration for mTLS.
pub struct CollectorTlsConfig {
    /// CA certificate for verifying server (database)
    pub ca_cert_path: String,
    /// Client certificate (this collector's identity)
    pub client_cert_path: String,
    /// Client private key
    pub client_key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Type of pinger backend to use.
pub enum PingerBackend {
    /// Use real ICMP ping via surge_ping (requires raw socket permissions)
    Real,
    /// Use mock pinger for testing (no network operations)
    Mock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Component-specific configuration.
pub struct ComponentConfig {
    /// Heartbeat interval in milliseconds for collector state
    pub heartbeat_interval_ms: u64,

    /// Batch size for mem-db
    pub memdb_batch_size: usize,

    /// Type of pinger backend to use
    #[serde(default = "default_pinger_backend")]
    pub pinger_backend: PingerBackend,
}

fn default_pinger_backend() -> PingerBackend {
    PingerBackend::Real
}

impl ComponentConfig {
    /// Create component configuration with faster timing suitable for testing/demos.
    ///
    /// Uses shorter intervals than production defaults:
    /// - Heartbeat: 100ms instead of 5000ms
    /// - Batch size: 5 instead of 50
    /// - Mock pinger backend for testing
    pub fn fast_timing() -> Self {
        Self {
            heartbeat_interval_ms: 100,
            memdb_batch_size: 5,
            pinger_backend: PingerBackend::Mock,
        }
    }
}

impl CollectorConfig {
    /// Create a minimal configuration suitable for testing, demos, or development.
    ///
    /// This configuration uses:
    /// - TCP-only (no TLS)
    /// - localhost database connection
    /// - Fast timing intervals for testing
    /// - Minimal resource usage
    pub fn for_testing(collector_id: impl Into<String>) -> Self {
        Self {
            collector_id: collector_id.into(),
            database_host: "127.0.0.1".into(),
            database_port: 58443,
            tls: None, // TCP-only
            components: ComponentConfig::fast_timing(),
            reconnect_delay_ms: 100, // Fast reconnect for testing
        }
    }
}
