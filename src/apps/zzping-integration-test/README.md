# zzping-integration-test

The Hermetic Integration Test Suite for ZZPing v0.3.

## Overview

This crate provides a deterministic, in-process simulation of the entire ZZPing distributed system. It runs the real
production Actors (Collector, Database, Router) connected via in-memory mock transports.

**Key Features:**

- **Hermetic:** No network ports opened, no disk I/O (mocks), no root privileges needed.
- **Deterministic:** Uses `tokio::time::pause()` to control time. Tests pass instantly regardless of "sleeps" in the
  code.
- **Full Choreography:** Validates end-to-end scenarios (Config -> Pinger -> Network -> DB).

## The "Rules of the Game"

To maintain stability and speed, all tests in this crate must adhere to strict rules:

1. **Zero Real Sleeps:** Never use `std::thread::sleep`. Use `tokio::time::advance()` to skip time.
2. **Single Threaded:** The Harness runs all actors on `Arbiter::current()` inside a single `LocalSet`. This ensures the
   frozen clock is shared globally.
3. **No Network I/O:** Always use `SystemHarness` and `MockClient`/`MockServer`. Do not spawn `TcpTransport`.

## Architecture

The `SystemHarness` orchestrates the universe:

```rust
// The Harness wires up:
// 1. Database Actors (Router, MemDB, Intent, CState)
// 2. Collector Actors (Router, MemDB, Intent, Pinger)
// 3. Mock Transport (Memory Channels with KillSwitch)

let mut harness = SystemHarness::new().await?;
```

The Harness injects a `TokioAlignedClock` into the Pinger so that `SystemTime` calculations respect
`tokio::time::advance`.

## Writing a New Scenario

Create a new test file in `tests/` (e.g., `tests/my_scenario.rs`):

```rust
use zzping_integration_test::harness::SystemHarness;
use std::time::Duration;

#[actix_rt::test]
async fn test_my_scenario() {
    // 1. Freeze Time
    tokio::time::pause();

    // 2. Boot System
    let mut harness = SystemHarness::new().await.unwrap();

    // 3. Setup Intent
    harness.configure_intent(vec!["8.8.8.8".parse().unwrap()], 10).await;
    harness.enable_pinger(true).await;

    // 4. Advance Time (Simulate 5 seconds of operation)
    tokio::time::advance(Duration::from_secs(5)).await;

    // 5. Verify Results
    let db_health = harness.database_health().await.unwrap();
    assert!(db_health.total_results > 0);
}
```

## Advanced Controls

- **`harness.sever_connection()`**: Simulates a network cable cut. The transport watcher completes, triggering
  `maintain_connection` backoff logic.
- **`harness.restore_connection()`**: Creates a new mock transport pair and feeds it to the client/server queues,
  simulating connectivity restoration.
- **`harness.wait_for_pings(n)`**: Polls the Collector MemDB until `n` results are buffered/sent. (Uses 1ms polling loop
  compatible with virtual time).
