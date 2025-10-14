# Phase 6 Implementation Checklist V3: Integration, Stability & MVP
# (UPDATED WITH PHASE 4/5 LEARNINGS - Use this version)

**Last Updated:** October 14, 2025 (Post Phase 4 Review)
**Target:** Week 6 (Integration, performance, certificate infrastructure, and MVP acceptance)
**Status:** Use this as the authoritative Phase 6 plan
**Goal:** Deliver a stable end-to-end ZZPing system that is production-ready for MVP

**🔄 CHANGES FROM V2:**
- ✅ Updated with Phase 4 implementation patterns
- ✅ Added LocalSet requirement for all Actix-based applications
- ✅ Incorporated TLS debugging patterns
- ✅ Added validation checklists from PR #26 review
- ✅ Updated test patterns (11-test minimum as baseline)
- ✅ Added graceful shutdown requirements
- ✅ Clarified binary crate vs library crate standards

---

## 🚨 BEFORE YOU START: Pre-flight Checklist

**STOP:** Do not write any code until you complete these verification steps.

### Environment Verification
- [ ] **VERIFY WORKSPACE:** `pwd` → ensure you're in repo root
- [ ] **VERIFY GIT STATUS:** `git status` - clean state
- [ ] **VERIFY PHASE 4 COMPLETE:** `cargo test -p zzping-collector` → all tests pass
- [ ] **VERIFY PHASE 5 COMPLETE:** `cargo test -p zzping-database` → all tests pass
- [ ] **VERIFY BASELINE BUILD:** `cargo build --workspace` → succeeds
- [ ] **CREATE FEATURE BRANCH:** `git checkout -b feature/phase6-integration`

### Critical Document Review
- [ ] **READ:** `PR26_PHASE4_REVIEW.md` - Quality standards and what worked
- [ ] **READ:** `PHASE4_CHECKLIST_V3.md` - Final collector patterns (if exists, else V2)
- [ ] **READ:** `PHASE5_CHECKLIST_V3.md` - Final database patterns
- [ ] **READ:** `TLS_DEBUGGING_GUIDE.md` - mTLS troubleshooting
- [ ] **READ:** `INTEGRATION_TESTING_GUIDE.md` - Test patterns
- [ ] **READ:** `AGENT_CODING_STANDARDS.md` - Code quality requirements

### Certificate Infrastructure Verification
- [ ] **VERIFY CERT GENERATION:** Run `./generate_certs.sh`
  - Expected: Creates test_certs/ directory with:
    - ca.pem, ca.key
    - database.pem, database.key
    - collector.pem, collector.key
- [ ] **VERIFY CERT VALIDITY:** Run `openssl verify -CAfile test_certs/ca.pem test_certs/database.pem`
  - Expected: "test_certs/database.pem: OK"

### Understanding Check
- [ ] **INTEGRATION SCOPE:** Can you explain what Phase 6 delivers?
  - Write it here: _________________________________________________
  - Should be: "End-to-end system with N collectors + 1 database running 24h+ without crashes, with cert rotation, performance baseline, and documented failure modes"

- [ ] **QUALITY STANDARDS:** What's the minimum test count per application?
  - Write it here: _________________________________________________
  - Should be: "11+ tests minimum (7 config + 4 service baseline from Phase 4)"

- [ ] **RUNTIME REQUIREMENTS:** What runtime pattern is mandatory?
  - Write it here: _________________________________________________
  - Should be: "LocalSet for all Actix-based applications (collector + database)"

**IF YOU CANNOT ANSWER THESE:** Stop and re-read the documentation.

---

## Success Criteria (MVP Definition)

Phase 6 is successful when ALL of the following are verified:

### Core Functionality
- [ ] **24-Hour Stability:** System runs 24h without crashes (3 collectors + 1 database)
- [ ] **Connection Stability:** All collectors maintain connection for entire 24h period
- [ ] **Heartbeat Flow:** Collectors send heartbeats every 5s, database tracks all
- [ ] **Data Persistence:** Database persists all data, recovers after restart
- [ ] **Graceful Shutdown:** SIGTERM/SIGINT handled cleanly by all processes

