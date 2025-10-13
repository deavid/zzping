# TLS/mTLS Debugging Guide for ZZPing

**Purpose:** Comprehensive guide for debugging TLS and mTLS connection issues
**Last Updated:** October 13, 2025
**Critical for:** Phase 4 (Collector), Phase 5 (Database), Phase 6 (Integration)

---

## Overview

ZZPing uses **mutual TLS (mTLS)** for all collector-database communication. This means:
- Database has a server certificate
- Each collector has a client certificate
- Both verify each other using a common CA certificate

**This is MORE complex than regular HTTPS!**

---

## TLS Fundamentals

### What is mTLS?

```
Regular TLS (HTTPS):
Client → Verifies → Server certificate

Mutual TLS (mTLS):
Client ← Verifies → Server certificate
Server ← Verifies → Client certificate
```

### Certificate Chain

```
CA (Certificate Authority)
├── Signs → Database Server Certificate
└── Signs → Collector Client Certificate

Both database and collector trust the CA,
so they can verify each other's certificates.
```

### Required Files

**For Database (Server):**
```
ca.pem          - CA certificate (to verify client certs)
database.pem    - Server certificate (presents to clients)
database.key    - Server private key (keeps secret!)
```

**For Collector (Client):**
```
ca.pem          - CA certificate (to verify server cert)
collector.pem   - Client certificate (presents to server)
collector.key   - Client private key (keeps secret!)
```

**CRITICAL:** All use the SAME ca.pem file!

---

## Common TLS Errors and Solutions

### Error 1: "received fatal alert: BadCertificate"

**Full Error:**
```
Error: TLS handshake failed: received fatal alert: BadCertificate
```

**Meaning:** The peer rejected your certificate as invalid.

**Systematic Debug:**

#### Step 1: Verify certificate files exist

```bash
ls -la test_certs/
# Must see:
# ca.pem
# database.pem
# database.key
# collector.pem
# collector.key

# If any missing:
./generate_certs.sh
```

#### Step 2: Verify certificate validity

```bash
# Check database certificate
openssl x509 -in test_certs/database.pem -noout -dates
# Output:
# notBefore=Oct  1 00:00:00 2025 GMT
# notAfter=Oct  1 00:00:00 2026 GMT

# Check collector certificate
openssl x509 -in test_certs/collector.pem -noout -dates

# If expired (notAfter < today):
./generate_certs.sh  # Regenerate certificates
```

#### Step 3: Verify certificate chain

```bash
# Database cert must be signed by CA
openssl verify -CAfile test_certs/ca.pem test_certs/database.pem
# Expected: test_certs/database.pem: OK
# If error: Certificate chain is broken, regenerate

# Collector cert must be signed by CA
openssl verify -CAfile test_certs/ca.pem test_certs/collector.pem
# Expected: test_certs/collector.pem: OK
```

#### Step 4: Check certificate details

```bash
# View database certificate
openssl x509 -in test_certs/database.pem -noout -text | grep -A 5 "Subject:"
# Should see: CN=database or CN=localhost

# View collector certificate
openssl x509 -in test_certs/collector.pem -noout -text | grep -A 5 "Subject:"
# Should see: CN=collector-01 or similar
```

#### Step 5: Verify configuration uses correct files

```ron
// database.ron:
DatabaseConfig(
    tls: TlsConfig(
        ca_cert_path: "test_certs/ca.pem",         // ✅ Same CA
        server_cert_path: "test_certs/database.pem", // ✅ Server cert
        server_key_path: "test_certs/database.key",  // ✅ Server key
    ),
)

// collector.ron:
CollectorConfig(
    tls: TlsConfig(
        ca_cert_path: "test_certs/ca.pem",          // ✅ Same CA
        client_cert_path: "test_certs/collector.pem", // ✅ Client cert
        client_key_path: "test_certs/collector.key",  // ✅ Client key
    ),
)
```

**Common Mistakes:**
- ❌ Using different CA files
- ❌ Using client cert for server
- ❌ Using server cert for client
- ❌ Wrong file paths in config

---

### Error 2: "received fatal alert: UnknownCA"

**Full Error:**
```
Error: TLS handshake failed: received fatal alert: UnknownCA
```

**Meaning:** The CA that signed your certificate is not trusted by the peer.

**Systematic Debug:**

#### Step 1: Verify both use same CA file

