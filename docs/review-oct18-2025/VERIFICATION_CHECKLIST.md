# Quick Verification Checklist

Use this checklist to verify the current state of your project before moving forward with the action plan.

## 1. Certificate Generation - Is It Working?

```bash
# Test 1: Do certificates exist?
ls -la test_certs/
# Expected output: ca.pem, ca.key, database.pem, database.key, collector.pem, collector.key

# Test 2: Do certificates have SAN extensions?
openssl x509 -text -noout -in test_certs/database.pem | grep -A2 "Subject Alternative Name"
# Expected output: Should show "Subject Alternative Name" with "DNS:root"

# Test 3: Check certificate validity dates
openssl x509 -noout -dates -in test_certs/database.pem
# Expected output: notBefore and notAfter dates (should be valid for 365 days)
```

**If any of these fail:**
- [ ] Run `./generate_certs.sh --all` from the repo root
- [ ] Re-run the tests above
- [ ] If still failing, examine openssl output for specific errors

---

## 2. Project Build Status

```bash
# Test 1: Does the project compile?
cargo build --release 2>&1 | head -50

# Test 2: Any compilation errors?
cargo check --all 2>&1 | grep -i error | head -20

# Test 3: Do the main binaries exist?
ls -la target/release/zzping-collector target/release/zzping-database
```

**If build fails:**
- [ ] Document the error
- [ ] This is likely blocking our action plan

---

## 3. Component Dependencies - Are They Correct?

```bash
# Test 1: Does zznet-api have any zzping- dependencies?
grep -r "zzping" src/net/zznet-api/Cargo.toml

# Test 2: Does zznet-session have any zzping- dependencies?
grep -r "zzping" src/net/zznet-session/Cargo.toml

# Test 3: Do components properly isolate?
grep -r "zzpinger" src/components/zzintent-config/Cargo.toml
# Expected: No match (components should not depend on each other)
```

**If any zznet-* crate depends on zzping-*:**
- [ ] This contradicts the validation and needs investigation

---

## 4. Config Files Exist

```bash
# Test 1: Check config directory
ls -la src/apps/zzping-collector/config/
ls -la src/apps/zzping-database/config/

# Test 2: Can we read a config?
cat src/apps/zzping-collector/config/collector.ron 2>/dev/null || echo "Not found"
```

**If configs missing:**
- [ ] They may need to be created
- [ ] Or may be using command-line args instead

---

## 5. Integration Tests - Are They Registered?

```bash
# Test 1: Are there any integration tests?
find . -name "*.rs" -path "*/tests/*" | head -10

# Test 2: Are they registered in Cargo.toml?
grep -A5 "^\[\[test\]\]" Cargo.toml

# Test 3: Try running tests
cargo test --lib 2>&1 | head -50
```

**If tests don't run:**
- [ ] Integration tests may not be registered
- [ ] This is part of Priority #2 in the action plan

---

## 6. Quick Connectivity Test

```bash
# Only run this if certs exist and build succeeds

# Terminal 1: Start database server
cargo run --release --bin zzping-database -- --config src/apps/zzping-database/config/database.ron 2>&1 | head -20

# Terminal 2: Try to connect collector (in parallel)
timeout 5 cargo run --release --bin zzping-collector -- --config src/apps/zzping-collector/config/collector.ron 2>&1 | head -30
```

**Expected Behavior:**
- Database should start listening on configured port
- Collector should attempt connection and either succeed or show meaningful TLS error
- After 5 seconds, collector should exit gracefully

**What to look for:**
- [ ] "TLS handshake completed successfully" = ✅ Good!
- [ ] TLS error with specific reason = 🔍 Diagnostic info needed
- [ ] "connection refused" = Database not listening
- [ ] "Connection timed out" = Network issue

---

## 7. Current State Assessment

After running the above checks, answer these questions:

- [x] **Certificates:** Do test certs exist and have SAN extensions?
- [x] **Build:** Does `cargo build` succeed?
- [x] **Dependencies:** Are component dependencies correct?
- [x] **Configs:** Do config files exist for each app?
- [x] **Tests:** Are integration tests present?
- [ ] **Connectivity:** Can the apps attempt TLS connection?

---

## Summary

Based on your answers above:

| Scenario | Next Step |
|----------|-----------|
| All checks pass ✅ | You're in good shape. Move to action plan Priority #2 (test harness fixes). |
| Build fails ❌ | This is blocking. Debug compilation errors first. |
| No certs exist ❌ | Run `./generate_certs.sh --all` and re-check. |
| Certs fail validation ❌ | Either script issue or environment issue. Share certificate inspection output. |
| Connectivity fails with TLS error 🔍 | Capture the full error message. This is diagnostic data for Priority #1. |
| Connectivity succeeds ✅✅ | Excellent! The foundation is working. Proceed directly to data pipeline implementation. |

---

## Once You Complete This Checklist

Share the results, and we'll:
1. Identify which Priority items in the action plan are still needed
2. Potentially reorder priorities based on actual state
3. Begin implementation with confidence that the foundation is solid