### Certificate Infrastructure
- [ ] **Cert Generation:** Automated cert generation for N collectors + database
- [ ] **Cert Rotation:** Can rotate certs without downtime (dual CA support)
- [ ] **Cert Validation:** All mTLS handshakes succeed with valid certs
- [ ] **Cert Revocation:** Invalid/expired certs are rejected

### Performance
- [ ] **Baseline Established:** 100 collectors @10Hz sustained (documented)
- [ ] **Resource Usage:** Memory and CPU usage documented and reasonable
- [ ] **No Leaks:** Memory usage stable over 24h period
- [ ] **Latency Target:** P99 heartbeat latency <100ms under baseline load

### Testing & Documentation
- [ ] **Integration Tests:** E2E test suite passes in CI
- [ ] **Unit Test Coverage:** All applications meet 11+ test minimum
- [ ] **Documentation:** README, troubleshooting guide, runbook complete
- [ ] **Known Issues:** All limitations and failure modes documented

---

## Phase Overview - Day-by-day

| Day | Goal | Key Deliverables |
|-----|------|------------------|
| 1 | E2E Harness | Integration test framework, automated cert gen ✅ (implemented: tests/e2e_smoke.rs, tests/fixtures, scripts/generate_multi_certs.sh) |
| 2 | Certificate Rotation | Multi-CA support, rotation without downtime ✅ (implemented: TlsConfig.ca_cert_paths, scripts/generate_two_cas.sh, tests/cert_rotation_test.rs with explicit TLS handshake + tonic Heartbeat RPC) |
| 3 | 24h Stability | Long-running test, restart recovery, leak detection (in-progress: stability test scaffold added, memory monitoring to be wired) |
| 4 | Performance Baseline | Load testing, profiling, optimization |
| 5 | Chaos Testing | Network partitions, process kills, resilience |
| 6 | Documentation | README, runbook, troubleshooting, postmortem |
| 7 | PR Preparation | Final polish, review checklist, merge readiness |

---

## Day 1: End-to-End Harness and Cert Automation

**GOAL:** Create reproducible e2e test harness that starts multiple collectors + database, verifies connection, and validates data flow.

**Estimated time:** 6 hours

---

### Morning: E2E Test Framework - [3 hours]

#### Step 1: Create Integration Test Structure - [60 min]

**ACTIONS:**
- [ ] Create `tests/e2e_smoke.rs` at repository root:
  ```rust
  //! End-to-end smoke tests for collector + database integration.
  //!
  //! This test suite starts real collector and database processes
  //! and verifies they can communicate over mTLS.

  use std::process::{Command, Child};
  use std::time::Duration;
  use tokio::time::sleep;

  /// Helper to start database process
  fn start_database() -> std::io::Result<Child> {
      Command::new("./target/debug/zzping-database")
          .arg("--config")
          .arg("tests/fixtures/database-e2e.ron")
          .spawn()
  }

  /// Helper to start collector process
  fn start_collector(id: &str) -> std::io::Result<Child> {
      Command::new("./target/debug/zzping-collector")
          .arg("--config")
          .arg(format!("tests/fixtures/collector-{}-e2e.ron", id))
          .spawn()
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_e2e_single_collector_connects() {
      // Build binaries first
      assert!(Command::new("cargo")
          .args(&["build", "--bin", "zzping-database", "--bin", "zzping-collector"])
          .status()
          .unwrap()
          .success());

      // Start database
      let mut db = start_database().expect("Failed to start database");

      // Wait for database to be ready
      sleep(Duration::from_secs(2)).await;

      // Start collector
      let mut collector = start_collector("01").expect("Failed to start collector");

      // Wait for connection
      sleep(Duration::from_secs(5)).await;

      // TODO: Verify connection established (check logs or metrics)

      // Cleanup
      collector.kill().ok();
      db.kill().ok();
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_e2e_multiple_collectors_connect() {
      // Build binaries
      assert!(Command::new("cargo")
          .args(&["build", "--bin", "zzping-database", "--bin", "zzping-collector"])
          .status()
          .unwrap()
          .success());

      // Start database
      let mut db = start_database().expect("Failed to start database");
      sleep(Duration::from_secs(2)).await;

      // Start 3 collectors
      let mut collectors = vec![];
      for id in ["01", "02", "03"] {
          let collector = start_collector(id).expect(&format!("Failed to start collector {}", id));
          collectors.push(collector);
          sleep(Duration::from_millis(500)).await;
      }

      // Wait for all connections
      sleep(Duration::from_secs(10)).await;

      // TODO: Verify all collectors connected

      // Cleanup
      for mut collector in collectors {
          collector.kill().ok();
      }
      db.kill().ok();
  }
  ```