```bash
# Check database config points to same CA
grep "ca_cert_path" src/apps/zzping-database/database.ron
# Should be: ca_cert_path: "test_certs/ca.pem"

# Check collector config points to same CA
grep "ca_cert_path" src/apps/zzping-collector/collector.ron
# Should be: ca_cert_path: "test_certs/ca.pem"

# MUST BE THE SAME FILE!
```

#### Step 2: Verify CA signed both certificates

```bash
# Check database cert signed by this CA
openssl verify -CAfile test_certs/ca.pem test_certs/database.pem
# Expected: OK

# Check collector cert signed by this CA
openssl verify -CAfile test_certs/ca.pem test_certs/collector.pem
# Expected: OK

# If either fails:
./generate_certs.sh  # Regenerate all from same CA
```

#### Step 3: Check CA certificate itself

```bash
# View CA details
openssl x509 -in test_certs/ca.pem -noout -text | grep -A 5 "Subject:"
# Should see: CN=ZZPing Test CA or similar

# Check CA is self-signed (issuer == subject)
openssl x509 -in test_certs/ca.pem -noout -issuer -subject
# Issuer and Subject should match
```

**Solution:** Regenerate all certificates from same CA:
```bash
./generate_certs.sh
# This creates:
# 1. New CA certificate
# 2. New database cert signed by this CA
# 3. New collector cert signed by this CA
```

---

### Error 3: "certificate has expired"

**Full Error:**
```
Error: TLS handshake failed: certificate has expired
```

**Meaning:** One of the certificates is past its expiration date.

**Systematic Debug:**

```bash
# Check all certificate expiration dates
for cert in test_certs/*.pem; do
    echo "=== $cert ==="
    openssl x509 -in "$cert" -noout -dates
done

# Look for:
# notAfter=<date>
# If notAfter < today → EXPIRED
```

**Solution:**
```bash
# Regenerate certificates
./generate_certs.sh

# Verify new dates
openssl x509 -in test_certs/database.pem -noout -dates
# notAfter should be ~1 year in future
```

---

### Error 4: "handshake failed: wrong side of connection"

**Full Error:**
```
Error: TLS handshake failed: wrong side of connection
```

**Meaning:** Using server TLS code where client is expected, or vice versa.

**Systematic Debug:**

#### Check collector uses CLIENT TLS:
```rust
// CORRECT for collector:
use rustls::ClientConfig;

let config = ClientConfig::builder()
    .with_safe_defaults()
    .with_root_certificates(root_store)  // ✅ Root certs to verify server
    .with_client_auth_cert(cert_chain, private_key)?;  // ✅ Client auth

let connector = TlsConnector::from(Arc::new(config));
let stream = connector.connect(server_name, tcp_stream).await?;
```

```rust
// WRONG for collector:
use rustls::ServerConfig;  // ❌ This is for servers!

let config = ServerConfig::builder()  // ❌ Wrong!
    .with_safe_defaults()
    .with_client_cert_verifier(verifier)  // ❌ Collector doesn't verify clients!
```

#### Check database uses SERVER TLS:
```rust
// CORRECT for database:
use rustls::ServerConfig;

let config = ServerConfig::builder()
    .with_safe_defaults()
    .with_client_cert_verifier(verifier)  // ✅ Verify client certs
    .with_single_cert(cert_chain, private_key)?;  // ✅ Server cert

let acceptor = TlsAcceptor::from(Arc::new(config));
let stream = acceptor.accept(tcp_stream).await?;
```

```rust
// WRONG for database:
use rustls::ClientConfig;  // ❌ This is for clients!

let config = ClientConfig::builder()  // ❌ Wrong!
    .with_safe_defaults()
    .with_root_certificates(root_store)  // ❌ Database doesn't connect out!
```

**Quick Rule:**
- **Collector** = Client = ClientConfig + TlsConnector + connect()
- **Database** = Server = ServerConfig + TlsAcceptor + accept()

---

### Error 5: "unable to get local issuer certificate"

**Full Error:**
```
Error: unable to get local issuer certificate
```

**Meaning:** Certificate chain is incomplete or CA file not loaded.

**Systematic Debug:**

#### Step 1: Check CA file is actually loaded

```rust
// CORRECT: Load CA certificate
let ca_file = File::open(&config.tls.ca_cert_path)
    .context("Failed to open CA file")?;
let mut ca_reader = BufReader::new(ca_file);
let ca_certs = certs(&mut ca_reader)?;  // Parse PEM

if ca_certs.is_empty() {
    return Err(Error::Config("No CA certificates found in file".into()));
}

// Add to root store
let mut root_store = RootCertStore::empty();
for cert in ca_certs {
    root_store.add(&Certificate(cert))
        .context("Failed to add CA cert")?;
}
```

