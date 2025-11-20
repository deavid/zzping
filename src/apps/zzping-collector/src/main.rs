//! ZZPing Collector Application - Built with zznet-builder
//!
//! Fully trait-based service architecture using ZZNetApplication and AppHarness.
//! Reduces main.rs from 75 lines to ~30 lines!

use clap::Parser;
use zznet_builder::harness::AppHarness;
use zzping_collector::config::CollectorConfig;
use zzping_collector::service::CollectorApp;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to collector configuration file
    #[arg(short, long, default_value = "collector.ron")]
    config: String,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config_path = &args.config;
    let debug = args.debug;

    let log_level = if debug { "debug" } else { "info" };
    let harness = AppHarness::new().log_level(log_level);
    harness.init_logging();

    let config_content = std::fs::read_to_string(config_path)?;
    let config: CollectorConfig = ron::from_str(&config_content)?;

    let intent_builder =
        zzintent_config::builder::IntentConfigBuilder::new().config_for_collector();
    let memdb_builder = zzmem_db::builder::MemDBBuilder::new(
        zzmem_db::config::MemDBConfig::for_collector(config.components.memdb_batch_size),
    );

    let pinger_builder = zzpinger::builder::PingerBuilder {
        clock: None,
        spawn_strategy: zzpinger::builder::SpawnStrategy::NewArbiter,
    };

    let app = CollectorApp::new(config, pinger_builder, memdb_builder, intent_builder);

    harness.run(app)
}
