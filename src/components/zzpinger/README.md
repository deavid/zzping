# `zzpinger` Component

## Overview

The `zzpinger` component is a high-performance, concurrent ICMP ping engine for network monitoring. It is designed to ping multiple network targets simultaneously, each with its own configurable rate and timeout.

Its primary responsibilities are:
- Managing the lifecycle of ping operations for a dynamic list of targets.
- Enforcing per-target rate limits to avoid flooding the network.
- Detecting timeouts and tracking packet loss.
- Submitting structured `PingResult` data to a collector component (e.g., `zzmem-db`).
- Allowing for dynamic updates to the target list and operational state at runtime.

The component is built using the Actix actor framework, with a dedicated asynchronous task for each target to ensure non-blocking, independent operation.

## Privilege Requirements

To send ICMP packets, this component requires the `CAP_NET_RAW` capability. Without it, all ping attempts will fail.

You can grant this capability to the compiled binary using `setcap`:
```bash
sudo setcap cap_net_raw=+ep /path/to/your/binary
```
Alternatively, you can run the application as root, although this is not recommended for security reasons.

## Target Configuration

Targets are defined using the `TargetConfig` struct, which includes:
- `target`: The hostname or IP address to ping.
- `rate_ms`: The interval in milliseconds between pings to this specific target.
- `timeout_ms`: The duration in milliseconds to wait for a reply before considering the ping lost.

## Rate Limiting Behavior

Rate limiting is handled on a per-target basis. Each target has its own independent ping loop running in a dedicated task, which sleeps for the configured `rate_ms` between each ping attempt. This ensures that a slow or unresponsive target does not affect the monitoring of other targets.

## Integration with `zzmem-db`

`zzpinger` is designed to work with a collector component that accepts `StorePingResult` messages. The `PingerBuilder` allows you to configure the `Recipient` for this message, decoupling `zzpinger` from any specific collector implementation.

## Usage Example

Here is a basic example of how to create and run a pinger:

```rust
use zzpinger::builder::PingerBuilder;
use zzpinger::messages::TargetConfig;
use std::sync::Arc;
use tokio::time::sleep;
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Use the builder to configure the pinger
    let pinger_handle = PingerBuilder::new()
        .targets(vec![
            TargetConfig {
                target: "8.8.8.8".to_string(),
                rate_ms: 1000, // Ping every 1 second
                timeout_ms: 500, // 500ms timeout
            },
            TargetConfig {
                target: "1.1.1.1".to_string(),
                rate_ms: 2000, // Ping every 2 seconds
                timeout_ms: 1000,
            },
        ])
        .start()
        .expect("Failed to start pinger");

    println!("Pinger started. Monitoring targets...");
    sleep(Duration::from_secs(10)).await;

    // You can get health information at any time
    let health = pinger_handle.get_health().await.unwrap();
    println!("Current Pinger Health: {:?}", health);
}
```

This example uses the `MockBackend` by default, so it will not perform real network operations. To use the real ICMP backend, you would omit the `.backend()` call when using the `PingerBuilder`.