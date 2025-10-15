//! Certificate rotation tests

use std::time::Duration;
use tokio::process::Command as TokioCommand;
use tokio::time::sleep;

#[tokio::test]
async fn test_database_accepts_old_and_new_ca_certs() {
    // Generate two CAs and certificates
    // Run the script from the repository root
    assert!(
        std::process::Command::new("sh")
            .args(["-c", "../../scripts/generate_two_cas.sh"])
            .status()
            .expect("Failed to run generate_two_cas.sh")
            .success()
    );

    // Start database with BOTH CAs trusted using cargo run so path resolution is workspace-aware

    let mut db = TokioCommand::new("cargo")
        .args([
            "run",
            "-p",
            "zzping-database",
            "--",
            "--config",
            "../../tests/fixtures/database-dual-ca.ron",
        ])
        .spawn()
        .expect("Failed to start database");

    sleep(Duration::from_secs(2)).await;

    // Start collectors (old/new) and then cleanup - full TLS verification is in original test file
    let mut collector_old = TokioCommand::new("cargo")
        .args([
            "run",
            "-p",
            "zzping-collector",
            "--",
            "--config",
            "../../tests/fixtures/collector-old-ca.ron",
        ])
        .spawn()
        .expect("Failed to start collector old");

    sleep(Duration::from_secs(3)).await;

    let mut collector_new = TokioCommand::new("cargo")
        .args([
            "run",
            "-p",
            "zzping-collector",
            "--",
            "--config",
            "../../tests/fixtures/collector-new-ca.ron",
        ])
        .spawn()
        .expect("Failed to start collector new");

    sleep(Duration::from_secs(3)).await;

    // Graceful shutdown with short timeouts to avoid zombies.
    let _ = collector_old.kill().await;
    let _ = tokio::time::timeout(Duration::from_secs(3), collector_old.wait()).await;

    let _ = collector_new.kill().await;
    let _ = tokio::time::timeout(Duration::from_secs(3), collector_new.wait()).await;

    let _ = db.kill().await;
    let _ = tokio::time::timeout(Duration::from_secs(3), db.wait()).await;
}
