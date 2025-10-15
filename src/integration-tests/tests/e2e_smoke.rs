//! End-to-end smoke tests for collector + database integration.

use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tokio::process::Command as TokioCommand;
use tokio::time::sleep;

// Simple e2e: generate certs, start db + collector, and verify collector authentication
// by scanning service logs. The test is timeboxed to 60s using tokio::time::timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_e2e_single_collector_connects() {
    // Run the test body under a 60s timeout so it fails fast instead of hanging.
    let run = async {
        // 1) Ensure test certs exist
        // Pre-test cleanup: kill any leftover database/collector processes and wait a moment so ports are freed.
        let _ = Command::new("bash")
            .args([
                "-c",
                "pkill -f zzping-database || true; pkill -f zzping-collector || true; sleep 1",
            ])
            .status();

        assert!(
            Command::new("bash")
                .args(["-c", "../../scripts/generate_multi_certs.sh 1"]) // creates test_certs/ with ca.pem
                .status()
                .expect("Failed to run generate_multi_certs.sh")
                .success()
        );

        // 2) Avoid running `cargo build` here (cargo test already compiled needed artifacts).
        // Running an extra `cargo build` inside the test is expensive and can push the
        // wall-clock time over the target timeout; skip it to keep the test fast.

        // 3) Start database (workspace-aware), capturing output using tokio's async process
        // Compute absolute paths to built binaries from the workspace root so the test
        // finds them even when Cargo runs tests from a different current directory.
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
        // manifest_dir is .../zzping/src/integration-tests -> workspace root is two levels up
        let workspace_root = PathBuf::from(manifest_dir)
            .join("..")
            .join("..")
            .canonicalize()
            .expect("Failed to canonicalize workspace root");
        let db_bin = workspace_root
            .join("target")
            .join("debug")
            .join("zzping-database");
        let col_bin = workspace_root
            .join("target")
            .join("debug")
            .join("zzping-collector");

        // Redirect logs to files under workspace tmp so we can read them reliably
        let logs_dir = workspace_root.join("tmp");
        std::fs::create_dir_all(&logs_dir).expect("failed to create tmp logs dir");
        let db_log_path = logs_dir.join("e2e-db.log");
        let col_log_path = logs_dir.join("e2e-collector.log");

        let db_out_file =
            std::fs::File::create(db_log_path.clone()).expect("failed to create db log file");
        let db_err_file = std::fs::File::create(logs_dir.join("e2e-db-err.log"))
            .expect("failed to create db err file");

        let mut db = TokioCommand::new(db_bin)
            .args(["--config", "../../tests/fixtures/database-e2e.ron"])
            .stdout(std::process::Stdio::from(
                db_out_file.try_clone().expect("clone db out"),
            ))
            .stderr(std::process::Stdio::from(
                db_err_file.try_clone().expect("clone db err"),
            ))
            .spawn()
            .expect("Failed to start database");

        // 4) Wait for DB to be ready (poll DB log) before starting collector to avoid race/connection refused.
        // Wait up to 4s for DB to become ready (fast path should be a couple seconds)
        let db_ready_deadline = tokio::time::Instant::now() + Duration::from_secs(4);
        let mut db_ready = false;
        while tokio::time::Instant::now() < db_ready_deadline {
            if tokio::fs::read_to_string(&db_log_path)
                .await
                .map(|s| {
                    s.contains("Database service ready - accepting connections")
                        || s.contains("TCP listener bound successfully")
                })
                .unwrap_or(false)
            {
                db_ready = true;
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }

        if !db_ready {
            let mut combined = String::new();
            if let Ok(s) = tokio::fs::read_to_string(&db_log_path).await {
                combined.push_str("=== db stdout ===\n");
                combined.push_str(&s);
            }
            let db_err_path = logs_dir.join("e2e-db-err.log");
            if let Ok(s) = tokio::fs::read_to_string(&db_err_path).await {
                combined.push_str("\n=== db stderr ===\n");
                combined.push_str(&s);
            }
            panic!(
                "DB did not become ready before timeout. Logs:\n{}",
                combined
            );
        }

        // 5) Start collector (workspace-aware), capturing output
        let col_out_file = std::fs::File::create(col_log_path.clone())
            .expect("failed to create collector log file");
        let col_err_file = std::fs::File::create(logs_dir.join("e2e-collector-err.log"))
            .expect("failed to create collector err file");

        let mut collector = TokioCommand::new(col_bin)
            .args(["--config", "../../tests/fixtures/collector-01-e2e.ron"])
            .stdout(std::process::Stdio::from(
                col_out_file.try_clone().expect("clone col out"),
            ))
            .stderr(std::process::Stdio::from(
                col_err_file.try_clone().expect("clone col err"),
            ))
            .spawn()
            .expect("Failed to start collector");

        // Poll both stdout and stderr log files for evidence that the DB authenticated the collector
        let db_err_path = logs_dir.join("e2e-db-err.log");
        // Allow up to 12s for the collector to connect and be authenticated. Combined with
        // DB startup this should target ~16s end-to-end on normal machines and stay well
        // under the 60s test timeout.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
        let mut db_ok = false;
        while tokio::time::Instant::now() < deadline && !db_ok {
            let found_stdout = tokio::fs::read_to_string(&db_log_path)
                .await
                .map(|s| s.contains("Client") && s.contains("authenticated as role: Collector"))
                .unwrap_or(false);
            let found_stderr = tokio::fs::read_to_string(&db_err_path)
                .await
                .map(|s| s.contains("Client") && s.contains("authenticated as role: Collector"))
                .unwrap_or(false);

            if found_stdout || found_stderr {
                db_ok = true;
                break;
            }

            sleep(Duration::from_millis(100)).await;
        }

        if !db_ok {
            let mut combined = String::new();
            if let Ok(s) = tokio::fs::read_to_string(&db_log_path).await {
                combined.push_str("=== db stdout ===\n");
                combined.push_str(&s);
            }
            let db_err_path = logs_dir.join("e2e-db-err.log");
            if let Ok(s) = tokio::fs::read_to_string(&db_err_path).await {
                combined.push_str("\n=== db stderr ===\n");
                combined.push_str(&s);
            }
            panic!(
                "Did not observe DB authentication log within timeout. Logs:\n{}",
                combined
            );
        }

        // 5) At this point we've observed the DB accept connections and authenticate the collector
        // via its log output (mTLS + application-level auth). That is sufficient for the smoke test.
        // If needed, a programmatic RPC can be added later once the service advertises gRPC/HTTP2.

        // 6) Cleanup: try graceful termination and wait to avoid zombies
        // Try graceful kill and wait with a short timeout to avoid hanging test teardown.
        if let Err(e) = collector.kill().await {
            eprintln!("collector.kill() failed: {}", e);
        }
        match tokio::time::timeout(Duration::from_secs(3), collector.wait()).await {
            Ok(_) => {}
            Err(_) => {
                // timed out waiting for collector to exit; attempt to force kill and move on
                let _ = collector.kill().await;
            }
        }

        if let Err(e) = db.kill().await {
            eprintln!("db.kill() failed: {}", e);
        }
        match tokio::time::timeout(Duration::from_secs(3), db.wait()).await {
            Ok(_) => {}
            Err(_) => {
                let _ = db.kill().await;
            }
        }

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    };

    // Enforce a 60 second wall-time timeout for the whole test.
    match tokio::time::timeout(Duration::from_secs(60), run).await {
        Ok(Ok(())) => (),
        Ok(Err(e)) => panic!("e2e test failed: {}", e),
        Err(_) => panic!("e2e test timed out after 60s"),
    }
}