- [ ] Create directory: `mkdir -p tests/fixtures`
- [ ] Create `tests/fixtures/database-e2e.ron`:
  ```ron
  DatabaseConfig(
      bind_host: "127.0.0.1",
      bind_port: 9443,  // Different port for testing
      tls: TlsConfig(
          ca_cert_path: "test_certs/ca.pem",
          server_cert_path: "test_certs/database.pem",
          server_key_path: "test_certs/database.key",
      ),
      components: ComponentConfig(
          stale_timeout_secs: 30,
          max_collectors: 10,
      ),
  )
  ```

- [ ] Create `tests/fixtures/collector-01-e2e.ron` (and 02, 03):
  ```ron
  CollectorConfig(
      collector_id: "collector-01-e2e",
      database_host: "127.0.0.1",
      database_port: 9443,
      tls: TlsConfig(
          ca_cert_path: "test_certs/ca.pem",
          client_cert_path: "test_certs/collector.pem",
          client_key_path: "test_certs/collector.key",
      ),
      components: ComponentConfig(
          heartbeat_interval_secs: 5,
          memdb_batch_size: 50,
      ),
  )
  ```

- [ ] **VERIFY:** `cargo test --test e2e_smoke -- --nocapture`
  - Expected: Tests run (may fail if connection logic incomplete)

- [ ] **COMMIT:** `git add -A && git commit -m "test(e2e): Add smoke test framework"`

---

### Afternoon: Certificate Automation - [3 hours]

#### Step 2: Enhanced Cert Generation Script - [120 min]

**ACTIONS:**
- [ ] Create `scripts/generate_multi_certs.sh`:
  ```bash
  #!/bin/bash
  # Generate certificates for N collectors + database

  set -e

  NUM_COLLECTORS=${1:-3}
  CERT_DIR="test_certs_multi"

  echo "Generating certificates for $NUM_COLLECTORS collectors..."

  # Create directory
  mkdir -p "$CERT_DIR"
  cd "$CERT_DIR"

  # 1. Generate CA
  if [ ! -f ca.key ]; then
      echo "Generating CA certificate..."
      openssl genrsa -out ca.key 2048
      openssl req -new -x509 -days 3650 -key ca.key -out ca.pem \
          -subj "/C=US/ST=Test/L=Test/O=ZZPing Test/CN=Test CA"
  fi

  # 2. Generate database certificate
  if [ ! -f database.key ]; then
      echo "Generating database certificate..."
      openssl genrsa -out database.key 2048
      openssl req -new -key database.key -out database.csr \
          -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=database"
      openssl x509 -req -in database.csr -CA ca.pem -CAkey ca.key \
          -CAcreateserial -out database.pem -days 365
      rm database.csr
  fi

  # 3. Generate collector certificates
  for i in $(seq 1 $NUM_COLLECTORS); do
      COLLECTOR_ID=$(printf "collector-%02d" $i)
      if [ ! -f "${COLLECTOR_ID}.key" ]; then
          echo "Generating certificate for ${COLLECTOR_ID}..."
          openssl genrsa -out "${COLLECTOR_ID}.key" 2048
          openssl req -new -key "${COLLECTOR_ID}.key" -out "${COLLECTOR_ID}.csr" \
              -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=${COLLECTOR_ID}"
          openssl x509 -req -in "${COLLECTOR_ID}.csr" -CA ca.pem -CAkey ca.key \
              -CAcreateserial -out "${COLLECTOR_ID}.pem" -days 365
          rm "${COLLECTOR_ID}.csr"
      fi
  done

  echo "Certificate generation complete!"
  echo "CA: ca.pem"
  echo "Database: database.pem, database.key"
  for i in $(seq 1 $NUM_COLLECTORS); do
      COLLECTOR_ID=$(printf "collector-%02d" $i)
      echo "Collector $i: ${COLLECTOR_ID}.pem, ${COLLECTOR_ID}.key"
  done
  ```

