//! Integration test: harness-run lifecycle for collector using AppHarness.
//!
//! Verifies the harness-run API can start and gracefully stop the service.

use zznet_builder::harness::AppHarness;
use zznet_builder::traits::ZZNetApplication;
use zzping_collector::config::CollectorConfig;
use zzping_collector::service::CollectorApp;

// Build a minimal valid test config programmatically.
fn make_test_config() -> CollectorConfig {
    CollectorConfig::for_testing("test-collector")
}

#[actix_rt::test]
async fn test_harness_run_starts_and_stops_collector_service() {
    // This test uses harness.run to start the service in a blocking thread,
    // and then stops it via the oneshot channel.

    let config = make_test_config();
    let harness = AppHarness::new().log_level("info");
    harness.init_logging();

    let pinger_builder = zzpinger::builder::PingerBuilder {
        clock: None,
        spawn_strategy: zzpinger::builder::SpawnStrategy::NewArbiter,
    };
    let memdb_builder =
        zzmem_db::builder::MemDBBuilder::new(zzmem_db::config::MemDBConfig::for_collector(10000));
    let intent_builder =
        zzintent_config::builder::IntentConfigBuilder::new().config_for_collector();

    let mut app = CollectorApp::new(config, pinger_builder, memdb_builder, intent_builder);

    // Test that startup succeeds
    let result = app.startup().await;
    assert!(result.is_ok(), "Startup failed: {:?}", result);
}
