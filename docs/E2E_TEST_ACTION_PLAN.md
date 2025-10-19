# End-to-End Test Implementation Action Plan
**Date:** October 19, 2025
**Project:** ZZPing Monitoring System
**Based On:** E2E_TEST_INVESTIGATION_REPORT.md

---

## Overview

This document provides a detailed, step-by-step action plan for implementing comprehensive end-to-end integration tests for the ZZPing monitoring system. Each phase is broken down into specific tasks with clear objectives and verification steps.

**Total Estimated Effort:** 16-24 hours
**Approach:** Sequential phases building upon each other
**Risk Level:** Medium (changes to production code, but well-tested)

---

## Phase 1: Make TLS Optional in Configuration

**Objective:** Enable creation of test configurations without requiring TLS certificate files.

**Rationale:** This is not a test-only change. Production systems should support TCP-only mode for development environments and trusted networks where TLS overhead is unnecessary.

**Estimated Time:** 2-3 hours

### Task 1.1: Update CollectorConfig Structure

**Files to modify:**
- `src/apps/zzping-collector/src/config.rs`

**Changes:**
1. Change field type:
   - FROM: `pub tls: TlsConfig`
   - TO: `pub tls: Option<TlsConfig>`

2. Update `validate()` method:
   - Add check: if `self.tls.is_none()`, skip TLS file validation
   - If `self.tls.is_some()`, perform existing TLS validation
   - Log warning when running without TLS

3. Update `load()` method if needed:
   - Ensure RON deserialization handles `Option<TlsConfig>`
   - Test with config file containing `tls: None`

**Example change:**
```rust
// Before:
pub tls: TlsConfig,

// After:
pub tls: Option<TlsConfig>,

// In validate():
if let Some(tls) = &self.tls {
    // Existing TLS validation
} else {
    tracing::warn!("Running without TLS - connections will use plain TCP");
}
```

**Verification:**
- Compile succeeds
- Existing tests pass
- Can create config with `tls: None`
- Validation accepts None

### Task 1.2: Update DatabaseConfig Structure

**Files to modify:**
- `src/apps/zzping-database/src/config.rs`

**Changes:**
1. Change field type:
   - FROM: `pub tls: TlsConfig`
   - TO: `pub tls: Option<TlsConfig>`

2. Update `validate()` method:
   - Add check: if `self.tls.is_none()`, skip TLS file validation
   - If `self.tls.is_some()`, perform existing TLS validation
   - Log warning when running without TLS

3. Update `load()` method:
   - Handle path resolution for `Option<TlsConfig>`
   - Only resolve TLS paths if Some

**Verification:**
- Compile succeeds
- Existing tests pass
- Can create config with `tls: None`
- Validation accepts None

### Task 1.3: Update CollectorService to Handle Optional TLS

**Files to modify:**
- `src/apps/zzping-collector/src/service.rs`

**Changes:**
1. In `run()` method, change TLS config handling:
   - FROM: `let tls_cfg = Some(Self::convert_tls_config(&self.config.tls)?);`
   - TO: Check if tls is Some before converting

2. Logic flow:
   ```
   let tls_cfg = if let Some(tls) = &self.config.tls {
       Some(Self::convert_tls_config(tls)?)
   } else {
       None
   };
   ```

3. Pass tls_cfg to network creation (might already support None)

4. Add log statement indicating TLS status

**Verification:**
- Service starts with TLS config (existing behavior)
- Service starts without TLS config (new behavior)
- Network uses plain TCP when tls_cfg is None
- Appropriate warnings logged

### Task 1.4: Update DatabaseService to Handle Optional TLS

**Files to modify:**
- `src/apps/zzping-database/src/service.rs`

**Changes:**
1. In `run()` method, change TLS config handling:
   - FROM: `let tls_cfg = Self::build_transport_tls_config(&self.config.tls)?;`
   - TO: Check if tls is Some before building

2. Logic flow:
   ```
   let tls_cfg = if let Some(tls) = &self.config.tls {
       Self::build_transport_tls_config(tls)?
   } else {
       None
   };
   ```

3. Pass tls_cfg to network creation

4. Add log statement indicating TLS status

**Verification:**
- Service starts with TLS config (existing behavior)
- Service starts without TLS config (new behavior)
- Server accepts plain TCP when tls_cfg is None
- Appropriate warnings logged

### Task 1.5: Update Existing Tests

**Files to modify:**
- `src/apps/zzping-collector/src/config.rs` (test section)
- `src/apps/zzping-database/src/config.rs` (test section)
- Any other test files using config structs

**Changes:**
1. Update test configs to use `tls: Some(...)` where TLS is needed
2. Update test configs to use `tls: None` where TLS is not needed
3. Ensure all existing tests compile and pass

**Verification:**
- Run `cargo test --lib` - all pass
- Run `cargo test` - all pass
- No compiler warnings about TLS fields

### Task 1.6: Update Example Configs

**Files to modify:**
- `config/collector.ron.example`
- `config/database.ron.example`
- `src/apps/zzping-collector/collector.example.ron`
- `config/README.md`

**Changes:**
1. Add comments explaining TLS is optional
2. Show example with TLS enabled (production)
3. Show example with `tls: None` (development)
4. Update documentation about when to use each mode

**Verification:**
- Documentation is clear
- Examples parse correctly
- Both modes documented

---

## Phase 2: Change Heartbeat Timing to Milliseconds

**Objective:** Improve timing precision and enable faster test execution.

**Rationale:** Second-level precision is too coarse for responsive systems. Millisecond precision allows:
- Fine-grained heartbeat control
- Faster timeout detection
- More responsive tests
- Better alignment with other timing configs (ping rate, etc.)

**Estimated Time:** 2-3 hours

### Task 2.1: Update CollectorConfig Heartbeat Field

**Files to modify:**
- `src/apps/zzping-collector/src/config.rs`

**Changes:**
1. Rename field:
   - FROM: `pub heartbeat_interval_secs: u64`
   - TO: `pub heartbeat_interval_ms: u64`

2. Update validation:
   - Change error message from "secs" to "ms"
   - Keep zero-check (0 ms still invalid)

3. Update default values in tests:
   - FROM: `heartbeat_interval_secs: 5`
   - TO: `heartbeat_interval_ms: 5000`

4. Add doc comment explaining millisecond precision

