# Phase Checklist Improvements for AI Agent Execution

**Date:** October 13, 2025
**Purpose:** Lessons learned from Phase 3 to improve Phase 4, 5, 6 checklists
**Target:** Make checklists more "AI-executable" with fewer ambiguities

---

## Executive Summary

After analyzing the Phase 3 implementation by Jules and the PR25 review, we identified key patterns where AI agents struggle. This document proposes specific improvements to phase checklists to reduce ambiguity, enforce incremental development, and ensure completeness.

**Key Findings:**
1. **Missing explicit verification steps** - AI agents skip foundational checks
2. **Too much upfront scaffolding** - Encourages "big bang" approach that fails
3. **Unclear test-first requirements** - Testing added as afterthought
4. **Ambiguous completion criteria** - "Write tests" is too vague
5. **Missing dependency chains** - Tasks appear independent when they're not
6. **Implicit knowledge assumptions** - Expects AI to "know" patterns from other components

---

## Root Cause Analysis: Why Jules Struggled

### Problem 1: The "Phantom Component" Incident

**What happened:** Jules tried to fix compilation errors for a component that didn't exist in the workspace.

**Root cause:** No explicit "VERIFY BEFORE STARTING" step in checklist.

**Current checklist says:**
```markdown
### Morning: Crate Structure
- [ ] Create directory: `src/components/zzcollector-state/`
- [ ] Create `Cargo.toml` with dependencies:
```

**Problem:** Assumes clean slate, doesn't verify starting state.

**Proposed fix:**
```markdown
### Morning: Pre-flight Verification
- [ ] **VERIFY:** Run `ls -la src/components/` to confirm component does NOT exist yet
- [ ] **VERIFY:** Run `cargo build` to establish clean baseline (should succeed)
- [ ] **VERIFY:** Review PHASE3_CHECKLIST.md in full before writing any code
- [ ] **COMMIT BASELINE:** Note current test count and passing status

### Late Morning: Crate Structure
- [ ] Create directory: `src/components/zzcollector-state/`
- [ ] Create `Cargo.toml` with dependencies: [exact content provided]
- [ ] **VERIFY:** Run `cargo metadata` to confirm new package recognized
- [ ] **VERIFY:** Run `cargo check -p zzcollector-state` (expect errors about missing lib.rs - THAT'S OK)
- [ ] Create empty `src/lib.rs`
- [ ] **VERIFY:** Run `cargo check -p zzcollector-state` (should now pass with warnings about unused)
- [ ] **COMMIT:** "chore(zzcollector-state): Initialize crate structure"
```

**Key improvements:**
- Explicit verification commands with expected outcomes
- Incremental validation (metadata → check → build)
- Commit point after minimal viable structure
- Clear expectations ("expect errors" vs "should pass")

---

### Problem 2: The "Scaffolding Avalanche"

**What happened:** Jules generated all files at once with 30+ compilation errors, including fundamental issues like missing `Unpin` bounds and `Rc` in async contexts.

**Root cause:** Checklist encourages batch creation of files without compilation checkpoints.

**Current checklist says:**
```markdown
- [ ] Create files: `messages.rs`, `network_messages.rs`, `actor.rs`, `builder.rs`, `api.rs`, `role.rs`, `state.rs`
```

**Problem:**
- No compilation check between file creation
- No guidance on what "create" means (stub? full implementation?)
- No incremental test-driven approach

**Proposed fix:**
```markdown
### Afternoon: Message Definitions (Test-First Approach)

**IMPORTANT:** We will create ONE file at a time, with tests BEFORE implementation.

#### Step 1: Create message file structure
- [ ] Create `src/messages.rs` with ONLY module docstring:
  ```rust
  //! Internal actor messages for zzcollector-state component.
  ```
- [ ] Add to `lib.rs`: `pub mod messages;`
- [ ] **VERIFY:** `cargo check -p zzcollector-state` (should pass)
- [ ] **COMMIT:** "chore(zzcollector-state): Add messages module"

#### Step 2: Define first message with test
- [ ] In `messages.rs`, add imports:
  ```rust
  use actix::Message;
  ```
- [ ] Define ONLY `UpdateHealthMetrics` message:
  ```rust
  /// Command to update health metrics from other components.
  #[derive(Message, Debug, Clone)]
  #[rtype(result = "()")]
  pub struct UpdateHealthMetrics {
      pub pings_sent: Option<u64>,
      pub pings_received: Option<u64>,
      pub batches_sent: Option<u64>,
      pub last_config_update_ms: Option<u64>,
  }
  ```
- [ ] **VERIFY:** `cargo check -p zzcollector-state` (should pass)
- [ ] Create `tests/message_tests.rs`:
  ```rust
  use zzcollector_state::messages::*;

  #[test]
  fn test_update_health_metrics_creation() {
      let msg = UpdateHealthMetrics {
          pings_sent: Some(42),
          pings_received: None,
          batches_sent: Some(10),
          last_config_update_ms: None,
      };
      assert_eq!(msg.pings_sent, Some(42));
  }
  ```
- [ ] **VERIFY:** `cargo test -p zzcollector-state` (should pass 1 test)
- [ ] **COMMIT:** "feat(zzcollector-state): Add UpdateHealthMetrics message with test"

#### Step 3: Add remaining messages incrementally
- [ ] Add `GetCollectorState` message (follow same pattern: define → test → commit)
- [ ] Add `ForceHeartbeat` message (define → test → commit)
- [ ] Add `GetHealth` message (define → test → commit)
- [ ] **VERIFY:** `cargo test -p zzcollector-state` (should pass 4+ tests)
- [ ] **COMMIT:** "feat(zzcollector-state): Complete internal message definitions"
```

