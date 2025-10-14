//! Database application library.
//!
//! Contains testable business logic separated from main() entry point.
//! This allows unit testing of configuration, service orchestration, and
//! component integration without running the full binary.

// Module declarations
pub mod cli;
pub mod config;
pub mod error;
pub mod service;

// Re-exports for convenience (acceptable for binary crates)
pub use cli::CliArgs;
pub use config::DatabaseConfig;
pub use error::DatabaseError;
pub use service::DatabaseService;