- [ ] Make executable: `chmod +x scripts/generate_multi_certs.sh`

- [ ] **VERIFY:** Run script:
  ```bash
  ./scripts/generate_multi_certs.sh 5
  # Should create test_certs_multi/ with 5 collector certs + database cert + CA
  ```

- [ ] **VERIFY:** Test certificates:
  ```bash
  cd test_certs_multi
  openssl verify -CAfile ca.pem database.pem
  openssl verify -CAfile ca.pem collector-01.pem
  # Expected: Both show "OK"
  ```

- [ ] **COMMIT:** `git add -A && git commit -m "feat(certs): Add multi-collector cert generation"`

---

### 🛑 CHECKPOINT 1: E2E Framework Complete

**VERIFY:**
- [ ] E2E smoke tests exist and compile
- [ ] Cert generation script works for N collectors
- [ ] All generated certs validate with openssl verify
- [ ] Test fixtures created for database + collectors
- [ ] All changes committed

---

## Day 2: Certificate Rotation Support

**GOAL:** Implement certificate rotation with dual CA support (old + new CA trusted simultaneously).

**Estimated time:** 6 hours

---

### Morning: Dual CA Support - [3 hours]

#### Step 1: Update Configuration for Multiple CAs - [90 min]

**ACTIONS:**
- [ ] Update `src/apps/zzping-database/src/config.rs`:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct TlsConfig {
      /// CA certificates for verifying client certificates (can be multiple)
      pub ca_cert_paths: Vec<String>,  // Changed from single path to Vec
      /// Server certificate (this database's identity)
      pub server_cert_path: String,
      /// Server private key
      pub server_key_path: String,
  }

  // Update validation
  impl DatabaseConfig {
      pub fn validate(&self) -> crate::error::Result<()> {
          // ... existing checks ...

          // Validate ALL CA cert paths exist
          if self.tls.ca_cert_paths.is_empty() {
              return Err(crate::error::DatabaseError::Config(
                  "At least one CA certificate path required".into(),
              ));
          }

          for ca_path in &self.tls.ca_cert_paths {
              if !std::path::Path::new(ca_path).exists() {
                  return Err(crate::error::DatabaseError::Config(format!(
                      "CA certificate not found: {}",
                      ca_path
                  )));
              }
          }

          // ... rest of validation ...
      }
  }
  ```

- [ ] Update `src/apps/zzping-database/src/service.rs`:
  ```rust
  pub fn load_tls_config(tls: &crate::config::TlsConfig) -> Result<Arc<ServerConfig>> {
      // Load ALL CA certificates
      let mut root_store = RootCertStore::empty();

      for ca_path in &tls.ca_cert_paths {
          let ca_file = File::open(ca_path)
              .map_err(|e| DatabaseError::Config(format!("Failed to open CA file {}: {}", ca_path, e)))?;
          let mut ca_reader = BufReader::new(ca_file);
          let ca_certs: Vec<Certificate> = certs(&mut ca_reader)
              .map_err(|e| DatabaseError::Config(format!("Failed to parse CA certs from {}: {}", ca_path, e)))?
              .into_iter()
              .map(Certificate)
              .collect();

          if ca_certs.is_empty() {
              return Err(DatabaseError::Config(format!("No CA certificates in {}", ca_path)));
          }

          for cert in ca_certs {
              root_store
                  .add(&cert)
                  .map_err(|e| DatabaseError::Config(format!("Failed to add CA cert: {}", e)))?;
          }

          tracing::info!("Loaded CA certificates from: {}", ca_path);
      }

      // ... rest of TLS setup unchanged ...
  }
  ```

- [ ] Update example config `database.example.ron`:
  ```ron
  tls: TlsConfig(
      // Multiple CA certificates (for rotation support)
      ca_cert_paths: ["test_certs/ca.pem"],
      server_cert_path: "test_certs/database.pem",
      server_key_path: "test_certs/database.key",
  ),
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
- [ ] **VERIFY:** Update tests to use Vec for ca_cert_paths
- [ ] **VERIFY:** `cargo test -p zzping-database`

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Add dual CA support for cert rotation"`