**Key improvements:**
- One file/feature at a time with verification
- Tests written BEFORE or WITH implementation
- Explicit expected outcomes ("should pass 1 test")
- Commit after each successful increment
- Clear imports and dependencies shown

---

### Problem 3: Missing Core Features

**What happened:** 60% completion - stale detection, HeartbeatAck, QueryCollectors all missing despite being in checklist.

**Root cause:**
- Checklist items too vague ("Implement stale detection")
- No explicit test requirements per feature
- No verification that feature actually works
- Easy to mark as "done" without full implementation

**Current checklist says:**
```markdown
### Afternoon: Stale Collector Detection
- [ ] Implement periodic cleanup task
- [ ] Mark collectors as stale if no heartbeat for X seconds
- [ ] Remove stale collectors from tracking map
- [ ] Write tests for stale detection
```

**Problem:**
- "Implement periodic cleanup task" - WHERE? HOW? WHAT FUNCTION?
- "Write tests" - WHAT tests specifically?
- No verification that cleanup actually runs

**Proposed fix:**
```markdown
### Afternoon: Stale Collector Detection (CRITICAL FEATURE)

**CONTEXT:** Database must remove collectors that stop sending heartbeats. This prevents unbounded memory growth and incorrect "active" reporting.

**REQUIREMENT:** This feature is NOT OPTIONAL. PR will be rejected without working implementation.

#### Step 1: Define cleanup message
- [ ] In `messages.rs`, add:
  ```rust
  /// Internal message to trigger stale collector cleanup.
  /// Database role only.
  #[derive(Message, Debug)]
  #[rtype(result = "()")]
  pub struct CleanupStaleCollectors;
  ```
- [ ] **VERIFY:** `cargo check -p zzcollector-state` (should pass)

#### Step 2: Write test FIRST (TDD approach)
- [ ] In `tests/database_role_tests.rs`, add test:
  ```rust
  #[actix::test]
  async fn test_stale_collector_removal() {
      // This test MUST FAIL initially - that's correct!

      use tokio::time::{pause, advance, Duration};
      pause(); // Mock time for deterministic testing

      let role = CStateRole::Database {
          stale_timeout_secs: 10,
          max_collectors: None,
      };
      let actor = CStateBuilder::new(role)
          .session_manager(create_mock_session_manager())
          .start()
          .unwrap();

      // Send heartbeat to register collector
      let heartbeat = CStateMessage::Heartbeat {
          collector_id: "test-collector".to_string(),
          uptime_secs: 5,
          pings_sent: 100,
          pings_received: 90,
          batches_sent: 10,
          last_config_update_ms: 0,
          connection_nonce: 12345,
      };

      actor.send(WrappedCStateMessage {
          message: heartbeat,
          peer_id: "peer-1".to_string(),
      }).await.unwrap();

      // Verify collector is tracked
      let state = actor.send(GetDatabaseState).await.unwrap();
      assert_eq!(state.collectors.len(), 1);

      // Advance time past stale timeout
      advance(Duration::from_secs(15)).await;

      // Trigger cleanup explicitly
      actor.send(CleanupStaleCollectors).await.unwrap();

      // Verify collector removed
      let state = actor.send(GetDatabaseState).await.unwrap();
      assert_eq!(state.collectors.len(), 0, "Stale collector should be removed");
  }
  ```
- [ ] **VERIFY:** `cargo test -p zzcollector-state test_stale_collector_removal` (SHOULD FAIL - we haven't implemented yet)
- [ ] **COMMIT:** "test(zzcollector-state): Add failing test for stale detection"

#### Step 3: Implement handler to make test pass
- [ ] In `actor.rs`, add handler:
  ```rust
  impl<TMsg, TRole, SM> Handler<CleanupStaleCollectors> for CStateActor<TMsg, TRole, SM>
  where
      TMsg: RoomMessageTrait + From<CStateMessage> + TryInto<CStateMessage> + Unpin,
      TRole: ApplicationRole,
      SM: SessionManagerLike<TMsg, TRole> + Unpin,
  {
      type Result = ();

      fn handle(&mut self, _msg: CleanupStaleCollectors, _ctx: &mut Context<Self>) -> Self::Result {
          if let Some(state) = &mut self.database_state {
              if let CStateRole::Database { stale_timeout_secs, .. } = &self.role {
                  let now = std::time::SystemTime::now()
                      .duration_since(std::time::UNIX_EPOCH)
                      .unwrap_or_default()
                      .as_millis() as u64;

                  let timeout_ms = stale_timeout_secs * 1000;

                  let before_count = state.collectors.len();
                  state.collectors.retain(|id, collector| {
                      let age_ms = now.saturating_sub(collector.last_seen_ms);
                      if age_ms > timeout_ms {
                          tracing::debug!("Removing stale collector: {} (age: {}ms > {}ms)",
                              id, age_ms, timeout_ms);
                          false // Remove
                      } else {
                          true // Keep
                      }
                  });

                  let removed = before_count - state.collectors.len();
                  if removed > 0 {
                      tracing::info!("Cleanup removed {} stale collector(s)", removed);
                  }
              }
          }
      }
  }
  ```
- [ ] **VERIFY:** `cargo check -p zzcollector-state` (should pass)
- [ ] **VERIFY:** `cargo test -p zzcollector-state test_stale_collector_removal` (SHOULD NOW PASS)
- [ ] **COMMIT:** "feat(zzcollector-state): Implement stale collector cleanup handler"

#### Step 4: Schedule periodic cleanup
- [ ] In `actor.rs`, in `started()` method for Database role, add:
  ```rust
  fn started(&mut self, ctx: &mut Self::Context) {
      tracing::info!("CStateActor started in role: {:?}", self.role);

      match &self.role {
          CStateRole::Database { stale_timeout_secs, .. } => {
              // Run cleanup every half of stale timeout
              let check_interval = Duration::from_secs(stale_timeout_secs / 2);

              ctx.run_interval(check_interval, |_act, ctx| {
                  ctx.address().do_send(CleanupStaleCollectors);
              });

              tracing::info!("Scheduled stale collector cleanup every {:?}", check_interval);
          }
          CStateRole::Collector { heartbeat_interval_secs, .. } => {
              // Collector heartbeat scheduling (existing code)
          }
          CStateRole::Admin => {
              // No periodic tasks for admin role
          }
      }
  }
  ```
- [ ] **VERIFY:** `cargo check -p zzcollector-state` (should pass)

#### Step 5: Add integration test for automatic cleanup
- [ ] Add test for automatic periodic cleanup:
  ```rust
  #[actix::test]
  async fn test_automatic_stale_cleanup() {
      use tokio::time::{pause, advance, Duration};
      pause();

      // Setup with short timeout for testing
      let role = CStateRole::Database {
          stale_timeout_secs: 10,
          max_collectors: None,
      };
      // ... similar to previous test but wait for automatic cleanup
      // instead of manually triggering
  }
  ```
- [ ] **VERIFY:** `cargo test -p zzcollector-state` (all stale detection tests pass)
- [ ] **COMMIT:** "feat(zzcollector-state): Add periodic stale collector cleanup"

#### Step 6: Mandatory review checkpoint
- [ ] **VERIFY FEATURE COMPLETE:**
  - [ ] `CleanupStaleCollectors` message defined
  - [ ] Handler implemented and tested
  - [ ] Periodic task scheduled in `started()`
  - [ ] At least 2 tests passing (manual trigger + automatic)
  - [ ] Code uses `.unwrap_or_default()` not `.unwrap()` for time operations
  - [ ] Cleanup is logged at INFO level
- [ ] **IF ANY ITEM ABOVE IS NO:** Feature is incomplete, do not proceed

**END OF CRITICAL SECTION - STALE DETECTION COMPLETE**
```

