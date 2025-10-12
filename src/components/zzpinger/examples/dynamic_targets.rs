//! Example demonstrating dynamic target management.
//!
//! Shows how to start with no targets and add them dynamically at runtime.
//! Demonstrates runtime configuration changes without restarting the pinger.

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use zzpinger::builder::PingerBuilder;
use zzpinger::messages::TargetConfig;
use zzpinger::pinger::MockBackend;

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting dynamic targets example...");

    // Create pinger with no initial targets
    let pinger = PingerBuilder::new()
        .backend(Arc::new(MockBackend::new(Some(5000)))) // 5ms RTT
        .start()?;

    // Start with no targets - pinger is idle
    sleep(Duration::from_secs(2)).await;

    let health = pinger.get_health().await?;
    println!("Initial health (no targets): {health:?}");

    // Add first target
    pinger
        .update_targets(vec![TargetConfig {
            target: "api.example.com".to_string(),
            rate_ms: 1000,
            timeout_ms: 5000,
        }])
        .await?;
    println!("Added first target");

    // Let it ping for a bit
    sleep(Duration::from_secs(4)).await;

    let health = pinger.get_health().await?;
    println!("Health after adding target: {health:?}");

    // Add more targets dynamically
    pinger
        .update_targets(vec![
            TargetConfig {
                target: "api.example.com".to_string(),
                rate_ms: 1000,
                timeout_ms: 5000,
            },
            TargetConfig {
                target: "db.example.com".to_string(),
                rate_ms: 2000,
                timeout_ms: 5000,
            },
            TargetConfig {
                target: "cache.example.com".to_string(),
                rate_ms: 500,
                timeout_ms: 3000,
            },
        ])
        .await?;
    println!("Updated to multiple targets");

    // Let it run with multiple targets
    sleep(Duration::from_secs(6)).await;

    let health = pinger.get_health().await?;
    println!("Final health with multiple targets: {health:?}");

    // Disable pinging temporarily
    pinger.set_enabled(false).await?;
    println!("Disabled pinging");

    sleep(Duration::from_secs(2)).await;

    let health = pinger.get_health().await?;
    println!("Health while disabled: {health:?}");

    // Re-enable pinging
    pinger.set_enabled(true).await?;
    println!("Re-enabled pinging");

    sleep(Duration::from_secs(3)).await;

    let final_health = pinger.get_health().await?;
    println!("Final health: {final_health:?}");

    println!("Dynamic targets example completed!");
    Ok(())
}
