//! Collector application library.
//!
//! Contains testable business logic separated from main() entry point.
//! This allows unit testing of configuration, service orchestration, and
//! component integration without running the full binary.

// Module declarations
pub mod cli;
pub mod config;
pub mod error;
pub mod service;

// Re-exports for convenience
pub use cli::CliArgs;
pub use config::CollectorConfig;
pub use error::CollectorError;
pub use service::CollectorService;
