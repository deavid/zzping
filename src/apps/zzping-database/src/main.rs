//! ZZPing Database Application - Built with zznet-builder
//!
//! Fully trait-based service architecture using ZZNetService and ZZNetConfig.
//! Reduces main.rs from 75 lines to 12 lines!

use anyhow::Result;
use zznet_builder::builder::AppBuilder;
use zzping_database::service::DatabaseService;

fn main() -> Result<()> {
    AppBuilder::new("ZZPing Database", env!("CARGO_PKG_VERSION"))
        .with_default_config("database.ron")
        .run_service::<DatabaseService>()
}
