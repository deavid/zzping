# zzping

*Ping your home network while you sleep.*

## Description

This is a collection of tools to monitor home network latency and packet loss using a distributed collector architecture with zero-trust security.

## Architecture

- **Database**: Central server that collects and stores ping data from collectors.
- **Collectors**: Distributed agents that perform ping measurements and send data to the database.
- **Security**: mTLS-based authentication with certificate rotation support.

## Quick Start

### Prerequisites

- Rust 1.70+
- OpenSSL for certificate generation

### Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/deavid/zzping.git
   cd zzping
   ```

2. Generate test certificates:
   ```bash
   ./generate_certs.sh
   ```

3. Build the binaries:
   ```bash
   cargo build --release
   ```

### Running

1. Start the database:
   ```bash
   ./target/release/zzping-database --config database.example.ron
   ```

2. Start a collector:
   ```bash
   ./target/release/zzping-collector --config collector.example.ron
   ```

## Testing

### Unit Tests
```bash
cargo test
```

### Integration Tests

**⚠️ IMPORTANT:** Integration tests are not currently functional as Cargo test targets.

Test files exist in `tests/` but are not registered in Cargo.toml:
- `tests/e2e_smoke.rs` - End-to-end smoke tests
- `tests/cert_rotation_test.rs` - Certificate rotation validation
- `tests/stability_test.rs` - Long-running stability tests

**Known Issues:**
- Running `cargo test --test e2e_smoke` fails with "no test target"
- TLS handshake errors: "UnsupportedCertVersion" - cert generation needs fixes

**Test Scripts (Unverified):**
```bash
# Short stability test (script runner)
./scripts/run_short_stability.sh

# Load test (requires working TLS certs)
./scripts/load_test.sh 10 60

# Chaos test (needs manual validation)
./scripts/chaos_test.sh 3 120
```

### Certificate Management

Generate certificates for multiple collectors:
```bash
./scripts/generate_multi_certs.sh 100
```

Test certificate rotation:
```bash
./scripts/generate_two_cas.sh
cargo test --test cert_rotation_test
```

## Configuration

See `database.example.ron` and `collector.example.ron` for configuration examples.

## Troubleshooting

### Common Issues

- **TLS handshake failures**: Ensure certificates are valid and CN/SAN matches the host.
- **Connection refused**: Check that the database is running and ports are open.
- **Certificate rotation**: Use dual CA support for zero-downtime rotation.

### Logs

Enable debug logging:
```bash
RUST_LOG=debug ./target/release/zzping-database --config config.ron
```

## Development

### Building
```bash
cargo build
cargo build --release
```

### Testing
```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

### Phase 6 MVP Status

**Completed:**
- ✅ Multi-CA certificate rotation support (database accepts multiple CA certs)
- ✅ Comprehensive documentation (README, TROUBLESHOOTING, RUNBOOK)
- ✅ Test infrastructure created (files, scripts, fixtures)
- ✅ All unit tests pass
- ✅ Clippy clean
- ✅ Release builds work

**In Progress / Needs Work:**
- ⚠️ Integration tests not registered in Cargo.toml
- ⚠️ TLS certificate generation has compatibility issues
- ⚠️ Load/chaos/stability tests exist but unverified
- ⚠️ No actual 24-hour run completed

**Note:** This is an MVP phase - focus is on foundation, not production perfection.

## Contributing

See `CONTRIBUTING.md` for development guidelines.

## License

Apache-2.0
