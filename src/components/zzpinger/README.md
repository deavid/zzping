# zzpinger Component

The `zzpinger` component manages ICMP ping operations for multiple network targets with configurable rates and timeouts. It submits ping results to `zzmem-db` for storage and analysis, enabling network monitoring and diagnostics.

## Purpose

This component provides the core ping engine for the zzping system. It handles:
- Concurrent ping operations for multiple targets
- Rate limiting to prevent network overload
- Timeout detection and loss tracking
- Result submission to the database for historical analysis

The design emphasizes testability and reliability, ensuring no real network operations occur during testing.

## Architecture

- **Actor-based**: Uses Actix actors for concurrent target management
- **Backend abstraction**: Supports pluggable ping backends (real ICMP or test mocks)
- **Rate limiting**: Per-target tokio tasks with configurable intervals
- **Result submission**: Sends `PingResult` messages to `zzmem-db` via Actix recipients

## Privilege Requirements

**Requires CAP_NET_RAW capability** for ICMP socket operations on Linux. Without this, real ping operations will fail.

To run with privileges:
```bash
# Set capability on binary
sudo setcap cap_net_raw=+ep target/debug/examples/basic_pinger

# Or run as root
sudo ./target/debug/examples/basic_pinger
```

## Usage

### Basic Setup

```rust
use zzpinger::{PingerBuilder, TargetConfig};

let targets = vec![
    TargetConfig {
        target: "8.8.8.8".to_string(),
        rate_ms: 1000,  // ping every second
        timeout_ms: 5000,
    }
];

let pinger = PingerBuilder::new()
    .targets(targets)
    .enabled(true)
    .start()
    .expect("failed to start pinger");
```

### With MemDB Integration

```rust
use zzmem_db::actor::MemDBActor;
use zzmem_db::permissions::MemDBPermission;

let memdb = MemDBActor::new(/* config */).start();

let pinger = PingerBuilder::new()
    .memdb_addr(memdb)
    .targets(targets)
    .start()
    .expect("failed to start pinger");
```

### Testing

For tests, inject a mock backend to avoid real ICMP:

```rust
use zzpinger::pinger::MockBackend;

let mock_backend = Arc::new(MockBackend::new(Some(1000))); // 1ms RTT

let pinger = PingerBuilder::new()
    .backend(mock_backend)
    .targets(targets)
    .start()
    .expect("failed to start pinger");
```

## Configuration

### TargetConfig

- `target`: IP address or hostname to ping
- `rate_ms`: Milliseconds between ping attempts (minimum 1)
- `timeout_ms`: Maximum wait time for ping response (minimum 1)

### Rate Limiting

Pings are sent at fixed intervals per target. The component uses tokio `sleep` to respect rates, ensuring predictable timing without drift.

## Integration with zzmem-db

Results are submitted as `StorePingResult` messages containing:
- `target`: The pinged address
- `timestamp_ms`: Unix timestamp in milliseconds
- `rtt_us`: Round-trip time in microseconds (None on timeout)
- `sequence`: Per-target sequence number

The component uses Actix `Recipient<StorePingResult>` for loose coupling, allowing tests to inject mock recipients.

## Error Handling

- Invalid targets (empty strings, zero rates/timeouts) are rejected at configuration time
- Ping failures (timeouts, network errors) result in `rtt_us = None`
- Submission failures to MemDB are logged but don't stop pinging
- Actor panics are avoided through proper error propagation

## Testing

The component is designed for comprehensive testing:
- All unit tests use `MockBackend` (no real network calls)
- Integration tests verify end-to-end message flow
- API tests exercise public interfaces
- Coverage targets >85% for reliability

Run tests:
```bash
cargo test -p zzpinger --lib
```

## Performance Considerations

- Each target spawns a dedicated tokio task
- Memory usage scales with number of targets
- CPU overhead is minimal (mostly async waits)
- Network I/O is bounded by configured rates

## Limitations

- ICMP requires elevated privileges on most systems
- IPv6 support depends on underlying ping library
- No built-in retry logic (single ping per interval)
- Sequence numbers reset on target reconfiguration</content>
<parameter name="filePath">/home/deavid/git/rust/zzping/src/components/zzpinger/README.md