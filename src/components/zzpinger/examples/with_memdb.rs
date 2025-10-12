//! Example showing pinger integration with a recipient.
//!
//! Demonstrates how ping results are sent via Recipient<StorePingResult>.
//! Shows the integration pattern for collecting ping data.

use actix::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use zzmem_db::messages::StorePingResult;
use zzpinger::builder::PingerBuilder;
use zzpinger::messages::TargetConfig;
use zzpinger::pinger::MockBackend;

/// Simple mock collector that receives ping results
#[derive(Clone)]
struct MockCollector {
    result_count: Arc<std::sync::Mutex<usize>>,
}

impl MockCollector {
    fn new() -> Self {
        Self {
            result_count: Arc::new(std::sync::Mutex::new(0)),
        }
    }

    fn get_count(&self) -> usize {
        *self.result_count.lock().unwrap()
    }
}

impl Actor for MockCollector {
    type Context = Context<Self>;
}

impl Handler<StorePingResult> for MockCollector {
    type Result = Result<(), zzmem_db::messages::MemDBError>;

    fn handle(&mut self, _msg: StorePingResult, _ctx: &mut Self::Context) -> Self::Result {
        let mut count = self.result_count.lock().unwrap();
        *count += 1;
        println!("Received ping result #{}", *count);
        Ok(())
    }
}

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting pinger with recipient example...");

    // Create a mock collector to receive ping results
    let collector_instance = MockCollector::new();
    let collector_addr = collector_instance.clone().start();

    // Create pinger with mock backend and recipient
    let pinger = PingerBuilder::new()
        .backend(Arc::new(MockBackend::new(Some(10000))))
        .targets(vec![
            TargetConfig {
                target: "example.com".to_string(),
                rate_ms: 1000,
                timeout_ms: 5000,
            },
            TargetConfig {
                target: "test.com".to_string(),
                rate_ms: 2000,
                timeout_ms: 5000,
            },
        ])
        .memdb_recipient(collector_addr.recipient())
        .start()?;

    // Let it collect some data
    sleep(Duration::from_secs(7)).await;

    // Check how many results were received
    let count = collector_instance.get_count();
    println!("Collected {} ping results", count);

    // Check pinger health
    let health = pinger.get_health().await?;
    println!("Pinger health: {:?}", health);

    println!("Recipient integration example completed!");
    Ok(())
}
