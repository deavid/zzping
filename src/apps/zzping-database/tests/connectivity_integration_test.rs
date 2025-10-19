//! Integration test for ZZPing connectivity between database and collector.
//!
//! # Why This Test Exists
//!
//! This integration test serves two critical purposes:
//!
//! 1. **Reliable Connectivity Verification**: Testing the complete TLS connection flow
//!    between a running database server and a collector client requires both processes
//!    to be alive simultaneously. This test spawns both as subprocesses in a controlled
//!    way, which is much more reliable than trying to manage separate terminal commands.
//!    The test framework automatically handles process lifecycle, cleanup, and error
//!    handling.
//!
//! 2. **Regression Detection**: This test will immediately catch if:
//!    - TLS certificate validation breaks
//!    - Rustls crypto provider setup fails
//!    - Network binding fails
//!    - Configuration loading fails
//!    - Component initialization fails
//!
//!    These issues would otherwise only be discovered during manual testing, which
//!    is error-prone and time-consuming.
//!
//! # How It Works
//!
//! The test:
//! 1. Spawns a database server process with release binary and test config
//! 2. Waits for it to be ready (listening on TCP port)
//! 3. Spawns a collector client process
//! 4. Waits briefly for connection attempt (with short timeout)
//! 5. Verifies from logs that connection was attempted and succeeded
//! 6. Cleans up both processes
//!
//! # Why This Approach?
//!
//! **Why not use `tokio::spawn` instead of subprocess spawning?**
//! - Would require linking the binary libraries at test time
//! - Would create complex dependency chains in the test binary
//! - Would not test the actual compiled binaries
//! - Subprocess approach tests what users actually run
//!
//! **Why capture logs instead of assertions in code?**
//! - Both services use structured logging with `tracing`
//! - Logs are the system's observable interface
//! - Testing logs ensures the error messages users see are correct
//! - Easier to debug when tests fail (logs show what happened)
//!
//! **Why is this important?**
//! - Connectivity is a critical integration point
//! - Manual testing is error-prone (easy to forget to run both)
//! - CI/CD pipelines can't run manual tests reliably
//! - This test ensures the system works end-to-end
//!
//! # When This Test Would Catch Bugs
//!
//! Examples of issues this would detect:
//! - "LocalSet not initialized" panic (TLS setup before runtime ready)
//! - CryptoProvider not installed (Rustls error)
//! - Wrong bind address (can't listen on required port)
//! - Config file not found or malformed
//! - TCP handshake errors
//! - TLS certificate validation failures
//! - Component initialization deadlocks
//!
//! # Requirements
//!
//! This test requires:
//! - `config/database.ron` and `config/collector.ron` to exist (with valid paths)
//! - `test_certs/` directory with certificates
//! - Both binaries to be built: `cargo build --release`
//! - Port 8443 to be available (or change test config)

