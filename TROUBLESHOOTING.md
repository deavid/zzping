# Troubleshooting Guide

## Common Issues and Solutions

### TLS Handshake Failures

**Error**: `TLS handshake failed: invalid peer certificate`

**Causes**:
- Certificate CN/SAN doesn't match the server hostname/IP
- Certificate expired or not yet valid
- CA certificate not trusted

**Solutions**:
1. Check certificate validity:
   ```bash
   openssl verify -CAfile test_certs/ca.pem test_certs/database.pem
   ```

2. Ensure CN/SAN matches:
   - For IP connections, certificates need `subjectAltName = IP:127.0.0.1`
   - Regenerate certificates with correct SAN

3. For development, use test certificates from `./generate_certs.sh`

### Connection Refused

**Error**: `Connection refused`

**Causes**:
- Database not running
- Wrong host/port in collector config
- Firewall blocking connections

**Solutions**:
1. Check database is running: `ps aux | grep zzping-database`
2. Verify ports: `netstat -tlnp | grep 9443`
3. Check collector config host/port

### Certificate Rotation Issues

**Error**: Collectors can't connect after rotation

**Solutions**:
1. Ensure dual CA support is enabled in database config
2. Collectors must trust both old and new CA
3. Use `./scripts/generate_two_cas.sh` for testing

### Memory Leaks

**Symptoms**: Memory usage grows over time

**Debugging**:
1. Run stability test: `./scripts/run_short_stability.sh`
2. Monitor with `htop` or `ps`
3. Check for unbounded queues or connections

### Process Crashes

**Debugging**:
1. Enable logging: `RUST_LOG=debug ./target/release/zzping-database`
2. Check system resources: `free -h`, `df -h`
3. Look for panic messages in logs

### Performance Issues

**Symptoms**: High latency, low throughput

**Solutions**:
1. Run load test: `./scripts/load_test.sh 50 60`
2. Check CPU usage during load
3. Monitor network bandwidth
4. Profile with `cargo flamegraph`

## Logs and Debugging

### Log Levels

```bash
# Info level (default)
./target/release/zzping-database

# Debug level
RUST_LOG=debug ./target/release/zzping-database

# Trace level
RUST_LOG=trace ./target/release/zzping-database
```

### Key Log Messages

- `Database service ready - accepting connections`: Database started successfully
- `Accepted connection from X.X.X.X:PORT`: New collector connected
- `Connection handler error`: TLS or protocol error
- `Collector service running`: Collector started successfully

## Testing and Validation

### Quick Health Check

```bash
# Build and test
cargo build --release
cargo test

# Start minimal system
./target/release/zzping-database --config database.example.ron &
DB_PID=$!
sleep 2
./target/release/zzping-collector --config collector.example.ron &
COL_PID=$!
sleep 10

# Check processes
kill -0 $DB_PID && echo "Database running"
kill -0 $COL_PID && echo "Collector running"

# Cleanup
kill $COL_PID $DB_PID
```

### Certificate Validation

```bash
# Validate all certs
for cert in test_certs/*.pem; do
    if [[ $cert != *"ca.pem" ]]; then
        openssl verify -CAfile test_certs/ca.pem "$cert" || echo "Invalid: $cert"
    fi
done
```

## Getting Help

1. Check this guide first
2. Review recent commits and issues
3. Enable debug logging
4. Run tests to isolate issues
5. Check system resources

## Known Limitations

- Certificate rotation requires manual intervention
- No automatic collector discovery
- Memory usage scales with collector count
- Network partitions not handled gracefully