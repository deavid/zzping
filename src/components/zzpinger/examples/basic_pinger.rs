//! Basic pinger example demonstrating core functionality.
//!
//! Shows how to create a pinger, configure targets, and monitor health.
//! Uses MockBackend for deterministic behavior in examples.

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use zzpinger::builder::PingerBuilder;
use zzpinger::messages::TargetConfig;
use zzpinger::pinger::MockBackend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting basic pinger example...");

    // Create pinger with mock backend (no real network calls)
    let pinger = PingerBuilder::new()
        .backend(Arc::new(MockBackend::new(Some(10000)))) // 10ms RTT
        .targets(vec![
            TargetConfig {
                target: "example.com".to_string(),
                rate_ms: 1000,
                timeout_ms: 5000,
            },
            TargetConfig {
                target: "google.com".to_string(),
                rate_ms: 2000,
                timeout_ms: 5000,
            },
        ])
        .start()?;

    // Let it run for a bit
    sleep(Duration::from_secs(5)).await;

    // Check health
    let health = pinger.get_health().await?;
    println!("Health: {:?}", health);

    // Update targets dynamically
    pinger
        .update_targets(vec![TargetConfig {
            target: "github.com".to_string(),
            rate_ms: 1000,
            timeout_ms: 5000,
        }])
        .await?;

    // Run a bit more
    sleep(Duration::from_secs(3)).await;

    let final_health = pinger.get_health().await?;
    println!("Final health: {:?}", final_health);

    println!("Example completed successfully!");
    Ok(())
}