---

### Afternoon: Rotation Testing - [3 hours]

#### Step 2: Create Rotation Test - [120 min]

**ACTIONS:**
- [ ] Create `tests/cert_rotation_test.rs`:
  ```rust
  //! Certificate rotation tests

  use std::process::Command;
  use std::fs;
  use std::time::Duration;
  use tokio::time::sleep;

  #[tokio::test]
  async fn test_database_accepts_old_and_new_ca_certs() {
      // Generate two CAs
      Command::new("sh")
          .args(&["-c", "cd test_certs_rotation && ../scripts/generate_two_cas.sh"])
          .status()
          .unwrap();

      // Start database with BOTH CAs trusted
      let mut db = Command::new("./target/debug/zzping-database")
          .arg("--config")
          .arg("tests/fixtures/database-dual-ca.ron")
          .spawn()
          .unwrap();

      sleep(Duration::from_secs(2)).await;

      // Start collector with OLD CA-signed cert
      let mut collector_old = Command::new("./target/debug/zzping-collector")
          .arg("--config")
          .arg("tests/fixtures/collector-old-ca.ron")
          .spawn()
          .unwrap();

      sleep(Duration::from_secs(3)).await;

      // Start collector with NEW CA-signed cert
      let mut collector_new = Command::new("./target/debug/zzping-collector")
          .arg("--config")
          .arg("tests/fixtures/collector-new-ca.ron")
          .spawn()
          .unwrap();

      sleep(Duration::from_secs(3)).await;

      // Both should connect successfully
      // TODO: Verify both are connected (check logs or database metrics)

      // Cleanup
      collector_old.kill().ok();
      collector_new.kill().ok();
      db.kill().ok();
  }
  ```

- [ ] Create `scripts/generate_two_cas.sh` (generates CA v1 and CA v2)

- [ ] **VERIFY:** `cargo test --test cert_rotation_test`

- [ ] **COMMIT:** `git add -A && git commit -m "test(certs): Add dual CA rotation test"`

---

### 🛑 CHECKPOINT 2: Certificate Rotation Support

**VERIFY:**
- [ ] Database accepts multiple CA certificates
- [ ] Collectors with certs from different CAs can both connect
- [ ] Rotation test passes
- [ ] All changes committed

---

## Day 3: 24-Hour Stability Testing

**GOAL:** Validate system runs 24+ hours without crashes, memory leaks, or connection drops.

**Estimated time:** 8+ hours (mostly run time)

---

### Morning: Stability Test Harness - [4 hours]

#### Step 1: Create Long-Running Test - [180 min]