```rust
// WRONG: Forgot to load CA
let mut root_store = RootCertStore::empty();
// ❌ Nothing added! No CA loaded!
```

#### Step 2: Verify CA file is valid PEM

```bash
# Check CA file format
openssl x509 -in test_certs/ca.pem -noout -text
# Should show certificate details

# If error "unable to load certificate":
file test_certs/ca.pem
# Should be: "PEM certificate"

# If wrong format, regenerate:
./generate_certs.sh
```

---

## TLS Configuration Patterns

### Pattern 1: Collector (Client) TLS Setup

```rust
use rustls::{ClientConfig, Certificate, PrivateKey, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::sync::Arc;
use std::fs::File;
use std::io::BufReader;

pub fn load_client_tls_config(
    ca_path: &str,
    cert_path: &str,
    key_path: &str,
) -> Result<Arc<ClientConfig>> {
    // 1. Load CA certificate (to verify server)
    let ca_file = File::open(ca_path)?;
    let mut ca_reader = BufReader::new(ca_file);
    let ca_certs: Vec<Certificate> = certs(&mut ca_reader)?
        .into_iter()
        .map(Certificate)
        .collect();

    let mut root_store = RootCertStore::empty();
    for cert in ca_certs {
        root_store.add(&cert)?;
    }

    // 2. Load client certificate
    let cert_file = File::open(cert_path)?;
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain: Vec<Certificate> = certs(&mut cert_reader)?
        .into_iter()
        .map(Certificate)
        .collect();

    // 3. Load client private key
    let key_file = File::open(key_path)?;
    let mut key_reader = BufReader::new(key_file);
    let mut keys: Vec<PrivateKey> = pkcs8_private_keys(&mut key_reader)?
        .into_iter()
        .map(PrivateKey)
        .collect();

    if keys.is_empty() {
        return Err(Error::Config("No private key found".into()));
    }
    let private_key = keys.remove(0);

    // 4. Build client config
    let config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_client_auth_cert(cert_chain, private_key)?;

    Ok(Arc::new(config))
}
```

### Pattern 2: Database (Server) TLS Setup

```rust
use rustls::{ServerConfig, Certificate, PrivateKey, RootCertStore};
use rustls::server::AllowAnyAuthenticatedClient;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::sync::Arc;
use std::fs::File;
use std::io::BufReader;

pub fn load_server_tls_config(
    ca_path: &str,
    cert_path: &str,
    key_path: &str,
) -> Result<Arc<ServerConfig>> {
    // 1. Load server certificate
    let cert_file = File::open(cert_path)?;
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain: Vec<Certificate> = certs(&mut cert_reader)?
        .into_iter()
        .map(Certificate)
        .collect();

    // 2. Load server private key
    let key_file = File::open(key_path)?;
    let mut key_reader = BufReader::new(key_file);
    let mut keys: Vec<PrivateKey> = pkcs8_private_keys(&mut key_reader)?
        .into_iter()
        .map(PrivateKey)
        .collect();

    if keys.is_empty() {
        return Err(Error::Config("No private key found".into()));
    }
    let private_key = keys.remove(0);

    // 3. Load CA certificate (to verify clients)
    let ca_file = File::open(ca_path)?;
    let mut ca_reader = BufReader::new(ca_file);
    let ca_certs: Vec<Certificate> = certs(&mut ca_reader)?
        .into_iter()
        .map(Certificate)
        .collect();

    let mut root_store = RootCertStore::empty();
    for cert in ca_certs {
        root_store.add(&cert)?;
    }

    // 4. Create client verifier (REQUIRE client certificates)
    let client_verifier = AllowAnyAuthenticatedClient::new(root_store);

    // 5. Build server config
    let config = ServerConfig::builder()
        .with_safe_defaults()
        .with_client_cert_verifier(Arc::new(client_verifier))
        .with_single_cert(cert_chain, private_key)?;

    Ok(Arc::new(config))
}
```

---

## Testing TLS Configuration

### Test 1: Verify Certificate Loading

```rust
#[test]
fn test_load_client_tls_config() {
    let config = load_client_tls_config(
        "test_certs/ca.pem",
        "test_certs/collector.pem",
        "test_certs/collector.key",
    );

    assert!(config.is_ok(), "Failed to load client TLS config: {:?}", config.err());
}

#[test]
fn test_load_server_tls_config() {
    let config = load_server_tls_config(
        "test_certs/ca.pem",
        "test_certs/database.pem",
        "test_certs/database.key",
    );

    assert!(config.is_ok(), "Failed to load server TLS config: {:?}", config.err());
}
```

