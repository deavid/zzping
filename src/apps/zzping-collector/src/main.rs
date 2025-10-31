//! ZZPing Collector Application - Built with zznet-builder
//!
//! Fully trait-based service architecture using ZZNetService and ZZNetConfig.
//! Reduces main.rs from 75 lines to 12 lines!

use anyhow::Result;
use zznet_builder::builder::AppBuilder;
use zzping_collector::service::CollectorService;

fn main() -> Result<()> {
    AppBuilder::new("ZZPing Collector", env!("CARGO_PKG_VERSION"))
        .with_default_config("collector.ron")
        .run_service::<CollectorService>()
}
