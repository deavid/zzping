//! Binary entrypoint for the database service.
//!
//! Starts the background runner that ingests and persists ping data.
use anyhow::Result;
use zzping_old_database::runner::run;

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}
