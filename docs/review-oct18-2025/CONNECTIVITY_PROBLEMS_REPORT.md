# Connectivity Test Report - October 18, 2025

## Executive Summary

**Status**: ❌ **BLOCKED - TLS Certificate Validation Failure**

The database and collector binaries now **successfully compile and start**, but **fail to establish a connection** due to a TLS certificate hostname validation mismatch.

**Root Cause**: The test certificates have `SAN=DNS:root`, but the collector is trying to connect to `127.0.0.1` (an IP address, not a hostname).

---

## Issues Found

### 1. TLS Certificate Hostname Mismatch ⛔

**Severity**: CRITICAL - Blocks all connectivity

**Evidence**:
```
TCP connection failed: Service error: TLS handshake failed:
invalid peer certificate: certificate not valid for name "127.0.0.1";
certificate is only valid for DnsName("root")
```

**Root Cause**:
- Database certificate SAN: `DNS:root`
- Collector connecting to: `127.0.0.1:8443` (IP address)
- Rustls/Tokio-rustls validates that the IP/hostname matches the certificate's SAN

**Why This Happened**:
The `generate_certs.sh` script generates all certificates with `SAN=DNS:root` (line 7 of the script):
```bash
#   ./generate_certs.sh --collector                   # Generate collector certificate (CN=collector, SAN=DNS:root)
```

But `collector.ron` is configured to connect to `127.0.0.1` (a numeric IP), not `root` (a DNS name).

**Solutions** (in priority order):

1. **Recommended**: Regenerate certificates with `SAN=DNS:localhost` or `SAN=IP:127.0.0.1`
   - Modify `generate_certs.sh` to accept a hostname parameter
   - Or create a new script: `generate_certs_localhost.sh`

2. **Alternative**: Update `collector.ron` to connect to `localhost` instead of `127.0.0.1`
   - Requires adding `127.0.0.1 localhost` to `/etc/hosts` (if not already there)
   - Less ideal for testing edge cases

3. **Workaround**: Disable TLS hostname validation (NOT for production)
   - Would require code changes in TLS setup

---

### 2. Configuration Files in Source Tree ⚠️

**Severity**: MEDIUM - Design/maintenance issue

**Problem**:
```
✗ src/apps/zzping-database/config/database.ron
✗ src/apps/zzping-collector/config/collector.ron
```

These config files were created by copying `.example.ron` files into `src/`. This is problematic because:

1. **Not `.gitignored`** - Will be committed if not excluded
2. **Wrong location** - Source directory, not runtime directory
3. **Conflicts with deployment** - No clear way to handle configs in deployment scenarios

**Why This Happened**:
I incorrectly assumed configs should be in the source tree. They should be:
- Generated/placed at runtime in a `.gitignored` directory (e.g., `runtime/`, `build/`)
- Or referenced from environment variables
- Or have a bootstrap process that generates them

**Solution**:
1. Add to `.gitignore`:
   ```
   src/apps/*/config/*.ron
   runtime/
   build/config/
   ```

2. Move configs to `.gitignored` location:
   ```bash
   mkdir -p runtime/config
   cp src/apps/zzping-*/Cargo.example.ron runtime/config/
   ```

3. Update CLI to support `--config` paths or environment variable `ZZPING_CONFIG_PATH`

---

### 3. Runtime Issues Fixed ✅

The following issues were successfully resolved:

#### a) LocalSet/Actix Compatibility
- **Problem**: `spawn_local` called outside of `task::LocalSet`
- **Fix**: Changed from `LocalSet::block_on(&rt, future)` to `rt.block_on(LocalSet::run_until(future))`
- **Files**: `src/apps/zzping-*/src/main.rs`

#### b) Rustls CryptoProvider Not Initialized
- **Problem**: Rustls couldn't determine which crypto provider to use (ring vs aws-lc-rs)
- **Fix**: Installed default provider early: `CryptoProvider::install_default(ring::default_provider())`
- **Files**: `src/apps/zzping-*/src/main.rs` and `src/apps/zzping-database/src/service.rs`

---

## Test Results

### Database Server
```
✅ Starts successfully
✅ Loads configuration from file
✅ Initializes all components (MemDB, IntentConfig, CState)
✅ Binds to 0.0.0.0:8443
✅ Loads TLS configuration
✅ Accepts TCP connections
✅ Graceful shutdown on SIGINT
```

### Collector Client
```
✅ Starts successfully
✅ Loads configuration from file
✅ Initializes all components (MemDB, IntentConfig, Pinger, CState)
❌ TLS handshake fails with certificate mismatch
❌ Cannot establish connection to database
```

### Connection Attempt
```
Database: "Accepted connection from 127.0.0.1:39080"
Collector: "TLS handshake failed: invalid peer certificate: certificate not valid for name "127.0.0.1""
Database: "Connection handler error: TLS error: received fatal alert: BadCertificate"
```

---

## Next Steps (Priority Order)

1. **🔴 URGENT - Fix TLS Certificate SAN**
   - Regenerate certificates with `localhost` or `127.0.0.1` as SAN
   - Update `generate_certs.sh` script
   - Re-run certificate generation
   - Delete old `src/apps/*/config/*.ron` files

2. **🟡 IMPORTANT - Fix Configuration File Locations**
   - Create `.gitignored` directories for runtime config
   - Move configs out of source tree
   - Update `.gitignore`
   - Update documentation on config file paths

3. **🟢 NICE-TO-HAVE - Improve Setup Process**
   - Create setup script that generates certs and configs automatically
   - Add bootstrap/initialization command to binaries
   - Document the full setup procedure

---

## Verification Commands

Once fixes are applied:

```bash
# Test 1: Certificate validation
openssl x509 -text -noout -in test_certs/database.pem | grep "Subject Alternative Name"
# Should show: DNS:localhost or IP:127.0.0.1

# Test 2: Start database
./target/release/zzping-database --config runtime/config/database.ron &

# Test 3: Run collector (in another terminal)
timeout 5 ./target/release/zzping-collector --config runtime/config/collector.ron

# Expected output:
# ✅ "TLS handshake completed successfully" or similar
# ✅ Connection established
# ✅ Both services exchange data without errors
```

---

## Code Changes Applied

### src/apps/zzping-database/src/main.rs
- Changed LocalSet usage from `LocalSet::block_on(&rt, future)` to `rt.block_on(LocalSet::run_until(future))`
- Added crypto provider installation in `async_main()`

### src/apps/zzping-database/src/service.rs
- Added crypto provider installation in `load_tls_config()`

### src/apps/zzping-collector/src/main.rs
- Same fixes as database

---

## Recommendations for Next Session

1. **Do not commit config files** in `src/apps/*/config/`
2. **Update certificate generation** before running connectivity tests
3. **Consider integration tests** that start both services programmatically rather than manual testing
4. **Document the full setup flow** in SETUP.md

---

## Appendix: Error Log

Full error from connection attempt:
```
2025-10-18T14:58:36.705477Z  INFO zzping_collector::service: Connecting to database at 127.0.0.1:8443...
2025-10-18T14:58:36.705604Z  INFO zzping_database::service: Accepted connection from 127.0.0.1:39080
2025-10-18T14:58:36.706257Z  ERROR zzping_collector::service: TCP connection failed: Service error:
  TLS handshake failed: invalid peer certificate: certificate not valid for name "127.0.0.1";
  certificate is only valid for DnsName("root")
2025-10-18T14:58:36.706299Z  ERROR zzping_database::service: Connection handler error for 127.0.0.1:39080:
  TLS error: TLS handshake failed with 127.0.0.1:39080: received fatal alert: BadCertificate
```