**ACTIONS:**
- [ ] Create `tests/stability_test.rs`:
  ```rust
  //! Long-running stability tests
  //!
  //! Run with: cargo test --test stability_test -- --nocapture --ignored
  //!
  //! These tests are marked #[ignore] because they take hours to run.

  use std::process::{Command, Child};
  use std::time::{Duration, Instant};
  use tokio::time::sleep;

  const TEST_DURATION_SECS: u64 = if cfg!(debug_assertions) {
      60  // 1 minute for CI
  } else {
      86400  // 24 hours for local
  };

  #[tokio::test]
  #[ignore]  // Run explicitly with --ignored flag
  async fn test_24h_stability_3_collectors() {
      println!("Starting 24-hour stability test...");
      println!("Duration: {} seconds", TEST_DURATION_SECS);

      // Build binaries
      assert!(Command::new("cargo")
          .args(&["build", "--release", "--bin", "zzping-database", "--bin", "zzping-collector"])
          .status()
          .unwrap()
          .success());

      // Start database
      let mut db = Command::new("./target/release/zzping-database")
          .arg("--config")
          .arg("tests/fixtures/database-stability.ron")
          .spawn()
          .expect("Failed to start database");

      sleep(Duration::from_secs(5)).await;

      // Start 3 collectors
      let mut collectors = vec![];
      for id in ["01", "02", "03"] {
          let collector = Command::new("./target/release/zzping-collector")
              .arg("--config")
              .arg(format!("tests/fixtures/collector-{}-stability.ron", id))
              .spawn()
              .expect(&format!("Failed to start collector {}", id));
          collectors.push(collector);
          sleep(Duration::from_secs(2)).await;
      }

      println!("All processes started. Running for {} seconds...", TEST_DURATION_SECS);

      let start = Instant::now();
      let mut check_interval = Duration::from_secs(60);  // Check every minute

      while start.elapsed() < Duration::from_secs(TEST_DURATION_SECS) {
          sleep(check_interval).await;

          // Check all processes are still alive
          match db.try_wait() {
              Ok(Some(status)) => {
                  panic!("Database exited unexpectedly with status: {}", status);
              }
              Ok(None) => {
                  // Still running
                  println!("Database still running after {:?}", start.elapsed());
              }
              Err(e) => {
                  panic!("Error checking database status: {}", e);
              }
          }

          for (i, collector) in collectors.iter_mut().enumerate() {
              match collector.try_wait() {
                  Ok(Some(status)) => {
                      panic!("Collector {} exited unexpectedly with status: {}", i, status);
                  }
                  Ok(None) => {
                      // Still running
                  }
                  Err(e) => {
                      panic!("Error checking collector {} status: {}", i, e);
                  }
              }
          }

          println!("All processes healthy. Elapsed: {:?} / {:?}",
                   start.elapsed(),
                   Duration::from_secs(TEST_DURATION_SECS));
      }

      println!("Stability test PASSED! All processes ran for full duration.");

      // Graceful shutdown
      for mut collector in collectors {
          collector.kill().ok();
      }
      db.kill().ok();
  }
  ```

- [ ] **VERIFY:** Short run (1 min in CI):
  ```bash
  cargo test --test stability_test -- --nocapture --ignored
  ```

- [ ] **COMMIT:** `git add -A && git commit -m "test(stability): Add 24h stability test"`

---

### Afternoon: Memory Leak Detection - [4 hours]

#### Step 2: Add Memory Monitoring - [120 min]

**ACTIONS:**
- [ ] Update stability test with memory checks:
  ```rust
  // Add to stability_test.rs
  use sysinfo::{System, SystemExt, ProcessExt};

  fn get_process_memory(pid: sysinfo::Pid) -> u64 {
      let mut system = System::new_all();
      system.refresh_all();

      if let Some(process) = system.process(pid) {
          process.memory()
      } else {
          0
      }
  }

  // In test, track memory over time
  let db_pid = sysinfo::Pid::from_u32(db.id() as u32);
  let initial_memory = get_process_memory(db_pid);

  // ... during checks ...
  let current_memory = get_process_memory(db_pid);
  let memory_growth = current_memory - initial_memory;
  println!("Database memory: {} KB (growth: {} KB)",
           current_memory / 1024,
           memory_growth / 1024);

  // Fail if memory grows more than 500MB
  assert!(memory_growth < 500 * 1024 * 1024,
          "Memory leak detected: grew {} MB",
          memory_growth / (1024 * 1024));
  ```