**Key improvements:**
- Explicit test-first approach (write failing test, then implement)
- Complete code samples with proper error handling
- Multiple verification steps with expected outcomes
- Mandatory review checkpoint before proceeding
- Clear labeling of CRITICAL vs optional features
- Context explanation (WHY this feature matters)

---

## Proposed Checklist Template Improvements

### 1. Add Pre-flight Section (ALL PHASES)

Every phase checklist should start with:

```markdown
## Pre-flight Checklist (COMPLETE BEFORE DAY 1)

**STOP:** Do not write any code until you complete these verification steps.

### Environment Verification
- [ ] **VERIFY WORKSPACE:** Run `pwd` and confirm you're in `/path/to/zzping`
- [ ] **VERIFY GIT STATUS:** Run `git status` to see current branch and changes
- [ ] **VERIFY BASELINE:** Run `cargo build` and confirm it succeeds
- [ ] **VERIFY TESTS:** Run `cargo test` and note number of passing tests
- [ ] **RECORD BASELINE:** Write down current test count: _____ tests passing

### Document Review (MANDATORY READING)
- [ ] **READ CHECKLIST:** Read this entire PHASEN_CHECKLIST.md file (don't skim!)
- [ ] **READ CODING STANDARDS:** Review AGENT_CODING_STANDARDS.md
- [ ] **READ COMPONENT TEMPLATE:** Review COMPONENT_TEMPLATE_GUIDE.md
- [ ] **READ EXAMPLE COMPONENT:** Examine `src/components/zzintent-config/` structure
- [ ] **READ PREVIOUS PHASE:** If Phase 4+, review previous phase checklist

### Understanding Check (ANSWER THESE QUESTIONS)
- [ ] What is the PRIMARY PURPOSE of this component in 1-2 sentences?
- [ ] Which 3 roles does this component support?
- [ ] What are the 2-3 CRITICAL features that CANNOT be skipped?
- [ ] Which existing components does this integrate with?

**IF YOU CANNOT ANSWER THESE QUESTIONS:** Stop and read the documentation again.

### Execution Strategy Commitment
- [ ] **I COMMIT TO:** Creating files incrementally with verification after each step
- [ ] **I COMMIT TO:** Writing tests BEFORE or WITH implementation (TDD)
- [ ] **I COMMIT TO:** Running `cargo check` after every significant change
- [ ] **I COMMIT TO:** Committing after each completed increment
- [ ] **I COMMIT TO:** Not moving to next day until current day passes all tests

**SIGNATURE (Confirm you've read and understood):** ____________

---
```

