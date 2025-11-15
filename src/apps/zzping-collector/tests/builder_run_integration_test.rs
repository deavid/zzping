//! Integration test: builder-run lifecycle for collector using AppBuilder.
//!
//! Verifies the builder-run `run_service_with_config_and_stop` API can start and gracefully stop the service.

use std::time::Duration;

use zznet_builder::builder::AppBuilder;
use zzping_collector::config::CollectorConfig;
use zzping_collector::service::CollectorService;

// Build a minimal valid test config programmatically.
fn make_test_config() -> CollectorConfig {
    CollectorConfig::for_testing("test-collector")
}

#[test]
fn test_builder_run_starts_and_stops_collector_service() {
    // This test uses builder.run_service_with_config_and_stop to start the service in a blocking thread,
    // and then stops it via the oneshot channel.

    let config = make_test_config();
    let builder = AppBuilder::new("ZZPing Collector", env!("CARGO_PKG_VERSION"))
        .with_default_config("collector.ron");

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    let cfg_clone = config.clone();

    // Run in a blocking thread to avoid creating nested Tokio Runtime
    let handle = std::thread::spawn(move || {
        // Use run_service_with_config_and_stop to manage lifetime
        let result = builder.run_service_with_config_and_stop::<CollectorService, _>(
            cfg_clone,
            async move {
                let _ = stop_rx.await;
            },
        );

        // If run_service_with_config_and_stop returns Err, panic in thread so test fails
        if let Err(e) = result {
            panic!("Builder run failed: {}", e);
        }
    });

    // Let the service run a very short while (builder is expected to return quickly)
    let start = std::time::Instant::now();
    std::thread::sleep(Duration::from_millis(10));

    // Request stop
    let _ = stop_tx.send(());

    // Wait for thread to finish and ensure it took less than 100ms
    let res = handle.join();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(100),
        "Test took too long: {:?}",
        elapsed
    );
    assert!(
        res.is_ok(),
        "Builder thread panicked or did not exit cleanly"
    );
}
