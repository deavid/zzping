//! Logging initialization for ZZNet applications.
//!
//! This module provides standardized logging setup using `tracing` and `tracing-subscriber`.

use tracing_subscriber::EnvFilter;

/// Initialize logging with the specified level.
///
/// This sets up a `tracing_subscriber` with:
/// - The specified log level filter
/// - Target display (module paths)
/// - Thread IDs
/// - Line numbers
///
/// # Panics
///
/// Panics if a global subscriber has already been set.
///
/// # Example
///
/// ```rust,no_run
/// use zznet_builder::logging::init_logging;
///
/// // Initialize with info level
/// init_logging("info");
/// tracing::info!("Application started");
/// ```
pub fn init_logging(level: &str) {
    let filter = EnvFilter::new(level);

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .init();
}

/// Initialize logging based on CLI arguments.
///
/// This is a convenience wrapper around `init_logging` that extracts
/// the log level from `StandardCliArgs`.
///
/// # Example
///
/// ```rust,no_run
/// use clap::Parser;
/// use zznet_builder::cli::StandardCliArgs;
/// use zznet_builder::logging::init_logging_from_args;
///
/// let args = StandardCliArgs::parse();
/// init_logging_from_args(&args);
/// ```
pub fn init_logging_from_args(args: &crate::cli::StandardCliArgs) {
    init_logging(args.log_level());
}