- [ ] Add `sysinfo` to dev-dependencies in root Cargo.toml:
  ```toml
  [dev-dependencies]
  sysinfo = "0.30"
  ```

- [ ] **VERIFY:** Run with memory monitoring

- [ ] **COMMIT:** `git add -A && git commit -m "test(stability): Add memory leak detection"`

---

### 🛑 CHECKPOINT 3: Stability Testing Complete

**VERIFY:**
- [ ] 1-minute stability test passes in CI
- [ ] Memory monitoring works
- [ ] All processes start and run without crashes
- [ ] Graceful shutdown works

**ACTION:** Run 24-hour test manually overnight

---

## Day 4-7: Remaining Tasks (Summary)

### Day 4: Performance Baseline
- Create load testing harness (100 collectors)
- Measure throughput, latency, resource usage
- Document baseline metrics
- Identify and fix bottlenecks

### Day 5: Chaos Testing
- Network partition tests
- Process kill/restart tests
- Database crash recovery tests
- Collector reconnection tests

### Day 6: Documentation
- Update README with setup instructions
- Create troubleshooting guide
- Document known limitations
- Create runbook for operations

### Day 7: PR Preparation
- Run full test suite
- Review all code for quality
- Update CHANGELOG
- Create comprehensive PR description
- Merge to main

---

## Quality Gates (Must Pass Before Merge)

### Code Quality
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` passes (all tests)
- [ ] `cargo build --workspace --release` succeeds
- [ ] No panics in any error paths

### Testing
- [ ] Collector: 11+ tests (Phase 4 baseline)
- [ ] Database: 11+ tests (Phase 5 baseline)
- [ ] Integration: 5+ e2e tests
- [ ] Stability: 1-minute test passes in CI

### Documentation
- [ ] README updated with Phase 6 changes
- [ ] All public APIs documented
- [ ] Troubleshooting guide complete
- [ ] Known issues documented

### Performance
- [ ] 24-hour test passes (manually verified)
- [ ] Memory usage stable (<500MB growth)
- [ ] Baseline documented (100 collectors)
- [ ] No resource leaks detected

---

## Common Mistakes (Based on Phase 4/5 Learnings)

### ✅ DO:
- Use LocalSet for all Actix applications
- Write tests before implementation (test-first)
- Create comprehensive example configs
- Validate all configuration fields
- Use proper error handling (no unwrap in production paths)
- Test both success and failure cases
- Document all public APIs
- Use signal handlers for graceful shutdown
- Run full test suite before committing

### ❌ DON'T:
- Skip writing tests "because it's obvious"
- Use production certificates in tests
- Hardcode configuration values
- Forget to check process exit status in tests
- Skip memory leak detection
- Merge without running 24h test manually
- Leave TODOs or FIXMEs in final code
- Rush the documentation

---

## Reference Commands

```bash
# Run short stability test (CI)
cargo test --test stability_test -- --nocapture --ignored

# Run 24-hour stability test (manual)
RUST_LOG=info cargo test --release --test stability_test -- --nocapture --ignored

# Generate multi-collector certs
./scripts/generate_multi_certs.sh 10

# Run e2e smoke tests
cargo test --test e2e_smoke -- --nocapture

# Full workspace test
cargo test --workspace

# Lint everything
cargo clippy --workspace --all-targets -- -D warnings

# Build release
cargo build --workspace --release
```

---

**END OF PHASE 6 CHECKLIST V3**

*This checklist incorporates all learnings from Phase 4 PR #26 review and Phase 5 implementation.*
