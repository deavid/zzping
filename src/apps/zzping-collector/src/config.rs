//! Configuration structures and loading.

use anyhow::Result;
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

impl CollectorTlsConfig {
    /// Converts this configuration into the transport layer's TLS configuration.
    pub fn to_transport_config(&self) -> Result<zznet_transport_tcp::TlsConfig> {
        Ok(zznet_transport_tcp::TlsConfig::new(
            &self.client_cert_path,
            &self.client_key_path,
            Some(&self.ca_cert_path),
            "zzping-mesh".into(),
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Selects the pinger implementation.
pub enum PingerBackend {
    /// Use real ICMP ping via surge_ping (requires raw socket permissions)
    Real,
    /// Use mock pinger for testing (no network operations)
    Mock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Configuration for internal components.
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
    /// Creates configuration with millisecond intervals for unit testing.
    pub fn fast_timing() -> Self {
        Self {
            heartbeat_interval_ms: 100,
            memdb_batch_size: 5,
            pinger_backend: PingerBackend::Mock,
        }
    }
}

impl CollectorConfig {
    /// Creates a minimal TCP-only configuration for testing.
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
