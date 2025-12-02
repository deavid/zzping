//! Full choreography test (Acts I: Obedience, II: Disaster, III: Recovery)
use anyhow::Result;
use std::net::IpAddr;

use tracing::info;

use zzping_integration_test::harness::SystemHarness;

#[actix_rt::test]
async fn choreography_full() -> Result<()> {
    // Freeze Tokio time for deterministic advances
    tokio::time::pause();

    // Enable TRACE during debug runs so test-only trace instrumentation appears.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    info!("Starting choreography test (full)");

    // Create the harness (actors will use Arbiter::current())
    let mut harness = SystemHarness::new(Default::default())
        .await
        .expect("harness start");

    // Act I - Obedience: configure pinger and verify pings flow into collector MemDB
    let target1: IpAddr = "192.168.2.1".parse().unwrap();
    let target2: IpAddr = "192.168.2.2".parse().unwrap();

    harness.configure_intent(vec![target1, target2], 10).await;
    harness.enable_pinger(true).await;

    // Advance 2 virtual seconds (in 10ms steps) to allow ~10 pings per target
    for _ in 0..200 {
        tokio::time::advance(std::time::Duration::from_millis(10)).await;
    }

    // Wait until collector reports at least 10 total results
    // (we'll assert on DB flush later)
    harness.wait_for_pings(10).await.expect("expected pings");

    info!("Act I complete: pings observed");

    // Act II - Disaster: sever connection and ensure collector buffers results
    harness.sever_connection();

    // Advance time so more pings are produced and buffering occurs
    for _ in 0..400 {
        tokio::time::advance(std::time::Duration::from_millis(10)).await;
    }

    let coll_health = harness.collector_health().await.unwrap();
    assert!(
        coll_health.buffer_size > 0,
        "collector should have buffered results when disconnected"
    );

    info!(
        "Act II complete: buffered {} results",
        coll_health.buffer_size
    );

    // Act III - Recovery: restore connection and verify buffered results flushed to DB
    harness.restore_connection().await;

    // Advance to allow flush processing
    for _ in 0..400 {
        tokio::time::advance(std::time::Duration::from_millis(10)).await;
    }

    // The database should have received at least some results
    let db_health = harness.database_health().await.unwrap();
    // We expect significantly more than just the initial pings.
    // Act I produced ~20-40. Act II buffered ~80. Act III produced ~80.
    // So we should see > 100 results easily if recovery works.
    assert!(
        db_health.total_results > 90,
        "database should have received flushed results (got {})",
        db_health.total_results
    );

    info!(
        "Act III complete: DB total_results={}",
        db_health.total_results
    );

    // FIXME: This test relies on the hardcoded 10-second flush interval in MemDBActor.
    // If that changes, this test will need to be updated to advance time accordingly.
    tokio::time::advance(std::time::Duration::from_secs(11)).await;

    let stored_blobs = harness.get_stored_blobs().await?;
    assert!(
        !stored_blobs.is_empty(),
        "storage actor should have persisted blobs"
    );

    info!("Epilogue complete: data persisted to storage");

    Ok(())
}
