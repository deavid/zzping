//! Logging initialization utilities.

use tracing_subscriber::EnvFilter;

/// Initialize the global `tracing` subscriber with the provided level.
///
/// Panics if a global subscriber has already been set.
pub fn init_logging(level: &str) {
    let filter = EnvFilter::new(level);

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .try_init();
}

/// Initialize logging using CLI args.
pub fn init_logging_from_args(args: &crate::cli::StandardCliArgs) {
    init_logging(args.log_level());
}