### 2. Enhance Task Structure

Every task should follow this template:

```markdown
#### Step N: [Task Name] - [Estimated Time: X min]

**CONTEXT:** [Why this step matters, how it fits in the bigger picture]

**REQUIREMENT:** [Is this CRITICAL or OPTIONAL? What happens if skipped?]

**DEPENDENCIES:** [What must be complete before starting this?]

**ACTIONS:**
- [ ] [Specific action with exact file/function names]
  ```rust
  // Exact code to write, not pseudocode
  ```
- [ ] **VERIFY:** [Exact command to run] (Expected outcome: [specific result])
- [ ] **COMMIT:** "[exact commit message in conventional format]"

**DONE CRITERIA:**
- [ ] [Specific measurable outcome 1]
- [ ] [Specific measurable outcome 2]
- [ ] Tests pass: `[exact test command]`

**IF STUCK:** [Common errors and how to fix them]
```

### 3. Add Mandatory Checkpoints

After each major feature:

```markdown
---
## 🛑 CHECKPOINT: [Feature Name] Complete

**BEFORE PROCEEDING, VERIFY:**
- [ ] Feature X fully implemented (not stubbed)
- [ ] At least N tests passing for this feature
- [ ] `cargo clippy` produces no warnings in new code
- [ ] All public items have doc comments
- [ ] No `.unwrap()` calls on fallible operations
- [ ] Committed with message: "[expected commit message]"

**RUN THIS COMMAND:** `cargo test -p [package] [test_pattern]`
**EXPECTED:** X tests pass, 0 failures

**IF ANY ITEM ABOVE FAILS:**
1. Do not proceed to next section
2. Review the feature implementation
3. Check PR25_REVIEW.md for similar issues
4. Ask for help if stuck >30 minutes

**SIGN OFF:** [ ] I verify all items above are complete
---
```

### 4. Add Test-First Templates

For every feature, provide the test template first:

```markdown
### Step 1: Write Failing Test (TEST-FIRST)

**PHILOSOPHY:** We write the test FIRST to clarify requirements, then implement to make it pass.

- [ ] Create test file if it doesn't exist: `tests/[feature]_tests.rs`
- [ ] Add test (THIS MUST FAIL initially - that's correct!):
  ```rust
  #[actix::test]
  async fn test_[specific_feature]() {
      // Arrange
      let actor = setup_test_actor();

      // Act
      let result = actor.send(SomeMessage).await.unwrap();

      // Assert
      assert_eq!(result.expected_field, expected_value, "Explanation of why");
  }
  ```
- [ ] **VERIFY TEST FAILS:** `cargo test -p [package] test_[specific_feature]`
  - Expected: Test should FAIL or have compilation errors
  - **IF TEST PASSES:** Something is wrong, investigate
- [ ] **COMMIT:** "test([component]): Add failing test for [feature]"

### Step 2: Implement Feature to Make Test Pass

- [ ] [Implementation steps...]
- [ ] **VERIFY TEST PASSES:** `cargo test -p [package] test_[specific_feature]`
  - Expected: Test should now PASS
  - **IF TEST STILL FAILS:** Debug before proceeding
- [ ] **COMMIT:** "feat([component]): Implement [feature]"
```

### 5. Add Common Pitfalls Section PER FEATURE

Instead of general pitfalls at the end, add them inline:

```markdown
### Common Mistakes for This Feature

**❌ MISTAKE 1:** Using `Rc<SessionManager>` in async context
- **ERROR YOU'LL SEE:** "Rc cannot be sent between threads safely"
- **FIX:** Use `Arc<SessionManager>` instead, even in single-threaded tests
- **WHY:** `tokio::spawn` requires `Send`, and `Rc` is not `Send`

**❌ MISTAKE 2:** Forgetting `Unpin` bound on generic types
- **ERROR YOU'LL SEE:** "cannot be unpinned"
- **FIX:** Add `Unpin` to all trait bounds: `T: ApplicationRole + Unpin`
- **WHY:** Actix requires `Unpin` for actor types

**❌ MISTAKE 3:** Using `.unwrap()` on `SystemTime::now()`
- **ERROR YOU'LL SEE:** Potential panic if system clock before 1970
- **FIX:** Always use `.unwrap_or_default()`
- **WHY:** System clocks can be misconfigured or adjusted by NTP

**CHECK YOURSELF:** After implementing this feature:
- [ ] No `Rc` types in actor or spawned tasks (use `Arc`)
- [ ] All generic trait bounds include `Unpin`
- [ ] No bare `.unwrap()` on time operations
```

---

## Specific Improvements for Phase 4, 5, 6

### Phase 4 (Collector Application) - Specific Additions