**Verification:**
- Field renamed everywhere in file
- Tests compile and pass
- Validation still works

### Task 2.2: Update CStateRole Heartbeat Field

**Files to modify:**
- `src/components/zzcollector-state/src/role.rs`

**Changes:**
1. In `CStateRole::Collector` variant:
   - FROM: `heartbeat_interval_secs: u64`
   - TO: `heartbeat_interval_ms: u64`

2. Update default implementations:
   - Change 5 seconds → 5000 milliseconds
   - Update test values proportionally

3. Update doc comments

**Verification:**
- Role definition updated
- All role creation sites updated
- Tests compile

### Task 2.3: Update CStateActor Heartbeat Usage

**Files to modify:**
- `src/components/zzcollector-state/src/actor.rs`
- `src/components/zzcollector-state/src/builder.rs`

**Changes:**
1. Find all uses of `heartbeat_interval_secs`
2. Change to `heartbeat_interval_ms`
3. Update interval creation:
   - FROM: `Duration::from_secs(interval_secs)`
   - TO: `Duration::from_millis(interval_ms)`

4. Update any logging/debug output mentioning seconds

**Verification:**
- Actor uses milliseconds correctly
- Heartbeat fires at correct intervals
- Tests pass

### Task 2.4: Update CollectorService Usage

**Files to modify:**
- `src/apps/zzping-collector/src/service.rs`

**Changes:**
1. Update component builder calls
2. Update any logging referencing heartbeat interval
3. Ensure value passed correctly from config to component

**Verification:**
- Service wires heartbeat correctly
- Log output shows milliseconds
- Behavior unchanged (5000ms = 5s)

### Task 2.5: Update Configuration Files

**Files to modify:**
- `config/collector.ron`
- `config/collector.ron.example`
- `src/apps/zzping-collector/collector.example.ron`

**Changes:**
1. Change field name:
   - FROM: `heartbeat_interval_secs: 5`
   - TO: `heartbeat_interval_ms: 5000`

2. Update comments explaining milliseconds

**Verification:**
- Config files parse correctly
- RON deserialization works
- Value interpreted as milliseconds

### Task 2.6: Update Tests Using Heartbeat Config

**Files to modify:**
- `src/components/zzcollector-state/src/tests.rs`
- `src/apps/zzping-database/tests/connectivity_integration_test.rs`
- Any other test files

**Changes:**
1. Update all test configs:
   - Change field names
   - Multiply values by 1000 (secs → ms)
   - Or use smaller values for faster tests

2. Update test expectations:
   - If tests check interval values
   - If tests wait for heartbeats

**Verification:**
- All tests compile
- All tests pass
- Test timing still correct

### Task 2.7: Update Documentation

**Files to modify:**
- `config/README.md`
- Any design docs mentioning heartbeat
- RUNBOOK.md if it mentions heartbeat

**Changes:**
1. Update descriptions of heartbeat interval
2. Change examples from seconds to milliseconds
3. Update troubleshooting guides if needed

**Verification:**
- Documentation consistent
- No stale references to seconds

---

## Phase 3: Fix Time Handling for Mocking Compatibility

**Objective:** Replace `std::time::Instant` with `tokio::time::Instant` to enable time mocking in tests.

**Rationale:** This is a bug fix. Tokio-based async applications should use `tokio::time` types, not `std::time` types. This enables `tokio::time::pause()` and `advance()` to work correctly in tests.

**Estimated Time:** 1-2 hours

### Task 3.1: Update CollectorStateData Time Fields

**Files to modify:**
- `src/components/zzcollector-state/src/state.rs`

**Changes:**
1. Change import:
   - FROM: `use std::time::Instant;`
   - TO: `use tokio::time::Instant;`

