# Certificate SAN Configuration Reference

## Quick Reference: Why Localhost?

| Aspect | Before (root) | After (localhost) | Why Changed |
|--------|---------------|-------------------|------------|
| Certificate SAN | `DNS:root` | `DNS:localhost` | Matches connection hostname |
| Collector config | `127.0.0.1` | `localhost` | Aligns with certificate SAN |
| TLS validation | ❌ Failed | ✅ Passes | SAN matches hostname |
| Connection type | IP-based | Hostname-based | Standard for local dev |

## How TLS Certificate Validation Works

```
┌─────────────────────────────────────┐
│ Collector connects to: localhost:8443
└──────────────────┬──────────────────┘
                   ↓
┌──────────────────────────────────────────┐
│ Rustls checks: Does cert SAN match?      │
│ Connection hostname: "localhost"         │
│ Cert SAN: "DNS:localhost"                │
└──────────────────┬───────────────────────┘
                   ↓
              ✅ Match!
                   ↓
         TLS Handshake Succeeds
```

## Common Issues and Fixes

### Error: "certificate not valid for name \"127.0.0.1\"; certificate is only valid for DnsName(\"localhost\")"

**Problem**: Connection made via IP, but certificate uses hostname SAN

**Solution**:
```ron
// Change this in collector.ron:
database_host: "localhost",  // ← Use hostname instead of IP
```

### Error: "certificate is only valid for DnsName(\"root\")"

**Problem**: Old certificates still have `SAN=DNS:root`

**Solution**:
```bash
# Regenerate certificates
./generate_certs.sh --all
```

### Connection Timeout or Refused

**Problem**: Possible certificate validation issue during handshake

**Debug**:
```bash
# Check what SAN is in the certificate
openssl x509 -in test_certs/database.pem -text -noout | grep SAN

# Check what hostname collector is using
grep "database_host:" config/collector.ron

# They should match!
```

## Configuration Checklist

✅ **Before running connectivity test, verify:**

- [ ] Certificates exist: `test_certs/database.pem`, `test_certs/collector.pem`
- [ ] Certificates have correct SAN: `DNS:localhost`
  ```bash
  openssl x509 -in test_certs/database.pem -text -noout | grep SAN
  ```
- [ ] Collector config uses localhost:
  ```bash
  grep "database_host:" config/collector.ron
  # Should show: database_host: "localhost",
  ```
- [ ] Config files exist: `config/database.ron`, `config/collector.ron`

## Regenerating Certificates

```bash
# Regenerate all certificates with localhost SAN
./generate_certs.sh --all

# Verify they have correct SAN
openssl x509 -in test_certs/database.pem -text -noout | grep -A 1 "Subject Alternative Name"

# Should output:
# X509v3 Subject Alternative Name:
#     DNS:localhost
```

## Why Not Use IP-Based SAN?

While theoretically possible with `SAN=IP:127.0.0.1`, hostname-based SAN is preferred because:

1. **Standard Practice**: Hostnames are the standard for TLS certificates
2. **Flexibility**: Enables both `localhost` and potentially `127.0.0.1` via hostname resolution
3. **Portability**: Works across different systems without IP address changes
4. **Development**: "localhost" is the conventional hostname for local services

## If You Need to Change to IP-Based

If your deployment requires IP-based SAN instead:

1. Modify `generate_certs.sh`:
   ```bash
   # Change this line in both generate_collector() and generate_database():
   subjectAltName=IP:127.0.0.1
   ```

2. Regenerate certificates:
   ```bash
   ./generate_certs.sh --all
   ```

3. Update collector config:
   ```ron
   // In config/collector.ron:
   database_host: "127.0.0.1",
   ```

## Testing Connectivity

Run the automated integration test:

```bash
# Full test with output
cargo test --release --package zzping-database --test connectivity_integration_test -- --ignored --test-threads=1

# Expected output:
# test test_connectivity_database_to_collector ... ok
# test result: ok. 1 passed
```

---

See `generate_certs.sh` for inline documentation of certificate generation process.
