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
   ./target/release/zzping-database --config src/apps/zzping-database/database.example.ron
   ```

2. Start a collector:
   ```bash
   ./target/release/zzping-collector --config src/apps/zzping-collector/collector.example.ron
   ```

## Development

The `zznet-builder` crate is the canonical way to build all applications in this workspace. It provides a framework that handles the entire application lifecycle, including configuration, logging, runtime, and graceful shutdown.

For instructions on how to create a new application, see the documentation in the builder crate itself:
- **[`src/net/zznet-builder/README.md`](./src/net/zznet-builder/README.md)**

### Building
```bash
cargo build
cargo build --release
```

### Testing
```bash
# Run all tests in the workspace
cargo nextest run --workspace

# Run all checks and lints
cargo clippy --workspace -- -D warnings
```

## Phase 6 MVP Status

**Completed:**
- ✅ Multi-CA certificate rotation support (database accepts multiple CA certs)
- ✅ Comprehensive documentation (README, TROUBLESHOOTING, RUNBOOK)
- ✅ Test infrastructure (unit tests in src/, select integration tests for complex E2E)
- ✅ All unit tests pass
- ✅ Clippy clean
- ✅ Release builds work
- ✅ `zznet-builder` integration (apps use the builder API instead of manual wiring)

**In Progress / Needs Work:**
- 🔄 zznet-room integration (apps should leverage Room abstraction)
- ⚠️ TLS certificate generation may have compatibility issues
- ⚠️ Load/chaos/stability tests exist but unverified
- ⚠️ No actual 24-hour run completed

**Note:** This is an MVP phase - focus is on foundation, not production perfection.

## Contributing

See `CONTRIBUTING.md` for development guidelines.

## License

Apache-2.0