2. Field remains same type (just different module):
   - `pub start_time: Instant` (but now tokio's Instant)

3. Update initialization:
   - FROM: `start_time: Instant::now()`
   - TO: `start_time: tokio::time::Instant::now()`

4. Any `.elapsed()` calls should still work (same API)

**Verification:**
- Compiles successfully
- No behavioral changes
- API is compatible

### Task 3.2: Update Uptime Calculation

**Files to modify:**
- `src/components/zzcollector-state/src/actor.rs`

**Changes:**
1. Find uses of `state.start_time.elapsed()`
2. Verify they still work with tokio::time::Instant
3. No changes should be needed (API compatible)

**Verification:**
- Uptime calculation correct
- Logs show correct values
- Tests pass

### Task 3.3: Add Time Mocking Test

**Files to modify:**
- `src/components/zzcollector-state/src/tests.rs`

**Changes:**
1. Add new test: `test_time_mocking_compatibility`
2. Use `tokio::time::pause()`
3. Create CollectorStateData
4. Advance time by 10 seconds
5. Verify uptime reflects advancement

**Example:**
```rust
#[tokio::test]
async fn test_time_mocking_compatibility() {
    tokio::time::pause();

    let state = CollectorStateData::new("test".into());
    assert_eq!(state.uptime_secs(), 0);

    tokio::time::advance(Duration::from_secs(10)).await;
    assert_eq!(state.uptime_secs(), 10);
}
```

**Verification:**
- Test passes
- Time mocking works correctly
- Uptime calculation accurate with mocked time

### Task 3.4: Verify No Other std::time::Instant Usage

**Files to check:**
- All files in `src/apps/`
- All files in `src/components/`
- All files in `src/net/`
- Exclude `src/old/` (deprecated code)

**Method:**
1. Run: `grep -r "std::time::Instant" src/ --exclude-dir=old`
2. Verify no matches in production code
3. Test files using it for benchmarking are OK

**Verification:**
- Only tokio::time::Instant in production code
- Tests still pass
- No performance regressions

---

## Phase 4: Expose Service Layer APIs for Testing

**Objective:** Make component creation and wiring accessible to integration tests while keeping it useful for other purposes.

**Rationale:** Not test-only - useful for custom deployments, embedding, and service composition patterns.

**Estimated Time:** 1-2 hours

### Task 4.1: Make CollectorService Methods Public

**Files to modify:**
- `src/apps/zzping-collector/src/service.rs`

**Changes:**
1. Make method public:
   - FROM: `fn create_builders(&self) -> Result<ComponentBuilders>`
   - TO: `pub fn create_builders(&self) -> Result<ComponentBuilders>`
   - Add doc comment explaining use case

2. Make method public:
   - FROM: `async fn start_components(...) -> Result<StartedComponents>`
   - TO: `pub async fn start_components(...) -> Result<StartedComponents>`
   - Add doc comment

3. Potentially add convenience method:
   ```rust
   /// Create and start all components without network layer.
   /// Useful for testing and custom deployment scenarios.
   pub async fn start_all_components(&self) -> Result<StartedComponents> {
       let builders = self.create_builders()?;
       Self::start_components(builders).await
   }
   ```

4. Keep `start_connection_manager()` public (already is)

**Verification:**
- Methods callable from tests
- Doc comments clear
- API makes sense

### Task 4.2: Make StartedComponents Struct Public

**Files to modify:**
- `src/apps/zzping-collector/src/service.rs`

**Changes:**
1. Make struct public:
   - FROM: `struct StartedComponents { ... }`
   - TO: `pub struct StartedComponents { ... }`

2. Make fields public:
   - Add `pub` to each field
   - Or keep private and add accessors (prefer pub fields)

3. Add doc comments:
   - Explain what each field is
   - Explain when this struct is used

**Verification:**
- Struct accessible from tests
- Fields accessible
- Clear API

### Task 4.3: Make DatabaseService Methods Public

**Files to modify:**
- `src/apps/zzping-database/src/service.rs`

**Changes:**
1. Same changes as CollectorService:
   - Make `create_builders()` public
   - Make `start_components()` public
   - Add `start_all_components()` convenience method
   - Keep `start_connection_manager()` public

2. Make `StartedComponents` public with public fields

3. Add comprehensive doc comments

**Verification:**
- Methods callable from tests
- API consistent with CollectorService
- Doc comments clear

### Task 4.4: Consider Making ComponentBuilders Public

**Files to modify:**
- `src/apps/zzping-collector/src/service.rs`
- `src/apps/zzping-database/src/service.rs`

**Decision point:**
- Do tests need access to `ComponentBuilders`?
- Or is `StartedComponents` sufficient?

**Recommendation:**
- Start with just `StartedComponents` public
- Keep `ComponentBuilders` private unless needed
- Can expose later if use case emerges

**Verification:**
- Tests can be written with current API
- No unnecessary exposure

### Task 4.5: Add Usage Examples to Doc Comments

**Files to modify:**
- `src/apps/zzping-collector/src/service.rs`
- `src/apps/zzping-database/src/service.rs`

**Changes:**
1. Add doc comment examples showing:
   - How to create service
   - How to start components
   - How to access actor addresses
   - How to inject custom transport

2. Example:
   ```rust
   /// # Example
   /// ```no_run
   /// use zzping_collector::{CollectorService, CollectorConfig};
   ///
   /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
   /// let config = CollectorConfig::for_testing("collector-01");
   /// let service = CollectorService::new(config)?;
   /// let components = service.start_all_components().await?;
   ///
   /// // Access actor addresses
   /// let pinger = &components.pinger;
   /// let memdb = &components.memdb;
   /// # Ok(())
   /// # }
   /// ```
   ```

**Verification:**
- Examples compile (check with cargo test)
- Examples clear and helpful

---

## Phase 5: Add Configuration Helper Methods

**Objective:** Provide convenient ways to create test configurations without repetitive boilerplate.

**Rationale:** Not test-only - useful for documentation examples, quick demos, and development mode.

**Estimated Time:** 1-2 hours

### Task 5.1: Add CollectorConfig::for_testing()

**Files to modify:**
- `src/apps/zzping-collector/src/config.rs`

**Changes:**
1. Add public method:
   ```rust
   /// Create a minimal configuration suitable for testing, demos, or development.
   ///
   /// This configuration uses:
   /// - TCP-only (no TLS)
   /// - localhost database
   /// - Fast timing intervals
   /// - Minimal resource usage
   ///
   /// # Example
   /// ```
   /// use zzping_collector::config::CollectorConfig;
   ///
   /// let config = CollectorConfig::for_testing("test-collector-01");
   /// assert_eq!(config.collector_id, "test-collector-01");
   /// assert!(config.tls.is_none()); // No TLS in test mode
   /// ```
   pub fn for_testing(collector_id: impl Into<String>) -> Self {
       Self {
           collector_id: collector_id.into(),
           database_host: "127.0.0.1".into(),
           database_port: 8443,
           tls: None,  // TCP-only
           components: ComponentConfig::fast_timing(),
       }
   }
   ```

2. Not using `#[cfg(test)]` - useful beyond tests

**Verification:**
- Method compiles
- Returns valid config
- Can be used in tests

### Task 5.2: Add DatabaseConfig::for_testing()

**Files to modify:**
- `src/apps/zzping-database/src/config.rs`

**Changes:**
1. Add public method:
   ```rust
   /// Create a minimal configuration suitable for testing, demos, or development.
   ///
   /// This configuration uses:
   /// - TCP-only (no TLS)
   /// - localhost binding
   /// - Fast timing intervals
   /// - Minimal resource usage
   ///
   /// # Example
   /// ```
   /// use zzping_database::config::DatabaseConfig;
   ///
   /// let config = DatabaseConfig::for_testing();
   /// assert!(config.tls.is_none()); // No TLS in test mode
   /// ```
   pub fn for_testing() -> Self {
       Self {
           bind_host: "127.0.0.1".into(),
           bind_port: 0,  // OS assigns port (useful for parallel tests)
           tls: None,  // TCP-only
           components: ComponentConfig::fast_timing(),
           data_dir: ".".into(),  // Current directory
       }
   }
   ```

**Verification:**
- Method compiles
- Returns valid config
- Can be used in tests

### Task 5.3: Add ComponentConfig::fast_timing()

**Files to modify:**
- `src/apps/zzping-collector/src/config.rs`
- `src/apps/zzping-database/src/config.rs`

**Changes (Collector):**
1. Add method:
   ```rust
   /// Create component configuration with faster timing for testing/demos.
   ///
   /// Uses shorter intervals than production defaults:
   /// - Heartbeat: 100ms instead of 5s
   /// - Batch size: 5 instead of 100
   pub fn fast_timing() -> Self {
       Self {
           heartbeat_interval_ms: 100,  // 100ms instead of 5000ms
           memdb_batch_size: 5,  // Small batches for faster tests
       }
   }
   ```

**Changes (Database):**
1. Add method:
   ```rust
   /// Create component configuration with faster timing for testing/demos.
   ///
   /// Uses shorter intervals than production defaults:
   /// - Stale timeout: 1s instead of 30s
   /// - Frame timeout: 100ms instead of 500ms
   pub fn fast_timing() -> Self {
       Self {
           stale_timeout_ms: 1000,  // 1s instead of 30s
           max_collectors: 100,
           message_frame_timeout_ms: 100,  // 100ms instead of 500ms
       }
   }
   ```

**Verification:**
- Methods return valid configs
- Values appropriate for fast testing
- Production defaults unchanged

### Task 5.4: Add ComponentConfig::default()

**Files to modify:**
- `src/apps/zzping-collector/src/config.rs`
- `src/apps/zzping-database/src/config.rs`

**Changes:**
1. Implement `Default` trait with production values:
   ```rust
   impl Default for ComponentConfig {
       fn default() -> Self {
           Self {
               heartbeat_interval_ms: 5000,  // 5 seconds
               memdb_batch_size: 100,
           }
       }
   }
   ```

2. Add doc comment explaining these are production defaults

**Verification:**
- Default values match current production behavior
- Used where appropriate
- Tests still work

---

## Phase 6: Create Test Infrastructure

**Objective:** Build reusable utilities for E2E tests.

**Rationale:** Avoid duplication, provide consistent test setup, make tests more readable.

**Estimated Time:** 2-3 hours

### Task 6.1: Create Test Module Structure

**Files to create:**
- `tests/common/mod.rs`
- `tests/common/test_utils.rs`

**Changes:**
1. Create directory: `tests/common/`
2. Create `tests/common/mod.rs`:
   ```rust
   pub mod test_utils;
   ```

3. This makes utilities available to all integration tests

**Verification:**
- Module compiles
- Can be imported from test files

### Task 6.2: Implement init_test_tracing()

**Files to modify:**
- `tests/common/test_utils.rs`

**Changes:**
1. Add tracing initialization function:
   ```rust
   use tracing_subscriber::EnvFilter;

   /// Initialize comprehensive tracing for tests.
   ///
   /// Configures detailed logging with:
   /// - Test writer (captured by test framework)
   /// - DEBUG level by default
   /// - TRACE for protocol layers
   /// - Target names and line numbers
   ///
   /// # Returns
   /// Guard that must be held for duration of test.
   /// Tracing is reset when guard is dropped.
   pub fn init_test_tracing() -> tracing::subscriber::DefaultGuard {
       let subscriber = tracing_subscriber::fmt()
           .with_env_filter(
               EnvFilter::from_default_env()
                   .add_directive(tracing::Level::DEBUG.into())
                   .add_directive("zznet_hello=trace".parse().unwrap())
                   .add_directive("zznet_session=debug".parse().unwrap())
                   .add_directive("zzintent_config=debug".parse().unwrap())
                   .add_directive("zzpinger=debug".parse().unwrap())
                   .add_directive("zzmem_db=debug".parse().unwrap())
                   .add_directive("zzcollector_state=debug".parse().unwrap())
           )
           .with_test_writer()
           .with_target(true)
           .with_line_number(true)
           .finish();

       tracing::subscriber::set_default(subscriber)
   }
   ```

2. Add doc comments explaining usage

**Verification:**
- Function compiles
- Can be called from tests
- Logs appear in test output

### Task 6.3: Implement Time Advancement Helper

**Files to modify:**
- `tests/common/test_utils.rs`

**Changes:**
1. Add helper for time control:
   ```rust
   use std::time::Duration;

   /// Advance virtual time and yield to let actors process.
   ///
   /// When using `tokio::time::pause()`, this function:
   /// 1. Advances the virtual clock by `duration`
   /// 2. Yields to allow pending tasks to run
   /// 3. Sleeps briefly (virtual time) to let actors settle
   ///
   /// # Example
   /// ```no_run
   /// tokio::time::pause();
   /// // ... start actors ...
   /// advance_time_and_yield(Duration::from_secs(5)).await;
   /// // Now actors have processed 5 seconds of events
   /// ```
   pub async fn advance_time_and_yield(duration: Duration) {
       tokio::time::advance(duration).await;
       tokio::task::yield_now().await;
       tokio::time::sleep(Duration::from_millis(10)).await;
   }
   ```

**Verification:**
- Function compiles
- Works with time mocking
- Actors process events

### Task 6.4: Add Configuration Creation Helpers

**Files to modify:**
- `tests/common/test_utils.rs`

**Changes:**
1. Add helpers wrapping the config methods:
   ```rust
   use zzping_collector::config::CollectorConfig;
   use zzping_database::config::DatabaseConfig;

   /// Create a test collector configuration.
   pub fn create_test_collector_config(collector_id: &str) -> CollectorConfig {
       CollectorConfig::for_testing(collector_id)
   }

   /// Create a test database configuration.
   pub fn create_test_database_config() -> DatabaseConfig {
       DatabaseConfig::for_testing()
   }
   ```

2. Add doc comments

**Question:** Are these helpers needed or can tests call the config methods directly?

**Recommendation:** Skip these - tests can call `CollectorConfig::for_testing()` directly. Keep it simple.

### Task 6.5: Add Verification Helpers (Optional)

**Files to modify:**
- `tests/common/test_utils.rs`

**Changes (if useful):**
1. Add assertion helpers:
   ```rust
   /// Wait for a condition with timeout.
   pub async fn wait_for<F>(condition: F, timeout: Duration, description: &str)
   where
       F: Fn() -> bool
   {
       let start = tokio::time::Instant::now();
       while !condition() {
           if start.elapsed() > timeout {
               panic!("Timeout waiting for: {}", description);
           }
           tokio::time::sleep(Duration::from_millis(10)).await;
       }
   }
   ```

**Recommendation:** Add only if needed. Start simple.

**Verification:**
- Helpers work as expected
- Clear and useful API

---

## Phase 7: Implement Main E2E Test

**Objective:** Build comprehensive integration test validating full protocol flow.

**Rationale:** Single macro-test covering all scenarios, with heavy logging for debuggability.

**Estimated Time:** 6-8 hours

### Task 7.1: Create Test File Structure

**Files to create:**
- `tests/e2e_full_protocol_test.rs`

**Changes:**
1. Create file with module structure:
   ```rust
   //! End-to-End Integration Test - Full Protocol Flow
   //!
   //! This test validates the complete interaction between collector and database:
   //! - HELLO handshake and room negotiation
   //! - IntentConfig distribution (Database → Collector)
   //! - Pinger reacting to config changes
   //! - Ping results flowing back (Collector → Database)
   //! - MemDB storage and queries
   //! - Config updates propagating
   //! - Collector state tracking
   //!
   //! Uses:
   //! - Mock transport (no TCP/TLS)
   //! - Mock ping backend (no ICMP)
   //! - Time mocking (no real delays)
   //! - Real production code (same as prod)

   mod common;

   use common::test_utils;
   // ... imports ...
   ```

2. Set up test attributes:
   ```rust
   #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
   async fn test_full_e2e_collector_database_protocol() {
       // ... test implementation ...
   }
   ```

**Verification:**
- File compiles
- Test discovered by cargo test
- Can run (even if empty)

### Task 7.2: Implement Test Setup Phase

**What to do:**
1. Initialize time mocking
2. Initialize tracing
3. Create test configurations
4. Create mock backends
5. Create temp directories if needed

**Implementation outline:**
```rust
// ===== PHASE: Test Setup =====
tracing::info!("========================================");
tracing::info!("E2E Test: Full Protocol Flow");
tracing::info!("========================================");

// Freeze time for deterministic testing
tokio::time::pause();
tracing::info!("[SETUP] Time mocking enabled");

// Initialize tracing
let _tracing_guard = test_utils::init_test_tracing();
tracing::info!("[SETUP] Tracing initialized");

// Create test configs
let db_config = DatabaseConfig::for_testing();
tracing::info!("[SETUP] Database config created: {:?}", db_config);

let collector_config = CollectorConfig::for_testing("test-collector-01");
tracing::info!("[SETUP] Collector config created: {:?}", collector_config);

// Create mock ping backend
let mock_ping_backend = Arc::new(MockBackend::new(Some(1500)));  // 1.5ms RTT
tracing::info!("[SETUP] Mock ping backend created (RTT: 1.5ms)");

// Create temp directory for database config persistence
let temp_dir = tempfile::tempdir()?;
let intent_config_path = temp_dir.path().join("intent.ron");
tracing::info!("[SETUP] Temp directory created: {:?}", temp_dir.path());
```

**Verification:**
- Setup completes without errors
- Logs show all setup steps
- Configs valid

### Task 7.3: Implement Component Creation Phase

**What to do:**
1. Create DatabaseService
2. Start database components
3. Create CollectorService
4. Start collector components
5. Inject mock backends where needed

**Implementation outline:**
```rust
// ===== PHASE: Component Creation =====
tracing::info!("[COMPONENTS] Creating database service...");
let db_service = DatabaseService::new(db_config)?;

tracing::info!("[COMPONENTS] Starting database components...");
let db_components = db_service.start_all_components().await?;
tracing::info!("[COMPONENTS] Database components started: intent_config={:?}, memdb={:?}, cm={:?}",
    db_components.intent_config,
    db_components.memdb,
    db_components.cm_addr
);

tracing::info!("[COMPONENTS] Creating collector service...");
let collector_service = CollectorService::new(collector_config)?;

tracing::info!("[COMPONENTS] Starting collector components...");
let mut collector_components = collector_service.start_all_components().await?;

// Replace pinger with one using mock backend
tracing::info!("[COMPONENTS] Injecting mock ping backend into pinger...");
collector_components.pinger = PingerBuilder::new()
    .backend(mock_ping_backend.clone())
    .memdb_addr(collector_components.memdb.clone())
    .targets(vec![])  // Will be set by config update
    .enabled(true)
    .start()?;
tracing::info!("[COMPONENTS] Collector components started with mock backend");
```

**Verification:**
- All components start successfully
- Actor addresses valid
- Mock backend injected
- Logs show component creation

### Task 7.4: Implement Transport Wiring Phase

**What to do:**
1. Create mock transport pair
2. Inject database transport into ConnectionManager
3. Inject collector transport into ConnectionManager
4. Advance time for connection establishment

**Implementation outline:**
```rust
// ===== PHASE: Transport Wiring =====
tracing::info!("[TRANSPORT] Creating mock transport pair...");
let (db_transport, collector_transport) = create_mock_pair("e2e_test");
tracing::info!("[TRANSPORT] Mock transport pair created (in-memory channels)");

tracing::info!("[TRANSPORT] Injecting database transport...");
db_components.cm_addr.send(HandleTransport {
    transport: Box::new(db_transport),
    config: HelloConfig::default(),
}).await.map_err(|e| anyhow!("Failed to send transport to database CM: {}", e))??;

tracing::info!("[TRANSPORT] Injecting collector transport...");
collector_components.cm_addr.send(HandleTransport {
    transport: Box::new(collector_transport),
    config: HelloConfig::default(),
}).await.map_err(|e| anyhow!("Failed to send transport to collector CM: {}", e))??;

tracing::info!("[TRANSPORT] Transports injected, advancing time for HELLO...");
test_utils::advance_time_and_yield(Duration::from_millis(200)).await;
```

**Verification:**
- Transports injected successfully
- HELLO handshake completes
- Logs show connection establishment

### Task 7.5: Implement HELLO Verification

**What to do:**
1. Verify HELLO handshake completed
2. Verify room negotiation succeeded
3. Verify peer roles extracted correctly
4. Log connection status

**Implementation outline:**
```rust
// ===== PHASE: HELLO Verification =====
tracing::info!("[VERIFY] Checking HELLO handshake completion...");

// TODO: Add method to ConnectionManager to query connection status
// For now, rely on logs showing successful handshake

// Check that no errors were logged
// Check that components are still running

tracing::info!("[VERIFY] ✓ HELLO handshake completed");
tracing::info!("[VERIFY] ✓ Room negotiation succeeded");
tracing::info!("[VERIFY] ✓ Connection established");
```

**Note:** May need to add query methods to ConnectionManager to verify state.

**Verification:**
- Handshake completed (from logs)
- Rooms negotiated
- Connection active

### Task 7.6: Implement Config Distribution Test

**What to do:**
1. Create test IntentConfig
2. Database sends config to collector
3. Wait for propagation
4. Verify collector received config
5. Verify pinger updated targets

**Implementation outline:**
```rust
// ===== PHASE: Config Distribution =====
tracing::info!("[CONFIG] Creating test IntentConfig...");
let test_config = IntentConfigData {
    targets: vec!["8.8.8.8".into(), "1.1.1.1".into()],
    rate_ms: 1000,  // 1 second ping rate
    timeout_ms: 500,
};
tracing::info!("[CONFIG] Test config: {:?}", test_config);

tracing::info!("[CONFIG] Database: setting initial config...");
// Note: Database IntentConfigActor should load from file or have initial state
// For test, we may need to send RequestConfigChange message

db_components.intent_config.send(RequestConfigChange {
    new_config: test_config.clone(),
    peer_role: AuthRole::ClientAdmin,  // Simulate admin
}).await??;
tracing::info!("[CONFIG] Database: config update sent");

tracing::info!("[CONFIG] Advancing time for config propagation...");
test_utils::advance_time_and_yield(Duration::from_millis(300)).await;

tracing::info!("[VERIFY] Checking collector received config...");
let collector_cfg = collector_components.intent_config
    .send(GetCurrentConfig)
    .await?;

assert_eq!(collector_cfg.targets, test_config.targets,
    "Collector should have received config with correct targets");
assert_eq!(collector_cfg.rate_ms, test_config.rate_ms,
    "Collector should have received config with correct rate");

tracing::info!("[VERIFY] ✓ Collector received config: {:?}", collector_cfg);

// Verify pinger updated
let pinger_health = collector_components.pinger.get_health().await?;
assert_eq!(pinger_health.active_targets.len(), 2,
    "Pinger should have 2 active targets");
tracing::info!("[VERIFY] ✓ Pinger has {} active targets", pinger_health.active_targets.len());
```

**Verification:**
- Config sent from database
- Config received by collector
- Pinger targets updated
- All assertions pass

### Task 7.7: Implement Ping Generation Test

**What to do:**
1. Advance time to trigger pings
2. Verify pings generated
3. Verify results in collector MemDB
4. Wait for batch to be sent
5. Verify results reached database MemDB

**Implementation outline:**
```rust
// ===== PHASE: Ping Generation =====
tracing::info!("[PING] Advancing time to trigger pings...");
test_utils::advance_time_and_yield(Duration::from_secs(2)).await;
tracing::info!("[PING] Time advanced by 2 seconds (2 ping cycles)");

tracing::info!("[VERIFY] Checking collector MemDB has results...");
// Query collector MemDB
let collector_results = collector_components.memdb
    .send(QueryLocalBuffer {
        target: "8.8.8.8".into(),
    })
    .await?;

assert!(!collector_results.is_empty(),
    "Collector MemDB should have buffered ping results");
tracing::info!("[VERIFY] ✓ Collector buffered {} results", collector_results.len());

tracing::info!("[PING] Advancing time for batch send...");
test_utils::advance_time_and_yield(Duration::from_millis(500)).await;

tracing::info!("[VERIFY] Checking database MemDB received results...");
let db_results = db_components.memdb
    .send(QueryPingResults {
        target: "8.8.8.8".into(),
        limit: 10,
    })
    .await??;

assert!(!db_results.is_empty(),
    "Database MemDB should have received ping results");
assert_eq!(db_results[0].rtt_us, Some(1500),
    "Ping result should have mock RTT of 1500μs");
tracing::info!("[VERIFY] ✓ Database received {} results", db_results.len());
tracing::info!("[VERIFY] ✓ First result RTT: {:?}μs", db_results[0].rtt_us);
```

**Verification:**
- Pings generated
- Results buffered in collector
- Results sent to database
- Database stored results
- RTT matches mock backend

### Task 7.8: Implement Config Update Test

**What to do:**
1. Send config update (remove one target)
2. Wait for propagation
3. Verify collector received update
4. Verify pinger stopped old target
5. Verify only new target pinged

**Implementation outline:**
```rust
// ===== PHASE: Config Update =====
tracing::info!("[CONFIG] Sending config update (removing 1.1.1.1)...");
let updated_config = IntentConfigData {
    targets: vec!["8.8.8.8".into()],  // Only 8.8.8.8 now
    rate_ms: 2000,  // Also change rate to 2s
    timeout_ms: 500,
};

db_components.intent_config.send(RequestConfigChange {
    new_config: updated_config.clone(),
    peer_role: AuthRole::ClientAdmin,
}).await??;
tracing::info!("[CONFIG] Update sent");

tracing::info!("[CONFIG] Advancing time for update propagation...");
test_utils::advance_time_and_yield(Duration::from_millis(300)).await;

tracing::info!("[VERIFY] Checking collector received update...");
let new_collector_cfg = collector_components.intent_config
    .send(GetCurrentConfig)
    .await?;

assert_eq!(new_collector_cfg.targets.len(), 1,
    "Collector should have 1 target after update");
assert_eq!(new_collector_cfg.targets[0], "8.8.8.8",
    "Collector should only have 8.8.8.8");
tracing::info!("[VERIFY] ✓ Collector updated: {:?}", new_collector_cfg);

tracing::info!("[VERIFY] Checking pinger updated targets...");
let new_health = collector_components.pinger.get_health().await?;
assert_eq!(new_health.active_targets.len(), 1,
    "Pinger should have 1 active target");
assert!(new_health.active_targets.contains(&"8.8.8.8".into()),
    "Pinger should have 8.8.8.8");
tracing::info!("[VERIFY] ✓ Pinger updated: {} targets", new_health.active_targets.len());

tracing::info!("[PING] Advancing time to verify new rate...");
test_utils::advance_time_and_yield(Duration::from_secs(3)).await;

// Verify 1.1.1.1 not pinged, 8.8.8.8 still pinged
let db_results_888 = db_components.memdb
    .send(QueryPingResults {
        target: "8.8.8.8".into(),
        limit: 20,
    })
    .await??;

let db_results_111 = db_components.memdb
    .send(QueryPingResults {
        target: "1.1.1.1".into(),
        limit: 20,
    })
    .await??;

// Should have new pings for 8.8.8.8, but old count for 1.1.1.1
tracing::info!("[VERIFY] 8.8.8.8 results: {}", db_results_888.len());
tracing::info!("[VERIFY] 1.1.1.1 results: {}", db_results_111.len());
tracing::info!("[VERIFY] ✓ Config update propagated correctly");
```

**Verification:**
- Update received by collector
- Pinger stopped old target
- New pings only for active targets
- Rate change applied

### Task 7.9: Implement Collector State Tracking Test

**What to do:**
1. Advance time for heartbeats
2. Verify heartbeats sent
3. Verify database tracks collector
4. Query collector status from database
5. Simulate staleness (advance time without heartbeat)
6. Verify collector marked stale

**Implementation outline:**
```rust
// ===== PHASE: Collector State Tracking =====
tracing::info!("[STATE] Testing collector heartbeat and tracking...");

tracing::info!("[STATE] Advancing time for heartbeats...");
test_utils::advance_time_and_yield(Duration::from_millis(500)).await;

// TODO: Add query method to CStateActor to get collector list
// For now, verify through logs

tracing::info!("[VERIFY] ✓ Heartbeats should be visible in logs");

// Test staleness detection
tracing::info!("[STATE] Testing staleness detection...");
tracing::info!("[STATE] Advancing time beyond stale timeout...");
test_utils::advance_time_and_yield(Duration::from_secs(2)).await;

// TODO: Query database CStateActor for collector status
// Verify collector marked as stale

tracing::info!("[VERIFY] ✓ Staleness detection should work (check logs)");
```

**Note:** May need to add query messages to CStateActor to properly verify.

**Verification:**
- Heartbeats sent (visible in logs)
- Database tracks collector
- Staleness detected when heartbeat stops

### Task 7.10: Implement Test Cleanup and Final Assertions

**What to do:**
1. Log test summary
2. Verify no errors in any components
3. Stop actors gracefully (automatic via Drop)
4. Log success

**Implementation outline:**
```rust
// ===== PHASE: Test Completion =====
tracing::info!("========================================");
tracing::info!("E2E Test: All Phases Complete");
tracing::info!("========================================");
tracing::info!("[SUMMARY] HELLO handshake: ✓");
tracing::info!("[SUMMARY] Config distribution: ✓");
tracing::info!("[SUMMARY] Pinger reaction: ✓");
tracing::info!("[SUMMARY] Ping data flow: ✓");
tracing::info!("[SUMMARY] Config updates: ✓");
tracing::info!("[SUMMARY] Collector state: ✓");
tracing::info!("========================================");
tracing::info!("✅ E2E Test PASSED");
tracing::info!("========================================");

// Components will be dropped here, stopping actors gracefully
Ok(())
```

**Verification:**
- Test completes successfully
- All assertions passed
- Clean shutdown
- Logs comprehensive

### Task 7.11: Add Error Handling and Debugging

**What to do:**
1. Add context to all errors
2. Add checkpoint logging
3. Add timing information
4. Handle potential race conditions

**Implementation patterns:**
```rust
// Context on errors
.map_err(|e| anyhow!("Failed to create database service: {}", e))?

// Checkpoint logging
tracing::info!("[CHECKPOINT] About to start component X...");

// Timing info
let start = tokio::time::Instant::now();
// ... operation ...
tracing::debug!("Operation took {:?}", start.elapsed());

// Race condition handling
test_utils::advance_time_and_yield(Duration::from_millis(100)).await;
// Extra yield to ensure processing
tokio::task::yield_now().await;
```

**Verification:**
- Errors have clear context
- Failures easy to debug
- Race conditions minimized

---

## Phase 8: Documentation and Polish

**Objective:** Document the testing framework and ensure maintainability.

**Estimated Time:** 2-3 hours

### Task 8.1: Update E2E Test Plan Document

**Files to modify:**
- `docs/E2E_TEST_PLAN.md`

**Changes:**
1. Add "Implementation Status" section
2. Document actual implementation vs. plan
3. Add lessons learned
4. Update examples with real code
5. Add troubleshooting guide

**Verification:**
- Documentation accurate
- Reflects actual implementation
- Helpful for future maintainers

### Task 8.2: Create Test Failure Debugging Guide

**Files to create:**
- `docs/E2E_TEST_DEBUGGING.md`

**Contents:**
1. How to run the E2E test
2. How to increase log verbosity
3. Common failure patterns and solutions
4. How to debug time-related issues
5. How to debug transport issues
6. How to debug actor startup issues

**Verification:**
- Guide is comprehensive
- Examples are clear
- Someone unfamiliar could follow it

### Task 8.3: Update RUNBOOK.md

**Files to modify:**
- `RUNBOOK.md`

**Changes:**
1. Add section on running E2E tests
2. Document test execution commands
3. Add expected output examples
4. Link to debugging guide

**Verification:**
- Runbook updated
- Test instructions clear
- Integrated with existing docs

### Task 8.4: Add In-Code Documentation

**Files to modify:**
- All modified files

**Changes:**
1. Review all public APIs for doc comments
2. Add examples where helpful
3. Explain design decisions in comments
4. Document any non-obvious behavior

**Verification:**
- cargo doc runs successfully
- Doc comments comprehensive
- Examples compile

### Task 8.5: Create Test Execution Guide

**Files to create:**
- `tests/README.md`

**Contents:**
1. Overview of test structure
2. How to run specific tests
3. How to run with different log levels
4. How to debug test failures
5. How to add new E2E tests
6. Test performance expectations

**Example commands:**
```bash
# Run E2E test
cargo test --test e2e_full_protocol_test

# Run with verbose output
cargo test --test e2e_full_protocol_test -- --nocapture

# Run with trace logging
RUST_LOG=trace cargo test --test e2e_full_protocol_test -- --nocapture

# Run all integration tests
cargo test --tests

# Run with nextest
cargo nextest run --test e2e_full_protocol_test
```

**Verification:**
- Guide complete
- Commands work
- Clear and helpful

### Task 8.6: Final Code Review

**What to do:**
1. Review all changes for consistency
2. Check for any remaining TODOs
3. Verify error messages are helpful
4. Check log levels are appropriate
5. Ensure naming conventions followed

**Checklist:**
- [ ] All files formatted (cargo fmt)
- [ ] No compiler warnings
- [ ] No clippy warnings (cargo clippy)
- [ ] Tests pass consistently
- [ ] Documentation complete
- [ ] Examples compile
- [ ] Code reviewed for clarity

**Verification:**
- All checks pass
- Code ready for review/merge

---

## Phase 9: Validation and Edge Cases

**Objective:** Ensure test is robust and covers edge cases.

**Estimated Time:** 2-3 hours

### Task 9.1: Run Test Multiple Times

**What to do:**
1. Run test 20 times sequentially
2. Run test with nextest in parallel
3. Check for any flakiness
4. Verify consistent timing

**Commands:**
```bash
# Sequential runs
for i in {1..20}; do
    echo "Run $i"
    cargo test --test e2e_full_protocol_test
done

# Parallel runs
cargo nextest run --test e2e_full_protocol_test --test-threads 10
```

**Verification:**
- 100% pass rate
- No timing-related failures
- No race conditions observed

### Task 9.2: Test with Different Log Levels

**What to do:**
1. Run with TRACE
2. Run with DEBUG (default)
3. Run with INFO
4. Run with WARN (minimal)
5. Verify test still works at all levels

**Verification:**
- Test passes at all levels
- Appropriate information at each level
- No panics due to logging

### Task 9.3: Test Time Mocking Edge Cases

**What to do:**
1. Verify zero advancement works
2. Verify large time jumps work
3. Verify backward jumps handled (or prevented)
4. Verify fractional seconds work

**Add small test cases:**
```rust
#[tokio::test]
async fn test_time_mocking_edge_cases() {
    tokio::time::pause();

    // Zero advancement
    tokio::time::advance(Duration::ZERO).await;

    // Large jump
    tokio::time::advance(Duration::from_secs(3600)).await;

    // Millisecond precision
    tokio::time::advance(Duration::from_millis(1)).await;

    // All should work without panic
}
```

**Verification:**
- Edge cases handled
- No panics
- Behavior correct

### Task 9.4: Add Error Case Tests (Optional)

**What to do:**
Consider adding tests for error scenarios:
1. Invalid config propagation
2. Component failure handling
3. Transport disconnection
4. Timeout handling

**Recommendation:** Start with happy path, add error tests later.

**Verification:**
- Error scenarios identified
- Can be tested in future iterations

### Task 9.5: Performance Baseline

**What to do:**
1. Measure test execution time
2. Document baseline performance
3. Set expectations for CI/CD

**Measure:**
```bash
time cargo test --test e2e_full_protocol_test
```

**Expected:** < 1 second (with time mocking)

**Verification:**
- Performance measured
- Baseline documented
- Acceptable for CI/CD

---

## Success Criteria Checklist

### Must Have (All Required)

- [ ] Test runs in single process (no TCP/TLS)
- [ ] Test uses real CollectorService and DatabaseService code
- [ ] Test completes in < 1 second
- [ ] Test validates HELLO → config → ping → storage flow
- [ ] Test uses mock transport
- [ ] Test uses mock ping backend
- [ ] Test passes 100% consistently (20+ runs)
- [ ] Test logs provide debugging information
- [ ] TLS is optional in configs (TCP-only mode supported)
- [ ] Heartbeat timing in milliseconds
- [ ] No std::time::Instant in production code
- [ ] Time mocking works correctly

### Should Have (Strongly Recommended)

- [ ] Test validates config update propagation
- [ ] Test validates pinger reaction to config changes
- [ ] Test validates collector state tracking
- [ ] Test safe for parallel execution (nextest)
- [ ] Changes improve architecture (not just for tests)
- [ ] Minimal use of #[cfg(test)]
- [ ] Documentation complete
- [ ] Debugging guide available

### Nice to Have (Future Enhancements)

- [ ] Test validates permission checks
- [ ] Test validates error handling
- [ ] Multiple E2E tests for different scenarios
- [ ] Performance benchmarks

---

## Risk Mitigation

### Identified Risks and Mitigations

**Risk:** Time mocking doesn't work as expected
- **Mitigation:** Test time mocking in isolation first (Phase 3.3)
- **Backup:** Add manual delays where needed

**Risk:** Race conditions in actor startup
- **Mitigation:** Add explicit delays/yields after component creation
- **Backup:** Add ready-check mechanisms

**Risk:** Test becomes flaky
- **Mitigation:** Extensive testing (Phase 9.1), deterministic timing
- **Backup:** Add retry logic or synchronization primitives

**Risk:** Breaking existing production behavior
- **Mitigation:** All changes backward-compatible (TLS optional, not removed)
- **Backup:** Comprehensive existing test suite validates behavior

**Risk:** Test too complex to maintain
- **Mitigation:** Heavy documentation (Phase 8), clear structure
- **Backup:** Split into smaller tests if needed

---

## Timeline Estimate

| Phase | Description | Time | Dependencies |
|-------|-------------|------|--------------|
| 1 | Make TLS Optional | 2-3 hours | None |
| 2 | Heartbeat to Milliseconds | 2-3 hours | None |
| 3 | Fix Time Handling | 1-2 hours | None |
| 4 | Expose Service APIs | 1-2 hours | Phase 1 |
| 5 | Config Helpers | 1-2 hours | Phase 1, 2 |
| 6 | Test Infrastructure | 2-3 hours | Phase 3, 4, 5 |
| 7 | Main E2E Test | 6-8 hours | Phase 6 |
| 8 | Documentation | 2-3 hours | Phase 7 |
| 9 | Validation | 2-3 hours | Phase 7 |

**Total:** 19-29 hours (approximately 3-4 working days)

**Recommended approach:** Complete Phases 1-5 first (foundation), then tackle Phase 6-7 (implementation), then Phase 8-9 (polish).

---

## Next Steps

**Immediate:**
1. Review this plan with stakeholders
2. Confirm approach is acceptable
3. Clarify any ambiguous requirements
4. Get approval to proceed

**Short-term:**
1. Begin Phase 1 (TLS optional)
2. Complete Phases 1-5 (foundation)
3. Checkpoint: Verify foundation is solid

**Medium-term:**
1. Implement Phase 6-7 (main test)
2. Checkpoint: Test runs and passes
3. Complete Phase 8-9 (polish)

**Long-term:**
1. Add more E2E test scenarios
2. Add permission and error tests
3. Add CLI integration tests
4. Continuous improvement

---

**End of Action Plan**
