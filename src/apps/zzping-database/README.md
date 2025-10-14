# ZZPing Database Server

Network monitoring database server that accepts mTLS connections from collectors,
stores ping data, and distributes configuration updates.

## Features

- **mTLS Server:** Accepts secure connections from authenticated collectors
- **Multi-Collector:** Handles multiple simultaneous collector connections
- **Component Integration:** Routes messages to IntentConfig, MemDB, and CState components
- **Graceful Shutdown:** Handles SIGTERM/SIGINT signals cleanly

## Configuration

Copy `database.example.ron` to `database.ron` and customize:

```ron
DatabaseConfig(
    bind_host: "0.0.0.0",
    bind_port: 8443,
    tls: TlsConfig(
        ca_cert_path: "test_certs/ca.pem",
        server_cert_path: "test_certs/database.pem",
        server_key_path: "test_certs/database.key",
    ),
    components: ComponentConfig(
        stale_timeout_secs: 30,
        max_collectors: 100,
    ),
)
```

## Running

```bash
# Build
cargo build --bin zzping-database

# Run with default config
./target/debug/zzping-database

# Run with custom config
./target/debug/zzping-database --config /path/to/database.ron

# Enable debug logging
RUST_LOG=debug ./target/debug/zzping-database

# Enable trace logging
./target/debug/zzping-database --trace
```

## Testing

```bash
# Run all tests
cargo test -p zzping-database

# Run specific test suite
cargo test -p zzping-database --test config_tests
cargo test -p zzping-database --test service_tests
```

## Architecture

- **main.rs:** Entry point with LocalSet for Actix runtime
- **config.rs:** Configuration structures and validation
- **service.rs:** Core service with TCP listener and connection handling
- **cli.rs:** Command-line argument parsing
- **error.rs:** Error types

## TLS Certificates

The database requires:
- **CA certificate:** For verifying collector client certificates
- **Server certificate:** Database's own identity
- **Server private key:** For TLS encryption

See `test_certs/` for test certificates (DO NOT USE IN PRODUCTION).

## Troubleshooting

**Connection refused:**
- Check bind_host/bind_port in config
- Verify port is not already in use: `netstat -ln | grep 8443`

**TLS handshake failed:**
- Verify certificates exist and are readable
- Check certificate validity: `openssl x509 -in database.pem -text -noout`
- Ensure collector certificate is signed by same CA

**Component failures:**
- Check component logs for errors
- Verify all Phase 1-3 components are built: `cargo build`
