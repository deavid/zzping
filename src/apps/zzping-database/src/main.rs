//! ZZPing Database Application - Built with zznet-builder
//!
//! Fully trait-based service architecture using ZZNetApplication and AppHarness.
//! Reduces main.rs from 75 lines to ~30 lines!

use clap::Parser;
use zznet_builder::harness::AppHarness;
use zzping_database::config::DatabaseConfig;
use zzping_database::service::DatabaseApp;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to database configuration file
    #[arg(short, long, default_value = "database.ron")]
    config: String,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config_path = &args.config;
    let debug = args.debug;

    // 1. Setup Harness
    let log_level = if debug { "debug" } else { "info" };
    let harness = AppHarness::new().log_level(log_level);
    harness.init_logging();

    // 2. Load Config (Application Responsibility)
    let config_content = std::fs::read_to_string(config_path)?;
    let config: DatabaseConfig = ron::from_str(&config_content)?;

    // 3. Construct Dependencies (Builders)
    let data_dir = std::path::PathBuf::from(&config.data_dir);
    let config_path = data_dir.join("intent.ron");

    let intent_builder =
        zzintent_config::builder::IntentConfigBuilder::new().config_for_database(config_path);
    let memdb_builder = zzmem_db::builder::MemDBBuilder::new(
        zzmem_db::config::MemDBConfig::for_database(10000, None),
    );
    let cstate_builder = zzcollector_state::builder::CStateBuilder::new(
        zzcollector_state::config::CStateConfig::for_database(
            config.components.stale_timeout_secs,
            Some(config.components.max_collectors),
        ),
    );

    // 4. Create App
    let app = DatabaseApp::new(config, intent_builder, memdb_builder, cstate_builder);

    // 5. Run
    harness.run(app)
}