### Test 2: Verify Certificate Chain

```rust
#[test]
fn test_certificate_chain_valid() {
    // Verify database cert signed by CA
    let output = std::process::Command::new("openssl")
        .args(&["verify", "-CAfile", "test_certs/ca.pem", "test_certs/database.pem"])
        .output()
        .expect("Failed to run openssl");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("OK"), "Database certificate verification failed: {}", stdout);

    // Verify collector cert signed by CA
    let output = std::process::Command::new("openssl")
        .args(&["verify", "-CAfile", "test_certs/ca.pem", "test_certs/collector.pem"])
        .output()
        .expect("Failed to run openssl");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("OK"), "Collector certificate verification failed: {}", stdout);
}
```

---

## Debugging Checklist

When you have a TLS error, work through this checklist:

### [ ] Step 1: Verify Files Exist
```bash
ls -la test_certs/ca.pem
ls -la test_certs/database.pem
ls -la test_certs/database.key
ls -la test_certs/collector.pem
ls -la test_certs/collector.key
```

### [ ] Step 2: Verify Certificates Not Expired
```bash
openssl x509 -in test_certs/database.pem -noout -dates
openssl x509 -in test_certs/collector.pem -noout -dates
```

### [ ] Step 3: Verify Certificate Chain
```bash
openssl verify -CAfile test_certs/ca.pem test_certs/database.pem
openssl verify -CAfile test_certs/ca.pem test_certs/collector.pem
```

### [ ] Step 4: Verify Configuration Files
```bash
# Check database uses server cert
grep "server_cert_path" src/apps/zzping-database/database.ron

# Check collector uses client cert
grep "client_cert_path" src/apps/zzping-collector/collector.ron

# Check both use same CA
grep "ca_cert_path" src/apps/zzping-database/database.ron
grep "ca_cert_path" src/apps/zzping-collector/collector.ron
```

### [ ] Step 5: Verify Code Uses Correct TLS Type
```bash
# Check database uses ServerConfig
grep "ServerConfig" src/apps/zzping-database/src/tls.rs

# Check collector uses ClientConfig
grep "ClientConfig" src/apps/zzping-collector/src/tls.rs
```

### [ ] Step 6: Test with OpenSSL
```bash
# Start database
./target/debug/zzping-database --config database.ron &

# Test TLS connection
openssl s_client -connect 127.0.0.1:8443 \
    -CAfile test_certs/ca.pem \
    -cert test_certs/collector.pem \
    -key test_certs/collector.key

# Should see:
# - "Verify return code: 0 (ok)"
# - Certificate chain printed
```

---

## Quick Reference: TLS Error → Solution

| Error | Likely Cause | Quick Fix |
|-------|--------------|-----------|
| BadCertificate | Certificate invalid or expired | Regenerate: `./generate_certs.sh` |
| UnknownCA | Different CA files | Ensure same ca.pem used everywhere |
| Certificate expired | Date past notAfter | Regenerate: `./generate_certs.sh` |
| Wrong side of connection | Using server code for client or vice versa | Check ClientConfig vs ServerConfig |
| Unable to load certificate | File not found or wrong format | Check file paths and permissions |
| Handshake timeout | Network issue or server not listening | Verify server is running |

---

## When All Else Fails

1. **Start fresh:**
   ```bash
   # Regenerate all certificates
   ./generate_certs.sh

   # Verify they work
   openssl verify -CAfile test_certs/ca.pem test_certs/database.pem
   openssl verify -CAfile test_certs/ca.pem test_certs/collector.pem
   ```

2. **Test in isolation:**
   ```bash
   # Test with openssl tools first
   openssl s_server -accept 8443 \
       -cert test_certs/database.pem \
       -key test_certs/database.key \
       -CAfile test_certs/ca.pem \
       -Verify 1

   # In another terminal:
   openssl s_client -connect 127.0.0.1:8443 \
       -cert test_certs/collector.pem \
       -key test_certs/collector.key \
       -CAfile test_certs/ca.pem
   ```

3. **Enable debug logging:**
   ```bash
   RUST_LOG=debug,rustls=trace cargo run
   # Look for detailed TLS handshake logs
   ```

---

**Remember:** TLS errors are almost always about:
1. Wrong certificate files
2. Expired certificates
3. Mismatched CA files
4. Using server config for client or vice versa

Work through the checklist systematically and you'll find it!
