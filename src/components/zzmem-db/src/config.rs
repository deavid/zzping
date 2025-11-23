//! Configuration for MemDB component.
//!
//! Replaces the deprecated `MemDBRole` enum with fine-grained configuration
//! properties that express *what* the component does rather than *who* it is.

use std::path::PathBuf;

/// Fine-grained configuration for MemDB actor.
///
/// Instead of thinking in terms of roles (Database vs. Collector), we configure
/// the component with specific capabilities and storage behaviors:
/// - How many results to buffer before action?
/// - Should it accept batches from the network?
/// - Should it provide query interface?
/// - Should it persist to disk?
///
/// This follows SOLID principles: components know about their own capabilities,
/// not about application-level roles.
#[derive(Clone, Debug, PartialEq)]
pub struct MemDBConfig {
    /// Maximum results to buffer before taking action.
    ///
    /// For collectors: number of local results to batch before sending.
    /// For databases: not directly used (storage limit uses max_results_per_target).
    pub buffer_size: usize,

    /// Maximum number of results to store per target in the database.
    ///
    /// 0 = unlimited. Only meaningful when `accept_batches` is true.
    pub max_results_per_target: usize,

    /// Path for persistence (None = in-memory only).
    pub persistence_path: Option<PathBuf>,

    /// Whether to accept batch submissions from the network.
    ///
    /// When true, the actor processes `SubmitBatch` messages.
    pub accept_batches: bool,

    /// Whether to provide query interface.
    ///
    /// When true, the actor processes `Query` messages and returns results.
    pub allow_queries: bool,
}

impl MemDBConfig {
    /// Create a configuration for the database role:
    /// - Accepts batches from network
    /// - Provides query interface
    /// - Can persist to disk
    /// - Has result storage limits
    pub fn for_database(max_results_per_target: usize, persistence_path: Option<PathBuf>) -> Self {
        Self {
            buffer_size: 0,
            max_results_per_target,
            persistence_path,
            accept_batches: true,
            allow_queries: true,
        }
    }

    /// Create a configuration for the collector role:
    /// - Buffers local results before sending
    /// - Does not accept batches
    /// - Does not provide queries
    /// - No persistence
    pub fn for_collector(buffer_size: usize) -> Self {
        Self {
            buffer_size,
            max_results_per_target: 0,
            persistence_path: None,
            accept_batches: false,
            allow_queries: false,
        }
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.accept_batches && self.max_results_per_target == 0 {
            // For batches, unlimited results might be intentional (e.g., development/testing)
            // So we allow it
        }
        Ok(())
    }
}

impl Default for MemDBConfig {
    /// Default to collector configuration (safe, no batch acceptance).
    fn default() -> Self {
        Self::for_collector(1000)
    }
}
