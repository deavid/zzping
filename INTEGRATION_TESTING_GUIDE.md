# Integration Testing Guide for ZZPing

**Purpose:** Comprehensive guide for writing reliable integration tests
**Last Updated:** October 13, 2025
**Critical for:** Phase 4 (Collector), Phase 5 (Database), Phase 6 (Integration)

---

## Overview

Integration tests verify that **multiple components work together correctly**. They are:
- More realistic than unit tests (test actual interactions)
- More complex than unit tests (more setup required)
- More prone to flakiness (timing issues, race conditions)
- Essential for distributed systems like ZZPing

**This guide shows you how to write robust, non-flaky integration tests.**

---

## Table of Contents

1. [Fundamentals](#fundamentals)
2. [Test Structure Patterns](#test-structure-patterns)
3. [Mocking SessionManager](#mocking-sessionmanager)
4. [Time Mocking](#time-mocking)
5. [Async Test Patterns](#async-test-patterns)
6. [Avoiding Flaky Tests](#avoiding-flaky-tests)
7. [Common Integration Scenarios](#common-integration-scenarios)
8. [Debugging Failed Tests](#debugging-failed-tests)

---

## Fundamentals

### What is an Integration Test?

```
Unit Test:
  Tests ONE component in isolation
  Fast, deterministic, easy to debug

Integration Test:
  Tests MULTIPLE components together
  Slower, more complex, tests real interactions
```

### Example Hierarchy

```
Unit Test:
  CStateComponent::handle_stale_detection()
  → Tests just this one method

Integration Test:
  Collector → SessionManager → Database
  → Tests message flow through all components
  → Tests heartbeat system end-to-end
  → Tests stale detection across components
```

### File Organization

```
tests/
  unit/                    # Unit tests (test ONE thing)
    cstate_tests.rs
    pinger_tests.rs

  integration/             # Integration tests (test MULTIPLE things)
    collector_integration.rs    # Collector components together
    database_integration.rs     # Database components together
    e2e_test.rs                # Full collector + database
```

**Location Rule:**
- Unit tests: `#[cfg(test)] mod tests` in same file as code
- Integration tests: `tests/` directory (separate crate)

---

## Test Structure Patterns

### Pattern 1: Component Interaction Test

Tests multiple components within same application (collector or database).

```rust
#[tokio::test]
async fn test_pinger_cstate_interaction() {
    // 1. Setup: Create components
    let mock_session = Arc::new(MockSessionManager::new());
    let pinger = PingerComponent::builder()
        .with_session_manager(mock_session.clone())
        .build()
        .await
        .expect("Failed to build pinger");

    let cstate = CStateComponent::builder()
        .with_session_manager(mock_session.clone())
        .build()
        .await
        .expect("Failed to build cstate");

    // 2. Exercise: Trigger interaction
    pinger.send(SendHeartbeat { target: "db1".into() })
        .await
        .expect("Failed to send heartbeat");

    // 3. Verify: Check expected outcome
    tokio::time::sleep(Duration::from_millis(100)).await;

    let msg = mock_session.get_sent_message()
        .expect("No message sent");
    assert_eq!(msg.message_type, "Heartbeat");

    // 4. Cleanup: Actors stop automatically when dropped
}
```

**Key Points:**
- ✅ Use MockSessionManager, not real TLS connections
- ✅ Sleep briefly to let async operations complete
- ✅ Verify specific outcomes (message sent, state changed)
- ✅ Let actors clean up automatically (Drop impl)

### Pattern 2: Full End-to-End Test

Tests complete flow through multiple applications (collector + database).

```rust
#[tokio::test]
async fn test_collector_database_e2e() {
    // 1. Setup database
    let db_config = DatabaseConfig {
        bind_host: "127.0.0.1".into(),
        bind_port: 8444,  // Use non-standard port for tests
        tls: TlsConfig { /* test certs */ },
        // ...
    };
    let database = start_database(db_config).await.unwrap();

    // 2. Setup collector
    let collector_config = CollectorConfig {
        database_host: "127.0.0.1".into(),
        database_port: 8444,
        tls: TlsConfig { /* test certs */ },
        // ...
    };
    let collector = start_collector(collector_config).await.unwrap();

    // 3. Wait for connection
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 4. Trigger heartbeat
    collector.pinger.send(SendHeartbeat { target: "db1".into() })
        .await
        .unwrap();

    // 5. Verify database received it
    tokio::time::sleep(Duration::from_millis(500)).await;

    let received = database.get_received_messages();
    assert!(received.iter().any(|m| m.message_type == "Heartbeat"));

    // 6. Cleanup
    collector.stop().await;
    database.stop().await;
}
```

**Key Points:**
- ✅ Use test-specific ports (avoid conflicts)
- ✅ Wait for connection establishment
- ✅ Add delays for message propagation
- ✅ Explicitly stop servers (avoid port conflicts)

---

## Mocking SessionManager

SessionManager is the bridge to network layer. Mock it to avoid TLS complexity in tests.

### MockSessionManager Implementation

```rust
use std::sync::Arc;
use tokio::sync::Mutex;
use zznet_api::{SessionManager, Message, Result};

#[derive(Clone)]
pub struct MockSessionManager {
    sent_messages: Arc<Mutex<Vec<Message>>>,
    injected_messages: Arc<Mutex<Vec<Message>>>,
}

impl MockSessionManager {
    pub fn new() -> Self {
        Self {
            sent_messages: Arc::new(Mutex::new(Vec::new())),
            injected_messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Inject a message to be "received" from network
    pub async fn inject_message(&self, msg: Message) {
        self.injected_messages.lock().await.push(msg);
    }

    /// Get messages that were "sent" to network
    pub async fn get_sent_messages(&self) -> Vec<Message> {
        self.sent_messages.lock().await.clone()
    }

    /// Clear all recorded messages
    pub async fn clear(&self) {
        self.sent_messages.lock().await.clear();
        self.injected_messages.lock().await.clear();
    }
}

#[async_trait]
impl SessionManager for MockSessionManager {
    async fn send_message(&self, room: &str, msg: Message) -> Result<()> {
        // Record that message was sent
        self.sent_messages.lock().await.push(msg);
        Ok(())
    }

    async fn receive_message(&self) -> Result<Message> {
        // Return injected messages
        loop {
            let mut msgs = self.injected_messages.lock().await;
            if let Some(msg) = msgs.pop() {
                return Ok(msg);
            }
            drop(msgs);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn join_room(&self, room: &str) -> Result<()> {
        Ok(())  // Mock: always succeeds
    }
}
```

### Using MockSessionManager in Tests

```rust
#[tokio::test]
async fn test_component_sends_message() {
    let mock_session = Arc::new(MockSessionManager::new());

    let component = MyComponent::builder()
        .with_session_manager(mock_session.clone())
        .build()
        .await
        .unwrap();

    // Trigger some action
    component.send(DoSomething).await.unwrap();

    // Wait for async processing
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify message was sent
    let sent = mock_session.get_sent_messages().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].message_type, "ExpectedType");
}

#[tokio::test]
async fn test_component_receives_message() {
    let mock_session = Arc::new(MockSessionManager::new());

    let component = MyComponent::builder()
        .with_session_manager(mock_session.clone())
        .build()
        .await
        .unwrap();

    // Inject message to be "received"
    mock_session.inject_message(Message {
        message_type: "TestMessage".into(),
        payload: vec![],
    }).await;

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify component processed it
    let state = component.send(GetState).await.unwrap();
    assert_eq!(state.messages_received, 1);
}
```

---

## Time Mocking

**CRITICAL:** Use `tokio::time::pause()` to make time-dependent tests deterministic.

### Problem: Real-Time Tests Are Flaky

```rust
// ❌ FLAKY TEST
#[tokio::test]
async fn test_heartbeat_timeout() {
    let component = setup_component().await;

    // Wait 10 seconds for timeout
    tokio::time::sleep(Duration::from_secs(10)).await;

    // Problems:
    // 1. Test takes 10 seconds to run (slow!)
    // 2. Might timeout at 9.9s or 10.1s (flaky!)
    // 3. CI servers under load might timeout differently
}
```

### Solution: Mock Time

```rust
// ✅ FAST, DETERMINISTIC TEST
#[tokio::test]
async fn test_heartbeat_timeout() {
    // Pause time at test start
    tokio::time::pause();

    let component = setup_component().await;

    // Advance time instantly (no actual waiting!)
    tokio::time::advance(Duration::from_secs(10)).await;

    // Verify timeout occurred
    let state = component.send(GetState).await.unwrap();
    assert!(state.is_stale);

    // Test runs in milliseconds, not 10 seconds!
}
```

### Time Mocking Patterns

#### Pattern 1: Test Timeout Behavior

```rust
#[tokio::test]
async fn test_stale_detection() {
    tokio::time::pause();

    let cstate = CStateComponent::builder()
        .with_stale_timeout(Duration::from_secs(30))
        .build()
        .await
        .unwrap();

    // Record heartbeat at T=0
    cstate.send(RecordHeartbeat { peer: "peer1".into() })
        .await
        .unwrap();

    // Advance to T=20s (before timeout)
    tokio::time::advance(Duration::from_secs(20)).await;
    let state = cstate.send(CheckStale { peer: "peer1".into() })
        .await
        .unwrap();
    assert!(!state.is_stale, "Should not be stale at T=20s");

    // Advance to T=35s (after timeout)
    tokio::time::advance(Duration::from_secs(15)).await;
    let state = cstate.send(CheckStale { peer: "peer1".into() })
        .await
        .unwrap();
    assert!(state.is_stale, "Should be stale at T=35s");
}
```

#### Pattern 2: Test Periodic Tasks

```rust
#[tokio::test]
async fn test_periodic_heartbeat() {
    tokio::time::pause();

    let mock_session = Arc::new(MockSessionManager::new());
    let pinger = PingerComponent::builder()
        .with_heartbeat_interval(Duration::from_secs(5))
        .with_session_manager(mock_session.clone())
        .build()
        .await
        .unwrap();

    // Start periodic task
    pinger.send(StartHeartbeat).await.unwrap();

    // Advance time and check each interval
    for i in 1..=5 {
        tokio::time::advance(Duration::from_secs(5)).await;

        let sent = mock_session.get_sent_messages().await;
        assert_eq!(sent.len(), i, "Expected {} heartbeats at T={}s", i, i * 5);
    }
}
```

#### Pattern 3: Test Multiple Timers

```rust
#[tokio::test]
async fn test_multiple_timers() {
    tokio::time::pause();

    let component = setup_component().await;

    // Component has:
    // - Heartbeat every 5s
    // - Stale check every 10s
    // - Cleanup every 30s

    // T=5s: heartbeat fires
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(component.heartbeat_count().await, 1);

    // T=10s: heartbeat + stale check fire
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(component.heartbeat_count().await, 2);
    assert_eq!(component.stale_check_count().await, 1);

    // T=30s: all three fire
    tokio::time::advance(Duration::from_secs(20)).await;
    assert_eq!(component.heartbeat_count().await, 6);
    assert_eq!(component.stale_check_count().await, 3);
    assert_eq!(component.cleanup_count().await, 1);
}
```

---

## Async Test Patterns

### Pattern 1: Wait for Condition

```rust
use tokio::time::{timeout, Duration};

async fn wait_for_condition<F, Fut>(mut check: F, timeout_duration: Duration) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    timeout(timeout_duration, async {
        loop {
            if check().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await
    .map_err(|_| Error::Timeout)?;
    Ok(())
}

// Usage:
#[tokio::test]
async fn test_connection_established() {
    let collector = start_collector().await;

    // Wait up to 5s for connection
    wait_for_condition(
        || async { collector.is_connected().await },
        Duration::from_secs(5)
    ).await.expect("Connection not established");

    // Now test with established connection
    // ...
}
```

### Pattern 2: Parallel Test Execution

```rust
#[tokio::test]
async fn test_concurrent_operations() {
    let component = setup_component().await;

    // Spawn multiple operations concurrently
    let tasks: Vec<_> = (0..10).map(|i| {
        let comp = component.clone();
        tokio::spawn(async move {
            comp.send(DoWork { id: i }).await
        })
    }).collect();

    // Wait for all to complete
    let results = futures::future::join_all(tasks).await;

    // Verify all succeeded
    for (i, result) in results.into_iter().enumerate() {
        assert!(result.is_ok(), "Task {} failed: {:?}", i, result.err());
    }
}
```

### Pattern 3: Test Cleanup on Failure

```rust
#[tokio::test]
async fn test_with_cleanup() {
    struct TestGuard {
        database: Database,
        collector: Collector,
    }

    impl Drop for TestGuard {
        fn drop(&mut self) {
            // Cleanup happens even if test panics
            tokio::runtime::Handle::current().block_on(async {
                let _ = self.collector.stop().await;
                let _ = self.database.stop().await;
            });
        }
    }

    let guard = TestGuard {
        database: start_database().await.unwrap(),
        collector: start_collector().await.unwrap(),
    };

    // Test code here
    // Even if assertion fails, cleanup runs

    // Explicit cleanup if test passes
    drop(guard);
}
```

---

## Avoiding Flaky Tests

### Common Causes of Flakiness

1. **Race Conditions:** Test checks state before async operation completes
2. **Timing Assumptions:** Test assumes operation completes in X milliseconds
3. **Shared State:** Multiple tests modify same global state
4. **Port Conflicts:** Tests use same network port
5. **Ordering Dependencies:** Test relies on execution order

### Rule 1: Use tokio::time::pause() for Time

```rust
// ❌ FLAKY: Real sleep
#[tokio::test]
async fn test_timeout() {
    let component = setup().await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(component.is_timeout().await);
}

// ✅ DETERMINISTIC: Paused time
#[tokio::test]
async fn test_timeout() {
    tokio::time::pause();
    let component = setup().await;
    tokio::time::advance(Duration::from_secs(5)).await;
    assert!(component.is_timeout().await);
}
```

### Rule 2: Wait for Conditions, Don't Sleep Arbitrary Durations

```rust
// ❌ FLAKY: Arbitrary sleep
#[tokio::test]
async fn test_message_received() {
    component.send_message().await;
    tokio::time::sleep(Duration::from_millis(100)).await;  // Might not be enough!
    assert!(component.has_message().await);
}

// ✅ ROBUST: Wait for condition
#[tokio::test]
async fn test_message_received() {
    component.send_message().await;

    wait_for_condition(
        || async { component.has_message().await },
        Duration::from_secs(1)
    ).await.expect("Message not received");

    assert!(component.has_message().await);
}
```

### Rule 3: Use Unique Resources Per Test

```rust
// ❌ FLAKY: Same port for all tests
const TEST_PORT: u16 = 8080;

#[tokio::test]
async fn test_a() {
    let server = start_server("127.0.0.1", TEST_PORT).await;  // Might conflict!
    // ...
}

// ✅ ROBUST: Unique port per test
use std::sync::atomic::{AtomicU16, Ordering};
static TEST_PORT_COUNTER: AtomicU16 = AtomicU16::new(9000);

fn get_test_port() -> u16 {
    TEST_PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[tokio::test]
async fn test_a() {
    let port = get_test_port();
    let server = start_server("127.0.0.1", port).await;  // No conflicts!
    // ...
}
```

### Rule 4: Isolate Test State

```rust
// ❌ FLAKY: Shared global state
static GLOBAL_CONFIG: Mutex<Config> = /* ... */;

#[tokio::test]
async fn test_a() {
    GLOBAL_CONFIG.lock().unwrap().value = 10;
    // Another test might change this concurrently!
}

// ✅ ROBUST: Isolated state per test
#[tokio::test]
async fn test_a() {
    let config = Config { value: 10 };  // Local, not shared
    let component = setup_with_config(config).await;
    // ...
}
```

### Rule 5: Verify Test Actually Tests Something

```rust
// ❌ FALSE POSITIVE: Test always passes
#[tokio::test]
async fn test_stale_detection() {
    let component = setup().await;
    tokio::time::advance(Duration::from_secs(30)).await;
    // Forgot to actually check if stale!
    // Test passes but doesn't verify anything!
}

// ✅ ACTUALLY TESTS: Explicit assertion
#[tokio::test]
async fn test_stale_detection() {
    tokio::time::pause();
    let component = setup().await;
    tokio::time::advance(Duration::from_secs(30)).await;

    let state = component.send(CheckStale { peer: "peer1".into() })
        .await
        .unwrap();
    assert!(state.is_stale, "Expected peer to be stale after 30s");
}
```

---

## Common Integration Scenarios

### Scenario 1: Test Collector Components Together

```rust
#[tokio::test]
async fn test_collector_integration() {
    tokio::time::pause();
    let mock_session = Arc::new(MockSessionManager::new());

    // Create all collector components
    let intent_config = IntentConfigComponent::builder()
        .with_session_manager(mock_session.clone())
        .build()
        .await
        .unwrap();

    let pinger = PingerComponent::builder()
        .with_session_manager(mock_session.clone())
        .with_interval(Duration::from_secs(5))
        .build()
        .await
        .unwrap();

    let cstate = CStateComponent::builder()
        .with_session_manager(mock_session.clone())
        .with_stale_timeout(Duration::from_secs(30))
        .build()
        .await
        .unwrap();

    // Start heartbeat system
    pinger.send(StartHeartbeat).await.unwrap();

    // Advance time: heartbeat sent
    tokio::time::advance(Duration::from_secs(5)).await;

    // Verify pinger sent heartbeat
    let sent = mock_session.get_sent_messages().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].message_type, "Heartbeat");

    // Inject HeartbeatAck from database
    mock_session.inject_message(Message {
        message_type: "HeartbeatAck".into(),
        payload: vec![],
    }).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify cstate recorded it (not stale)
    let state = cstate.send(CheckStale { peer: "db1".into() })
        .await
        .unwrap();
    assert!(!state.is_stale);

    // Advance past timeout without new ack
    tokio::time::advance(Duration::from_secs(35)).await;

    // Verify now stale
    let state = cstate.send(CheckStale { peer: "db1".into() })
        .await
        .unwrap();
    assert!(state.is_stale);
}
```

### Scenario 2: Test Database Components Together

```rust
#[tokio::test]
async fn test_database_integration() {
    tokio::time::pause();
    let mock_session = Arc::new(MockSessionManager::new());

    // Create database components
    let intent_config = IntentConfigComponent::builder()
        .with_role(ComponentRole::Database)
        .with_session_manager(mock_session.clone())
        .build()
        .await
        .unwrap();

    let mem_db = MemDBComponent::builder()
        .with_session_manager(mock_session.clone())
        .build()
        .await
        .unwrap();

    // Inject heartbeat from collector
    mock_session.inject_message(Message {
        message_type: "Heartbeat".into(),
        sender: "collector1".into(),
        payload: vec![],
    }).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify database sent ack
    let sent = mock_session.get_sent_messages().await;
    assert!(sent.iter().any(|m| m.message_type == "HeartbeatAck"));

    // Inject data from collector
    mock_session.inject_message(Message {
        message_type: "SubmitData".into(),
        sender: "collector1".into(),
        payload: b"test data".to_vec(),
    }).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify data stored
    let data = mem_db.send(GetData { key: "collector1".into() })
        .await
        .unwrap();
    assert_eq!(data, b"test data");
}
```

### Scenario 3: Test Collector + Database End-to-End

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_e2e_heartbeat_flow() {
    // Use real TLS but test certificates
    let test_port = get_test_port();

    // Start database
    let db_config = DatabaseConfig {
        bind_host: "127.0.0.1".into(),
        bind_port: test_port,
        tls: test_tls_config_server(),
        // ...
    };
    let database = start_database(db_config).await.unwrap();

    // Wait for database to listen
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Start collector
    let collector_config = CollectorConfig {
        database_host: "127.0.0.1".into(),
        database_port: test_port,
        tls: test_tls_config_client(),
        heartbeat_interval: Duration::from_secs(1),
        // ...
    };
    let collector = start_collector(collector_config).await.unwrap();

    // Wait for connection
    wait_for_condition(
        || async { collector.is_connected().await },
        Duration::from_secs(5)
    ).await.expect("Collector did not connect");

    // Wait for heartbeat exchange
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify heartbeat sent and ack received
    let collector_state = collector.get_state().await;
    assert!(collector_state.heartbeats_sent > 0);
    assert!(collector_state.heartbeats_acked > 0);

    let database_state = database.get_state().await;
    assert!(database_state.heartbeats_received > 0);
    assert!(database_state.acks_sent > 0);

    // Cleanup
    collector.stop().await.unwrap();
    database.stop().await.unwrap();
}
```

---

## Debugging Failed Tests

### Step 1: Enable Detailed Logging

```bash
# Run test with full logs
RUST_LOG=debug cargo test test_name -- --nocapture

# Or trace level for even more detail
RUST_LOG=trace cargo test test_name -- --nocapture
```

### Step 2: Run Test in Isolation

```bash
# Run only this test
cargo test test_name -- --exact

# Run with single thread (avoid concurrency issues)
cargo test test_name -- --test-threads=1
```

### Step 3: Add Debug Prints

```rust
#[tokio::test]
async fn test_something() {
    eprintln!("=== Starting test ===");

    let component = setup().await;
    eprintln!("Component created");

    component.send(DoWork).await.unwrap();
    eprintln!("Work sent");

    tokio::time::sleep(Duration::from_millis(100)).await;
    eprintln!("Sleep complete");

    let state = component.send(GetState).await.unwrap();
    eprintln!("State: {:?}", state);

    assert!(state.is_complete);
}
```

### Step 4: Check for Panics in Background Tasks

```rust
// Wrap tokio::spawn to catch panics
let handle = tokio::spawn(async move {
    component.do_work().await
});

match handle.await {
    Ok(result) => println!("Task succeeded: {:?}", result),
    Err(e) => {
        if e.is_panic() {
            eprintln!("Task panicked: {:?}", e);
        } else {
            eprintln!("Task cancelled: {:?}", e);
        }
    }
}
```

### Step 5: Use Test-Specific Timeouts

```rust
use tokio::time::timeout;

#[tokio::test]
async fn test_with_timeout() {
    let result = timeout(Duration::from_secs(10), async {
        // Test code here
        run_test().await
    }).await;

    match result {
        Ok(Ok(())) => println!("Test passed"),
        Ok(Err(e)) => panic!("Test failed: {:?}", e),
        Err(_) => panic!("Test timed out after 10s"),
    }
}
```

---

## Quick Reference: Integration Test Checklist

When writing an integration test, verify:

### [ ] Test Setup
- [ ] Use `tokio::time::pause()` if testing timeouts
- [ ] Use unique resources (ports, files) to avoid conflicts
- [ ] Use MockSessionManager for unit integration tests
- [ ] Create components in correct order (dependencies first)

### [ ] Test Execution
- [ ] Wait for conditions, don't sleep arbitrary durations
- [ ] Add small delays after async operations (`sleep(Duration::from_millis(50))`)
- [ ] Verify each step explicitly (don't assume)

### [ ] Test Assertions
- [ ] Check specific values, not just "something happened"
- [ ] Verify both positive and negative cases
- [ ] Test failure paths, not just happy path

### [ ] Test Cleanup
- [ ] Stop servers/components explicitly
- [ ] Clear mock state between test sections
- [ ] Use Drop or defer for cleanup on panic

### [ ] Test Quality
- [ ] Run test 10+ times to verify not flaky
- [ ] Run with `--test-threads=1` to verify no race conditions
- [ ] Enable logging to verify behavior
- [ ] Test should complete in < 1 second (with time mocking)

---

## Summary: Keys to Reliable Integration Tests

1. **Use tokio::time::pause()** for all time-dependent tests
2. **Use MockSessionManager** to avoid TLS complexity
3. **Wait for conditions** with timeouts, don't sleep fixed durations
4. **Use unique resources** (ports, files) per test
5. **Verify explicitly** - check exact values, not just "something happened"
6. **Test in isolation** - run individually to verify not flaky
7. **Add logging** - use eprintln! or RUST_LOG to debug failures
8. **Clean up properly** - stop servers, clear state

**Remember:** Integration tests are harder to write but essential for distributed systems. Invest time to make them robust and deterministic!
