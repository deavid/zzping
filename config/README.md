# Configuration Files

This directory contains configuration files for ZZPing services.

## Files

- `database.ron` - Database server configuration (template)
- `database.ron.example` - Reference example
- `collector.ron` - Collector client configuration (template)
- `collector.ron.example` - Reference example

## Setup Instructions

### 1. Generate Certificates

First, generate TLS certificates:

```bash
./generate_certs.sh --all
```

This will create certificates in `test_certs/` with proper SAN values.

### 2. Configure Database

Copy and customize the database config:

```bash
cp config/database.ron.example config/database.ron
# Edit config/database.ron if needed
```

### 3. Configure Collector

Copy and customize the collector config:

```bash
cp config/collector.ron.example config/collector.ron
# Edit config/collector.ron if needed
```

### 4. Run Services

**Terminal 1 - Start Database:**
```bash
./target/release/zzping-database --config config/database.ron
```

**Terminal 2 - Start Collector:**
```bash
./target/release/zzping-collector --config config/collector.ron
```

## Configuration Options

### Database Config

```ron
DatabaseConfig(
    bind_host: "0.0.0.0",              // Address to listen on
    bind_port: 8443,                   // Port to listen on
    tls: TlsConfig(
        ca_cert_paths: [...],          // Path(s) to CA certificates
        server_cert_path: "...",       // Path to server certificate
        server_key_path: "...",        // Path to server private key
    ),
    components: ComponentConfig(
        stale_timeout_secs: 30,        // Collector stale timeout
        max_collectors: 100,           // Maximum concurrent collectors
    ),
)
```

### Collector Config

```ron
CollectorConfig(
    collector_id: "collector-01",      // Unique collector identifier
    database_host: "127.0.0.1",        // Database hostname/IP
    database_port: 8443,               // Database port
    tls: TlsConfig(
        ca_cert_path: "...",           // Path to CA certificate
        client_cert_path: "...",       // Path to client certificate
        client_key_path: "...",        // Path to client private key
    ),
    components: ComponentConfig(
        heartbeat_interval_secs: 5,    // Heartbeat frequency
        memdb_batch_size: 50,          // Results batch size
    ),
)
```

## Troubleshooting

### TLS Certificate Errors

**Error**: "certificate not valid for name..."

**Solution**: Ensure your collector config uses the correct hostname. The certificates are validated against the connection target:

- If connecting to `127.0.0.1`, certificates must have `SAN=IP:127.0.0.1`
- If connecting to `localhost`, certificates must have `SAN=DNS:localhost`
- If connecting to a hostname, certificates must have `SAN=DNS:hostname`

### Certificate Mismatch

Regenerate certificates if needed:

```bash
./generate_certs.sh --all
```

### Config File Not Found

Ensure you specify the correct path with `--config`:

```bash
# Current directory
./target/release/zzping-database --config ./config/database.ron

# Relative path from repo root (when running from repo root)
./target/release/zzping-database --config config/database.ron
```

## Git Policy

**Tracked files** (in git):
- `.ron.example` files (reference examples)
- `README.md` (this file)

**Ignored files** (not committed):
- `*.ron` (actual configuration files with paths/IPs)

This prevents accidental commits of environment-specific settings while keeping examples for reference.

## Development Workflow

For quick local testing:

```bash
# Copy examples to working configs
cp config/database.ron.example config/database.ron
cp config/collector.ron.example config/collector.ron

# Run from repo root
./target/release/zzping-database --config config/database.ron &
sleep 1
./target/release/zzping-collector --config config/collector.ron

# Kill database
fg
# Ctrl+C to stop
```

## Next Steps

See [SETUP.md](../SETUP.md) for complete setup instructions.