```markdown
## Day 1: Application Structure and Configuration

### Pre-flight: Verify All Components Exist
- [ ] **CRITICAL CHECK:** Verify these components compile individually:
  ```bash
  cargo check -p zzintent-config  # Should pass
  cargo check -p zzpinger          # Should pass
  cargo check -p zzmem-db          # Should pass
  cargo check -p zzcollector-state # Should pass
  ```
- [ ] **IF ANY FAIL:** Stop. Fix component issues before building application.

### Morning: Create Application Crate (Binary)

**CONTEXT:** We're creating a BINARY crate (not library) that will integrate components.

**CRITICAL DIFFERENCE:**
- Library crate: `src/lib.rs` + optional `src/bin/`
- Binary crate: `src/main.rs` + `src/lib.rs` (for testable logic)

#### Step 1: Create directory structure
- [ ] Create directory: `src/apps/zzping-collector/`
- [ ] **VERIFY:** `ls -la src/apps/` shows zzping-collector directory
- [ ] Create `Cargo.toml`:
  ```toml
  [package]
  name = "zzping-collector"
  version = "0.1.0"
  edition = "2021"

  [[bin]]
  name = "zzping-collector"
  path = "src/main.rs"

  [dependencies]
  # Component dependencies
  zzintent-config = { path = "../../components/zzintent-config" }
  zzpinger = { path = "../../components/zzpinger" }
  zzmem-db = { path = "../../components/zzmem-db" }
  zzcollector-state = { path = "../../components/zzcollector-state" }

  # Network layer
  zznet-session = { path = "../../net/zznet-session" }
  zznet-builder = { path = "../../net/zznet-builder" }
  zznet-transport-tcp = { path = "../../net/zznet-transport-tcp" }
  zznet-auth = { path = "../../net/zznet-auth" }

  # Runtime
  tokio = { version = "1.0", features = ["full"] }
  actix = "0.13"

  # Config/Serialization
  serde = { version = "1.0", features = ["derive"] }
  ron = "0.8"

  # CLI
  clap = { version = "4.0", features = ["derive"] }

  # Logging
  tracing = "0.1"
  tracing-subscriber = { version = "0.3", features = ["env-filter"] }

  # Error handling
  anyhow = "1.0"
  thiserror = "1.0"

  # TLS
  rustls = "0.21"
  rustls-pemfile = "1.0"

  [dev-dependencies]
  tempfile = "3.0"
  ```
- [ ] **VERIFY:** `cargo metadata` includes zzping-collector
- [ ] **COMMIT:** "chore(collector): Initialize binary crate structure"

#### Step 2: Create minimal main.rs (JUST SKELETON)
- [ ] Create `src/main.rs`:
  ```rust
  //! ZZPing Collector Application
  //!
  //! Integrates all collector-side components and connects to database.

  use anyhow::Result;

  fn main() -> Result<()> {
      println!("zzping-collector starting...");
      Ok(())
  }
  ```
- [ ] **VERIFY:** `cargo build --bin zzping-collector` (should succeed)
- [ ] **VERIFY:** `./target/debug/zzping-collector` (should print message)
- [ ] **COMMIT:** "feat(collector): Add minimal main.rs skeleton"

#### Step 3: Create lib.rs for testable logic
- [ ] Create `src/lib.rs`:
  ```rust
  //! Collector application library
  //!
  //! Contains testable business logic separated from main().

  pub mod config;
  pub mod service;
  pub mod error;

  pub use config::CollectorConfig;
  pub use service::CollectorService;
  pub use error::CollectorError;
  ```
- [ ] Create empty module files: `src/config.rs`, `src/service.rs`, `src/error.rs`
- [ ] Add minimal content to each to make it compile:
  ```rust
  // src/error.rs
  use thiserror::Error;

  #[derive(Error, Debug)]
  pub enum CollectorError {
      #[error("Configuration error: {0}")]
      Config(String),
  }

  // src/config.rs
  use serde::{Deserialize, Serialize};

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct CollectorConfig {
      pub collector_id: String,
  }

  // src/service.rs
  pub struct CollectorService {
      // Will implement later
  }
  ```
- [ ] **VERIFY:** `cargo check --bin zzping-collector` (should pass with warnings about unused)
- [ ] **VERIFY:** `cargo test -p zzping-collector` (should pass 0 tests)
- [ ] **COMMIT:** "chore(collector): Add module structure"

### 🛑 CHECKPOINT: Crate Structure Complete

**VERIFY BEFORE PROCEEDING:**
- [ ] `cargo build --bin zzping-collector` succeeds
- [ ] Binary runs: `./target/debug/zzping-collector` works
- [ ] Module structure created and compiles
- [ ] No compilation errors (warnings OK at this stage)
- [ ] All changes committed

**IF ANY FAILS:** Fix before moving to configuration implementation.

---
```

### Phase 5 (Database Application) - Add Server-Specific Guidance

