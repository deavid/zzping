//! Configuration for the TCP lock component.

use serde::{Deserialize, Serialize};

/// Enum defining the locking strategy: either real TCP sockets or in-memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LockStrategy {
    /// Production Mode: Binds a real TCP port.
    /// Value is the bind address (e.g., "0.0.0.0:7879").
    Tcp(String),

    /// Test Mode: Uses an in-memory atomic registry.
    /// Value is an arbitrary ID (e.g., "lock-1").
    Memory(String),
}

/// Configuration for the TCP lock actor.
#[derive(Debug, Clone)]
pub struct TcpLockConfig {
    /// How to lock: TCP socket or in-memory registry.
    pub strategy: LockStrategy,
    /// How often to retry acquisition (in milliseconds).
    pub retry_interval_ms: u64,
}

impl TcpLockConfig {
    /// Create a TCP-based lock configuration for production use.
    pub fn tcp(bind_addr: String, retry_interval_ms: u64) -> Self {
        Self {
            strategy: LockStrategy::Tcp(bind_addr),
            retry_interval_ms,
        }
    }

    /// Create a memory-based lock configuration for testing.
    pub fn memory(lock_id: String, retry_interval_ms: u64) -> Self {
        Self {
            strategy: LockStrategy::Memory(lock_id),
            retry_interval_ms,
        }
    }
}

impl Default for TcpLockConfig {
    fn default() -> Self {
        Self {
            strategy: LockStrategy::Tcp("127.0.0.1:7879".to_string()),
            retry_interval_ms: 1000,
        }
    }
}
