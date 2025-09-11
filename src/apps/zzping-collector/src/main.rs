//! Main entry point for the zzping collector.
//!
//! This binary is a simple wrapper around the `run` function
//! in the library crate, which contains the actual application logic.

use anyhow::Result;

/// Main entry point for the zzping collector executable.
#[tokio::main]
async fn main() -> Result<()> {
    zzping_collector::run().await
}