```markdown
## Day 3: TLS Server and Connection Handling

### CRITICAL ARCHITECTURE DECISION

**CONTEXT:** The database is a SERVER that ACCEPTS incoming connections. This is fundamentally different from the collector (which is a CLIENT that CONNECTS).

**KEY DIFFERENCES:**

| Aspect | Collector (Client) | Database (Server) |
|--------|-------------------|-------------------|
| TLS Role | Connects with client cert | Accepts with server cert |
| Connection Count | 1 connection | N connections (multiple collectors) |
| SessionManager Mode | Client mode | Server mode |
| Cert Files | collector.pem, collector.key, ca.pem | database.pem, database.key, ca.pem |

**IF YOU'RE CONFUSED ABOUT TLS:**
- Stop and review `examples/tls-example/`
- The collector uses `TcpStreamTransport::connect()`
- The database uses `TcpStreamTransport::accept()` in a loop

### Morning: TLS Server Setup

**REQUIREMENT:** This is CRITICAL infrastructure. Cannot proceed without working TLS.

#### Step 1: Understand TLS acceptor pattern
- [ ] **READ:** `examples/tls-example/src/server.rs` (lines 1-50)
- [ ] **UNDERSTAND:** The pattern is:
  1. Load server certificate and private key
  2. Load CA certificate (to verify client certs)
  3. Create `rustls::ServerConfig` with client auth required
  4. Create `TcpListener` on bind address
  5. Loop: accept connection, do TLS handshake, spawn handler

#### Step 2: Write test for TLS setup (TEST-FIRST)
- [ ] Create `tests/tls_tests.rs`:
  ```rust
  #[tokio::test]
  async fn test_load_tls_config() {
      // This test ensures certificate loading works
      use zzping_database::tls::load_server_tls_config;

      let config = load_server_tls_config(
          "test_certs/database.pem",
          "test_certs/database.key",
          "test_certs/ca.pem",
      ).expect("Failed to load TLS config");

      // If we get here, certificates loaded successfully
      assert!(true);
  }
  ```
- [ ] **VERIFY TEST FAILS:** `cargo test -p zzping-database test_load_tls_config`
  - Expected: Compilation error (function doesn't exist yet)
- [ ] **COMMIT:** "test(database): Add failing test for TLS config loading"

#### Step 3: Implement TLS loading function
- [ ] Create `src/tls.rs`:
  ```rust
  //! TLS configuration for database server.

  use anyhow::{Context, Result};
  use rustls::{ServerConfig, Certificate, PrivateKey};
  use rustls::server::AllowAnyAuthenticatedClient;
  use rustls_pemfile::{certs, pkcs8_private_keys};
  use std::fs::File;
  use std::io::BufReader;
  use std::sync::Arc;

  /// Load server TLS configuration with client certificate verification.
  ///
  /// # Arguments
  /// * `cert_path` - Path to server certificate PEM file
  /// * `key_path` - Path to server private key PEM file
  /// * `ca_path` - Path to CA certificate for verifying clients
  ///
  /// # Errors
  /// Returns error if certificate files cannot be loaded or are invalid.
  pub fn load_server_tls_config(
      cert_path: &str,
      key_path: &str,
      ca_path: &str,
  ) -> Result<Arc<ServerConfig>> {
      // Load server certificate
      let cert_file = File::open(cert_path)
          .with_context(|| format!("Failed to open cert file: {}", cert_path))?;
      let mut cert_reader = BufReader::new(cert_file);
      let cert_chain: Vec<Certificate> = certs(&mut cert_reader)?
          .into_iter()
          .map(Certificate)
          .collect();

      // Load server private key
      let key_file = File::open(key_path)
          .with_context(|| format!("Failed to open key file: {}", key_path))?;
      let mut key_reader = BufReader::new(key_file);
      let mut keys: Vec<PrivateKey> = pkcs8_private_keys(&mut key_reader)?
          .into_iter()
          .map(PrivateKey)
          .collect();

      if keys.is_empty() {
          anyhow::bail!("No private key found in {}", key_path);
      }
      let private_key = keys.remove(0);

      // Load CA certificate for client verification
      let ca_file = File::open(ca_path)
          .with_context(|| format!("Failed to open CA file: {}", ca_path))?;
      let mut ca_reader = BufReader::new(ca_file);
      let ca_certs: Vec<Certificate> = certs(&mut ca_reader)?
          .into_iter()
          .map(Certificate)
          .collect();

      // Create root cert store
      let mut root_cert_store = rustls::RootCertStore::empty();
      for cert in ca_certs {
          root_cert_store.add(&cert)
              .context("Failed to add CA certificate to store")?;
      }

      // Create client verifier (REQUIRE client certificates)
      let client_verifier = AllowAnyAuthenticatedClient::new(root_cert_store);

      // Build server config
      let config = ServerConfig::builder()
          .with_safe_defaults()
          .with_client_cert_verifier(Arc::new(client_verifier))
          .with_single_cert(cert_chain, private_key)
          .context("Failed to build server config")?;

      Ok(Arc::new(config))
  }
  ```
- [ ] Add to `lib.rs`: `pub mod tls;`
- [ ] **VERIFY:** `cargo check -p zzping-database` (should pass)
- [ ] **VERIFY TEST PASSES:** `cargo test -p zzping-database test_load_tls_config`
  - Expected: Test should now PASS
  - **IF FAILS:** Check certificate paths in test
- [ ] **COMMIT:** "feat(database): Implement TLS server config loading"

#### Step 4: Common TLS errors and fixes

**❌ MISTAKE 1:** Using client TLS config instead of server config
- **ERROR:** "handshake failed: received fatal alert: UnexpectedMessage"
- **FIX:** Ensure using `ServerConfig`, not `ClientConfig`

**❌ MISTAKE 2:** Not requiring client certificates
- **ERROR:** Clients connect without authentication
- **FIX:** Use `AllowAnyAuthenticatedClient`, not `NoClientAuth`

**❌ MISTAKE 3:** Wrong certificate order
- **ERROR:** "handshake failed: bad certificate"
- **FIX:** Certificate chain must be [server_cert, ...intermediates]

### 🛑 CHECKPOINT: TLS Configuration Works

**VERIFY:**
- [ ] `test_load_tls_config` passes
- [ ] Function returns `Arc<ServerConfig>` (not unwrapped)
- [ ] All file operations use `.context()` for good error messages
- [ ] No `.unwrap()` calls in production code

**RUN:** `cargo test -p zzping-database tls`
**EXPECTED:** 1 test passes

---
```

