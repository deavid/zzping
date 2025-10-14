//! Long-running stability tests
//!
//! Run with: cargo test --test stability_test -- --nocapture --ignored
//!
//! These tests are marked #[ignore] because they take hours to run.

use std::process::Command;
use std::time::{Duration, Instant};
use std::fs::File;
use std::io::{BufRead, BufReader};
use tokio::time::sleep;

const TEST_DURATION_SECS: u64 = if cfg!(debug_assertions) {
    60 // 1 minute for CI
} else {
    86400 // 24 hours for local
};

fn get_process_memory_kb(pid: u32) -> u64 {
    // Read /proc/<pid>/status and extract VmRSS (Resident Set Size) in kB.
    let status_path = format!("/proc/{}/status", pid);
    if let Ok(file) = File::open(status_path) {
        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            if line.starts_with("VmRSS:") {
                // Format: VmRSS:\t   123456 kB
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        return kb;
                    }
                }
            }
        }
    }
    0
}

#[tokio::test]
#[ignore]
async fn test_24h_stability_3_collectors() {
    println!("Starting 24-hour stability test...");
    println!("Duration: {} seconds", TEST_DURATION_SECS);

    // Build binaries
    assert!(Command::new("cargo")
        .args(&["build", "--release", "--bin", "zzping-database", "--bin", "zzping-collector"])
        .status()
        .unwrap()
        .success());

    // Start database
    let mut db = Command::new("./target/release/zzping-database")
        .arg("--config")
        .arg("tests/fixtures/database-stability.ron")
        .spawn()
        .expect("Failed to start database");

    sleep(Duration::from_secs(5)).await;

    // Start 3 collectors
    let mut collectors = vec![];
    for id in ["01", "02", "03"] {
        let collector = Command::new("./target/release/zzping-collector")
            .arg("--config")
            .arg(format!("tests/fixtures/collector-{}-stability.ron", id))
            .spawn()
            .expect(&format!("Failed to start collector {}", id));
        collectors.push(collector);
        sleep(Duration::from_secs(2)).await;
    }

    println!("All processes started. Running for {} seconds...", TEST_DURATION_SECS);

    let start = Instant::now();
    let check_interval = Duration::from_secs(60); // Check every minute

    let db_pid = db.id() as u32;
    let initial_db_mem = get_process_memory_kb(db_pid);

    while start.elapsed() < Duration::from_secs(TEST_DURATION_SECS) {
        sleep(check_interval).await;

        // Check database
        match db.try_wait() {
            Ok(Some(status)) => {
                panic!("Database exited unexpectedly with status: {}", status);
            }
            Ok(None) => {
                // Still running
                println!("Database still running after {:?}", start.elapsed());
            }
            Err(e) => {
                panic!("Error checking database status: {}", e);
            }
        }

        // Check memory growth
    let current_db_mem = get_process_memory_kb(db_pid);
        let memory_growth_kb = if current_db_mem > initial_db_mem { current_db_mem - initial_db_mem } else { 0 };
        println!("Database memory: {} KB (growth: {} KB)", current_db_mem, memory_growth_kb);

        // Fail if memory grows more than 500MB
        assert!(memory_growth_kb < 500 * 1024, "Memory leak detected: grew {} MB", memory_growth_kb / 1024 / 1024);

        for (i, collector) in collectors.iter_mut().enumerate() {
            match collector.try_wait() {
                Ok(Some(status)) => {
                    panic!("Collector {} exited unexpectedly with status: {}", i, status);
                }
                Ok(None) => {
                    // Still running
                }
                Err(e) => {
                    panic!("Error checking collector {} status: {}", i, e);
                }
            }
        }

        println!("All processes healthy. Elapsed: {:?} / {:?}", start.elapsed(), Duration::from_secs(TEST_DURATION_SECS));
    }

    println!("Stability test PASSED! All processes ran for full duration.");

    // Graceful shutdown
    for mut collector in collectors {
        collector.kill().ok();
    }
    db.kill().ok();
}