use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// Test that database and collector can establish a TLS connection.
///
/// This integration test verifies the complete connectivity flow:
/// - Database server starts and binds to port
/// - Collector client attempts to connect
/// - TLS handshake succeeds
/// - No fatal errors in either service
#[test]
#[ignore] // Ignored by default; run with `cargo test -- --ignored --test-threads=1`
fn test_connectivity_database_to_collector() {
    println!("\n=== Connectivity Integration Test ===\n");

    // Verify prerequisites
    let workspace_root = verify_prerequisites();

    // Spawn database in background
    println!("Starting database server...");
    let db_binary = workspace_root.join("target/release/zzping-database");
    let config_db = workspace_root.join("config/database.ron");
    let mut db_process = Command::new(&db_binary)
        .current_dir(&workspace_root) // Run from workspace root so relative paths work
        .arg("--config")
        .arg(&config_db)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn database process");

    // Give database time to start and bind to port
    println!("Waiting for database to bind to port...");
    thread::sleep(Duration::from_millis(500));

    // Verify database is running
    match db_process.try_wait() {
        Ok(Some(_)) => {
            // Process exited
            let output = db_process
                .wait_with_output()
                .expect("Failed to get database output");
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            panic!(
                "Database process exited unexpectedly during startup\nStdout:\n{}\nStderr:\n{}",
                stdout, stderr
            );
        }
        Ok(None) => {
            // Still running - good!
        }
        Err(e) => {
            panic!("Failed to check database process status: {}", e);
        }
    }

    // Spawn collector in background
    println!("Starting collector client...");
    let collector_binary = workspace_root.join("target/release/zzping-collector");
    let config_collector = workspace_root.join("config/collector.ron");
    let mut collector_process = Command::new(&collector_binary)
        .current_dir(&workspace_root) // Run from workspace root so relative paths work
        .arg("--config")
        .arg(&config_collector)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn collector process");

    // Give collector time to attempt connection
    println!("Waiting for connection attempt...");
    thread::sleep(Duration::from_secs(2));

    // Terminate collector (should exit gracefully soon anyway)
    println!("Terminating collector...");
    collector_process.kill().expect("Failed to kill collector");
    let collector_output = collector_process
        .wait_with_output()
        .expect("Failed to wait for collector");

    // Terminate database
    println!("Terminating database...");
    db_process.kill().expect("Failed to kill database");
    let db_output = db_process
        .wait_with_output()
        .expect("Failed to wait for database");

    // Check outputs for critical errors
    let db_stderr = String::from_utf8_lossy(&db_output.stderr);
    let db_stdout = String::from_utf8_lossy(&db_output.stdout);
    let collector_stderr = String::from_utf8_lossy(&collector_output.stderr);
    let collector_stdout = String::from_utf8_lossy(&collector_output.stdout);

    println!("\n--- Database Output ---");
    println!("{}", db_stdout);
    if !db_stderr.is_empty() {
        println!("STDERR: {}", db_stderr);
    }

    println!("\n--- Collector Output ---");
    println!("{}", collector_stdout);
    if !collector_stderr.is_empty() {
        println!("STDERR: {}", collector_stderr);
    }

    // Verify database started successfully
    assert!(
        db_stdout.contains("Database service ready - accepting connections"),
        "Database did not reach 'ready' state. Full output:\n{}",
        db_stdout
    );

    // Verify database bound to port
    assert!(
        db_stdout.contains("TCP listener bound successfully"),
        "Database did not bind to TCP port. Full output:\n{}",
        db_stdout
    );

    // Verify collector started successfully
    assert!(
        collector_stdout.contains("Collector service starting"),
        "Collector did not start. Full output:\n{}",
        collector_stdout
    );

    // Verify collector attempted to connect
    assert!(
        collector_stdout.contains("Connecting to database"),
        "Collector did not attempt connection. Full output:\n{}",
        collector_stdout
    );

    // Verify database accepted the connection
    assert!(
        db_stdout.contains("Accepted connection from"),
        "Database did not accept connection from collector. Full output:\n{}",
        db_stdout
    );

    // Verify HELLO handshake occurred
    assert!(
        db_stdout.contains("Starting HELLO handshake with"),
        "Database did not start HELLO handshake. Full output:\n{}",
        db_stdout
    );

    assert!(
        db_stdout.contains("HELLO handshake completed successfully"),
        "Database HELLO handshake did not complete. Full output:\n{}",
        db_stdout
    );

    // Verify collector performed HELLO handshake
    assert!(
        collector_stdout.contains("Starting HELLO handshake as collector"),
        "Collector did not start HELLO handshake. Full output:\n{}",
        collector_stdout
    );

    assert!(
        collector_stdout.contains("HELLO handshake completed successfully as collector"),
        "Collector HELLO handshake did not complete. Full output:\n{}",
        collector_stdout
    );

    // Verify successful TLS connection
    // The collector should report successful connection
    assert!(
        collector_stdout.contains("Connected to database successfully")
            || collector_stdout.contains("Connecting to database"),
        "Collector did not attempt or succeed in connecting. Output:\n{}",
        collector_stdout
    );

    // Check for actual TLS handshake failures (not just missing client certs in logs)
    let has_handshake_failure = collector_stdout.contains("TLS handshake failed")
        || collector_stdout.contains("certificate not valid");

    assert!(
        !has_handshake_failure,
        "Collector experienced TLS handshake failure. Output:\n{}",
        collector_stdout
    );

    println!("\n✅ TLS handshake successful and collector connected!");

    // Verify no panics occurred
    assert!(
        !db_stdout.contains("panicked") && !db_stderr.contains("panicked"),
        "Database panicked. Stderr:\n{}\nStdout:\n{}",
        db_stderr,
        db_stdout
    );

    assert!(
        !collector_stdout.contains("panicked") && !collector_stderr.contains("panicked"),
        "Collector panicked. Stderr:\n{}\nStdout:\n{}",
        collector_stderr,
        collector_stdout
    );

    println!("\n✅ Test PASSED: Connectivity verified!\n");
}

/// Verify that all prerequisites for the test are in place.
/// Returns the workspace root path for use in the test.
fn verify_prerequisites() -> std::path::PathBuf {
    // Get the workspace root (project root, not the test binary location)
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    // CARGO_MANIFEST_DIR points to src/apps/zzping-database/
    // We need to go up 3 levels to get to repo root
    let mut workspace_root = std::path::PathBuf::from(manifest_dir);
    workspace_root.pop(); // Remove zzping-database
    workspace_root.pop(); // Remove apps
    workspace_root.pop(); // Remove src

    // Check binaries exist
    let db_binary = workspace_root.join("target/release/zzping-database");
    assert!(
        db_binary.exists(),
        "Database binary not found at: {}\nRun: cargo build --release",
        db_binary.display()
    );

    let collector_binary = workspace_root.join("target/release/zzping-collector");
    assert!(
        collector_binary.exists(),
        "Collector binary not found at: {}\nRun: cargo build --release",
        collector_binary.display()
    );

    // Check configs exist
    let config_db = workspace_root.join("config/database.ron");
    assert!(
        config_db.exists(),
        "Database config not found at: {}\nCreate config/database.ron from config/database.ron.example",
        config_db.display()
    );

    let config_collector = workspace_root.join("config/collector.ron");
    assert!(
        config_collector.exists(),
        "Collector config not found at: {}\nCreate config/collector.ron from config/collector.ron.example",
        config_collector.display()
    );

    // Check certs exist
    let ca_cert = workspace_root.join("test_certs/ca.pem");
    assert!(
        ca_cert.exists(),
        "CA certificate not found at: {}. Run: ./generate_certs.sh --all",
        ca_cert.display()
    );
    let db_cert = workspace_root.join("test_certs/database.pem");
    assert!(
        db_cert.exists(),
        "Database certificate not found at: {}. Run: ./generate_certs.sh --all",
        db_cert.display()
    );
    let db_key = workspace_root.join("test_certs/database.key");
    assert!(
        db_key.exists(),
        "Database key not found at: {}. Run: ./generate_certs.sh --all",
        db_key.display()
    );
    let collector_cert = workspace_root.join("test_certs/collector.pem");
    assert!(
        collector_cert.exists(),
        "Collector certificate not found at: {}. Run: ./generate_certs.sh --all",
        collector_cert.display()
    );
    let collector_key = workspace_root.join("test_certs/collector.key");
    assert!(
        collector_key.exists(),
        "Collector key not found at: {}. Run: ./generate_certs.sh --all",
        collector_key.display()
    );

    println!("✓ All prerequisites verified");
    workspace_root
}