### Phase 6 (Integration) - Add Debugging Guide

```markdown
## Day 3: End-to-End Integration Testing

### CRITICAL DEBUGGING GUIDE

**CONTEXT:** This is where everything comes together. When it fails, you need systematic debugging.

### Common Integration Failures and How to Fix

#### Failure Pattern 1: "Connection Refused"

**Symptoms:**
```
Error: Connection refused (os error 111)
```

**Systematic Debug Process:**
1. [ ] **VERIFY DATABASE RUNNING:**
   ```bash
   ps aux | grep zzping-database  # Should see process
   netstat -tlnp | grep 8443      # Should see LISTEN on port
   ```
   **IF NOT RUNNING:** Start database first!

2. [ ] **VERIFY BIND ADDRESS:**
   ```bash
   # In database config, check:
   bind_host: "0.0.0.0"  # ✅ Correct - accepts from anywhere
   bind_host: "127.0.0.1"  # ✅ OK for local testing
   bind_host: "localhost"  # ❌ WRONG - use IP address
   ```

3. [ ] **VERIFY COLLECTOR CONFIG:**
   ```bash
   # In collector config, check:
   database_host: "127.0.0.1"  # ✅ For local testing
   database_host: "localhost"  # ⚠️  Might resolve to IPv6
   database_port: 8443  # ✅ Must match database bind_port
   ```

4. [ ] **TEST RAW CONNECTION:**
   ```bash
   telnet 127.0.0.1 8443  # Should connect
   # If "Connection refused": Database not listening
   # If "Connected": TLS or application layer issue
   ```

#### Failure Pattern 2: "TLS Handshake Failed"

**Symptoms:**
```
Error: received fatal alert: BadCertificate
Error: received fatal alert: UnknownCA
Error: handshake failed
```

**Systematic Debug Process:**
1. [ ] **VERIFY CERTIFICATES EXIST:**
   ```bash
   ls -la test_certs/
   # Should see: ca.pem, database.pem, database.key, collector.pem, collector.key
   ```

2. [ ] **VERIFY CERTIFICATE VALIDITY:**
   ```bash
   openssl x509 -in test_certs/database.pem -noout -dates
   # Check notBefore and notAfter

   openssl x509 -in test_certs/collector.pem -noout -dates
   # Check not expired
   ```

3. [ ] **VERIFY CERTIFICATE CHAIN:**
   ```bash
   # Database cert should be signed by CA
   openssl verify -CAfile test_certs/ca.pem test_certs/database.pem
   # Expected: test_certs/database.pem: OK

   # Collector cert should be signed by CA
   openssl verify -CAfile test_certs/ca.pem test_certs/collector.pem
   # Expected: test_certs/collector.pem: OK
   ```

4. [ ] **COMMON TLS FIXES:**
   ```bash
   # If expired, regenerate:
   ./generate_certs.sh

   # If wrong CA, ensure both use same ca.pem:
   # Database config:
   ca_cert: "test_certs/ca.pem"
   server_cert: "test_certs/database.pem"
   server_key: "test_certs/database.key"

   # Collector config:
   ca_cert: "test_certs/ca.pem"
   client_cert: "test_certs/collector.pem"
   client_key: "test_certs/collector.key"
   ```

#### Failure Pattern 3: "No Rooms in Common"

**Symptoms:**
```
Connection established but no data flow
Logs show: "No rooms in common with peer"
```

**Systematic Debug Process:**
1. [ ] **VERIFY ROOM REGISTRATION:**
   ```rust
   // In database service.rs, check:
   session_manager.register_room_handler(
       RoomId::from("intent-config"),  // ✅ Exact string
       intent_config_handler
   );

   // In collector service.rs, check:
   session_manager.register_room_handler(
       RoomId::from("intent-config"),  // ✅ Must match exactly
       intent_config_handler
   );
   ```

2. [ ] **COMMON ROOM NAME MISTAKES:**
   ```rust
   RoomId::from("intentconfig")   // ❌ No hyphen
   RoomId::from("intent-config")  // ✅ Correct
   RoomId::from("intent_config")  // ❌ Underscore instead of hyphen
   RoomId::from("IntentConfig")   // ❌ Wrong case
   ```

3. [ ] **VERIFY IN LOGS:**
   ```bash
   # Database logs should show:
   "Registered room handler: intent-config"
   "Registered room handler: mem-db"
   "Registered room handler: c-state"

   # Collector logs should show:
   "Registered room handler: intent-config"
   "Registered room handler: mem-db"
   "Registered room handler: c-state"

   # Connection logs should show:
   "Room intersection: [intent-config, mem-db, c-state]"
   ```

4. [ ] **TEST ROOM INTERSECTION:**
   ```bash
   # Add debug logging in connection handler:
   tracing::info!("Database rooms: {:?}", session_manager.rooms());
   tracing::info!("Collector rooms: {:?}", hello_message.supported_rooms);
   tracing::info!("Intersection: {:?}", intersection);
   ```

#### Failure Pattern 4: "Component Not Responding"

**Symptoms:**
```
Collector connected but no pings
Config sent but collector doesn't update
Data sent but database doesn't store
```

**Systematic Debug Process:**
1. [ ] **VERIFY COMPONENTS STARTED:**
   ```rust
   // In service.rs bootstrap, check order:
   // 1. Create builders ✅
   // 2. Wire with SessionManager ✅
   // 3. START components (don't forget!) ✅

   let intent_addr = intent_builder.start()?;  // Must call start()!
   let pinger_addr = pinger_builder.start()?;
   // etc.
   ```

2. [ ] **VERIFY MESSAGE HANDLERS REGISTERED:**
   ```rust
   // Each component must register BEFORE connection:
   session_manager.register_room_handler(room_id, handler);
   // THEN
   session_manager.connect() or .accept()
   ```

3. [ ] **ADD INSTRUMENTATION:**
   ```rust
   // In each component's Handler impl:
   fn handle(&mut self, msg: Message, ctx: &mut Context<Self>) {
       tracing::debug!("Component received message: {:?}", msg);
       // ... rest of handling
   }
   ```

4. [ ] **CHECK ACTIX MAILBOX:**
   ```bash
   # If component running but not responding:
   # - Mailbox might be full (default 16 messages)
   # - Handler might be panicking silently
   # - Message type might not match handler signature

   # Solution: Add explicit error handling:
   match result {
       Ok(val) => tracing::info!("Success: {:?}", val),
       Err(e) => tracing::error!("Handler failed: {:?}", e),
   }
   ```

### 🛑 CHECKPOINT: Can Debug Integration Failures

**VERIFY YOU UNDERSTAND:**
- [ ] How to check if database is listening (netstat)
- [ ] How to verify certificates (openssl verify)
- [ ] How to check room registration (logs)
- [ ] How to add instrumentation (tracing)

**IF NOT CLEAR:** Review the debug patterns above before proceeding.

---
```

