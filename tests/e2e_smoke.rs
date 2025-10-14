//! End-to-end smoke tests for collector + database integration.
//!
//! This test suite starts real collector and database processes
//! and verifies they can communicate over mTLS.

use std::process::{Command, Child};
use std::time::Duration;
use tokio::time::sleep;

/// Helper to start database process
fn start_database() -> std::io::Result<Child> {
    Command::new("./target/debug/zzping-database")
        .arg("--config")
        .arg("tests/fixtures/database-e2e.ron")
        .spawn()
}

/// Helper to start collector process
fn start_collector(id: &str) -> std::io::Result<Child> {
    Command::new("./target/debug/zzping-collector")
        .arg("--config")
        .arg(format!("tests/fixtures/collector-{}-e2e.ron", id))
        .spawn()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_e2e_single_collector_connects() {
    // Build binaries first
    assert!(Command::new("cargo")
        .args(&["build", "--bin", "zzping-database", "--bin", "zzping-collector"])
        .status()
        .unwrap()
        .success());

    // Start database
    let mut db = start_database().expect("Failed to start database");

    // Wait for database to be ready
    sleep(Duration::from_secs(2)).await;

    // Start collector
    let mut collector = start_collector("01").expect("Failed to start collector");

    // Wait for connection
    sleep(Duration::from_secs(5)).await;

    // TODO: Verify connection established (check logs or metrics)

    // Cleanup
    collector.kill().ok();
    db.kill().ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_e2e_multiple_collectors_connect() {
    // Build binaries
    assert!(Command::new("cargo")
        .args(&["build", "--bin", "zzping-database", "--bin", "zzping-collector"])
        .status()
        .unwrap()
        .success());

    // Start database
    let mut db = start_database().expect("Failed to start database");
    sleep(Duration::from_secs(2)).await;

    // Start 3 collectors
    let mut collectors = vec![];
    for id in ["01", "02", "03"] {
        let collector = start_collector(id).expect(&format!("Failed to start collector {}", id));
        collectors.push(collector);
        sleep(Duration::from_millis(500)).await;
    }

    // Wait for all connections
    sleep(Duration::from_secs(10)).await;

    // TODO: Verify all collectors connected

    // Cleanup
    for mut collector in collectors {
        collector.kill().ok();
    }
    db.kill().ok();
}