---

## Summary of Proposed Changes

### For Phase 4 Checklist:
1. ✅ Add pre-flight verification section
2. ✅ Add explicit binary vs library crate guidance
3. ✅ Add component dependency verification before starting
4. ✅ Break down each day into smaller steps with verification
5. ✅ Add test templates for integration testing
6. ✅ Add checkpoint after crate structure before logic implementation

### For Phase 5 Checklist:
1. ✅ Add server vs client architecture clarification
2. ✅ Add TLS server-specific guidance with common errors
3. ✅ Add test-first approach for TLS configuration
4. ✅ Add multi-connection handling guidance
5. ✅ Add checkpoint for TLS working before moving to logic
6. ✅ Include comparison table (server vs client differences)

### For Phase 6 Checklist:
1. ✅ Add comprehensive debugging guide for integration
2. ✅ Add failure pattern recognition (Connection Refused, TLS, etc.)
3. ✅ Add systematic debug process for each failure type
4. ✅ Add instrumentation guidance
5. ✅ Add verification commands with expected outputs
6. ✅ Add "IF STUCK" guidance at each step

### Cross-Cutting Improvements:
1. ✅ Every task has VERIFY step with exact command and expected outcome
2. ✅ Every feature has test-first template
3. ✅ Every feature has common mistakes section inline
4. ✅ Mandatory checkpoints prevent proceeding with incomplete work
5. ✅ All code samples are complete (not pseudocode)
6. ✅ Explicit commit messages provided
7. ✅ Clear labeling of CRITICAL vs OPTIONAL features

---

## How to Use This Document

### For Phase 4:
1. Read this entire document first
2. Apply "Pre-flight Section" to start of PHASE4_CHECKLIST.md
3. Enhance each Day's tasks using "Task Structure Template"
4. Add "Mandatory Checkpoints" after major sections
5. Include "Test-First Templates" for integration tests
6. Review with Jules BEFORE starting Phase 4

### For Phase 5 & 6:
1. Apply same improvements as Phase 4
2. Add phase-specific sections (TLS for P5, Debugging for P6)
3. Ensure each phase builds on previous with explicit dependency checks

### For Jules:
1. You MUST complete pre-flight checklist before Day 1
2. You MUST verify after each step (not batch at end)
3. You MUST write tests before or with implementation
4. You MUST not proceed past checkpoints if verification fails
5. You MUST commit after each successful increment
6. When stuck >30 min, reference this document's troubleshooting sections

---

## Metrics for Success

After Phase 4 implementation with improved checklist:
- [ ] Fewer than 5 compilation errors at any single point
- [ ] No "Phantom Component" incidents (verification catches early)
- [ ] No features >50% complete (checkpoints prevent)
- [ ] Test coverage >85% (test-first ensures)
- [ ] Jules asks <5 clarifying questions (checklist is self-contained)
- [ ] Time to completion within estimated 1 week
- [ ] PR review identifies <3 major issues (not 9 like Phase 3)

**If these metrics are not met:** Iterate on checklist improvements further.
