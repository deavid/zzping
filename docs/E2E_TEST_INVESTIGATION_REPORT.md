# End-to-End Testing Investigation Report
**Date:** October 19, 2025
**Project:** ZZPing Monitoring System
**Purpose:** Design and implement comprehensive E2E integration tests for collector ↔ database communication

---

## Executive Summary

This report documents the investigation and design of an end-to-end (E2E) testing framework for the ZZPing monitoring system. The goal is to create comprehensive integration tests that validate the complete protocol flow between collector and database applications **without requiring TCP/TLS or separate processes**, while still running as much real production code as possible.

### Key Outcomes

1. **Architecture is test-friendly**: Mock transport already exists and is properly abstracted
2. **Time mocking is critical**: Tests need `tokio::time::pause()` to avoid long waits
3. **Minimal changes required**: Most needed changes are good architectural improvements, not test-specific hacks
4. **One critical bug found**: `std::time::Instant` usage in `zzcollector-state` breaks time mocking
5. **Config granularity issue**: Heartbeat intervals defined in seconds, should be milliseconds

---

## Problem Statement

### Current Testing Gaps

**What exists today:**
- ✅ Unit tests for individual components
- ✅ Integration tests for protocol layers (HELLO, transport)
- ✅ TCP-based integration tests (require separate processes)
- ✅ Mock transport implementation (in `zznet-api`)

**What's missing:**
- ❌ E2E test of full collector ↔ database interaction
- ❌ Tests of config distribution triggering pinger behavior
- ❌ Tests of ping data flowing from collector to database storage
- ❌ Tests of permission checks and role enforcement
- ❌ Tests of state synchronization (collector-state tracking)
- ❌ Tests catching serialization/deserialization bugs

### Why This Matters

**Manual testing is painful:**
- Requires launching two separate processes
- Needs real TLS certificates
- Hard to debug protocol issues
- AI agents can't easily control two processes
- Risk of accidentally using production configs/data

**Current test gaps hide bugs:**
- Peer role extraction showing as `None` in logs
- Room negotiation issues hard to reproduce
- Serialization bugs only caught in production
- Permission checks not comprehensively validated

### Requirements

**Must have:**
1. Test REAL production code (collector and database services)
2. Run in single process (no TCP/TLS, use mock transport)
3. Fast execution (mock time, mock ICMP)
4. Deterministic (no race conditions)
5. Test ALL business logic:
   - IntentConfig distribution (Database → Collector)
   - Pinger reacting to config (targets, rates)
   - Ping results flowing back (Collector → Database)
   - MemDB storage and queries
   - Config updates propagating
   - Collector-state heartbeats and staleness
6. Heavy logging for debugging failures
7. Safe for parallel execution with nextest

**Must NOT have:**
- Test-only code paths that bypass production logic
- Excessive `#[cfg(test)]` conditionals
- Changes that make production code worse

---

## Current Architecture Analysis

### Service Layer Structure

Both `CollectorService` and `DatabaseService` follow this pattern:

```
main.rs
  └─> service::run()
        ├─> create_builders()              [PRIVATE]
        ├─> start_components()             [PRIVATE]
        ├─> start_connection_manager()     [PRIVATE]
        ├─> create_tcp_network()           [HARDCODED TCP/TLS]
        └─> wait_for_shutdown_signal()     [BLOCKS FOREVER]
```

**Problem:** Everything is private, TCP/TLS is hardcoded, test can't inject mock transport.

**Solution:** Expose component creation, make network layer injectable.

### Configuration Structure

**Current:**
```rust
pub struct CollectorConfig {
    pub collector_id: String,
    pub database_host: String,
    pub database_port: u16,
    pub tls: TlsConfig,  // ← MANDATORY
    pub components: ComponentConfig,
}
```

**Problems:**
1. TLS is mandatory (validates cert files exist)
2. No way to create test configs without files
3. Heartbeat timing in seconds (too coarse)

**Solution:**
- Make `tls: Option<TlsConfig>` (TCP-only mode)
- Add helper constructors for testing
- Change heartbeat from seconds to milliseconds

### Component Architecture

**IntentConfigActor:**
- ✅ Supports Database and Collector roles
- ✅ Database role persists to file
- ⚠️ File path is mandatory for Database role
- 💡 Could use trait for persistence (file vs memory)

**PingerActor:**
- ✅ Already supports `PingBackend` trait
- ✅ `MockBackend` exists
- ✅ `RealPingBackend` for production
- ✅ Injectable via `PingerBuilder`

**MemDBActor:**
- ✅ Collector role: buffers results
- ✅ Database role: stores with limits
- ⚠️ Database role might persist (not currently implemented)
- 💡 Could use trait for persistence if needed

**CStateActor (Collector-State):**
- ✅ Tracks collector heartbeats and staleness
- ❌ **BUG**: Uses `std::time::Instant` (breaks time mocking)
- 🔧 **MUST FIX**: Change to `tokio::time::Instant`

### Transport Layer

**TCP Transport:**
- ✅ `TcpTransportClient` and `TcpTransportServer`
- ✅ Support both plain TCP and TLS
- ✅ Implement `TransportClient` and `TransportServer` traits

**Mock Transport:**
- ✅ `MockConnection` implements `TransportConnection`
- ✅ `create_mock_pair()` creates connected pair
- ✅ In-memory channels (microsecond latency)
- ✅ Used in protocol layer tests
- ❌ **NOT used in any app-level tests**

**ConnectionManager:**
- ✅ Accepts transports via `HandleTransport` message
- ✅ Transport-agnostic (works with any impl)
- ✅ Spawns `HelloActor` for handshake
- ✅ Manages session lifecycle

---

## Time Mocking Strategy

### The Problem

Production code has timing-sensitive behavior:
```rust
// PingerActor loop
loop {
    ping_once().await;
    tokio::time::sleep(Duration::from_millis(rate_ms)).await;  // Wait!
}

// CStateActor heartbeat
tokio::time::interval(Duration::from_secs(heartbeat_interval_secs));
```

If `rate_ms = 5000` (5 seconds), test must wait 5 real seconds. **Unacceptable.**

### Solution: tokio::time::pause()

Tokio provides built-in time mocking:

```rust
#[tokio::test]
async fn test_with_time_control() {
    tokio::time::pause();  // Freeze time

    // Start actors with real timing configs
    let pinger = start_pinger_with_5s_rate();

    // Advance time by 5 seconds INSTANTLY
    tokio::time::advance(Duration::from_secs(5)).await;

    // Pinger should have fired!
    assert_ping_happened();
}
```

**How it works:**
- `tokio::time::pause()` freezes the runtime's internal clock
- `tokio::time::advance()` moves clock forward without real delay
- All `tokio::time::sleep()` and `tokio::time::interval()` use this clock
- Tests run in milliseconds instead of minutes

### Requirements for Time Mocking

**✅ MUST use:**
- `tokio::time::sleep()`
- `tokio::time::interval()`
- `tokio::time::Instant` (for elapsed time tracking)

**❌ MUST NOT use:**
- `std::time::Instant` (reads real system clock)
- `std::thread::sleep()` (blocks thread, ignores Tokio)
- `SystemTime::now()` (reads real system clock)

### Current Code Audit

**Found issues:**
1. ✅ Most code already uses `tokio::time::sleep` - GOOD
2. ❌ `zzcollector-state` uses `std::time::Instant` - **MUST FIX**
3. ⚠️ Heartbeat interval in seconds - **SHOULD CHANGE TO MS**

**Files in `src/old/` using std::time:**
- Not relevant (old code being replaced)

**Test files using std::time::Instant:**
- `src/net/zznet-auth/tests/stress_test.rs` - OK (measuring real perf)
- `src/common/zzping-auth/tests/stress_test.rs` - OK (measuring real perf)

---

## Design: E2E Test Architecture

### Test Objectives

**Primary goal:** Validate full protocol flow with real business logic

**Test scenarios:**
1. **Basic connectivity:**
   - HELLO handshake completes
   - Room negotiation succeeds
   - Peer roles extracted correctly

2. **Config distribution:**
   - Database starts with IntentConfig
   - Config sent to collector via network
   - Collector receives and applies config

3. **Pinger reaction:**
   - Pinger starts with initial targets
   - Config update changes targets
   - Pinger stops old targets, starts new ones
   - Ping rate changes applied

4. **Ping data flow:**
   - Pinger generates mock ping results
   - Results sent to MemDB (collector side)
   - Results forwarded to database via network
   - Database MemDB receives and stores

5. **Storage and queries:**
   - Database stores ping results with limits
   - Query retrieves correct results
   - Per-target limits enforced

6. **Dynamic config changes:**
   - Admin sends config update to database
   - Database persists new config
   - Update sent to all collectors
   - Collectors react to changes

7. **Collector state tracking:**
   - Collector sends heartbeats
   - Database tracks collector status
   - Staleness detection works
   - Metrics flow correctly

### Test Structure

```
Test Setup Phase:
  1. Initialize time mocking (tokio::time::pause)
  2. Initialize tracing (DEBUG level, test writer)
  3. Create test configs (no TLS, fast timing)
  4. Create mock backends (ping, persistence if needed)

Component Creation Phase:
  5. Create DatabaseService with test config
  6. Start database components (real code!)
  7. Create CollectorService with test config
  8. Start collector components (real code!)
  9. Inject mock ping backend into pinger

Transport Wiring Phase:
  10. Create mock transport pair
  11. Inject database transport into ConnectionManager
  12. Inject collector transport into ConnectionManager

Protocol Flow Phase:
  13. Advance time for HELLO handshake
  14. Verify handshake completed
  15. Verify room negotiation
  16. Database sends initial IntentConfig
  17. Advance time for config propagation
  18. Verify collector received config

Ping Generation Phase:
  19. Advance time to trigger pings
  20. Verify pings generated
  21. Verify results in collector MemDB
  22. Advance time for batch send
  23. Verify results reached database

Config Update Phase:
  24. Send config update to database
  25. Advance time for propagation
  26. Verify collector received update
  27. Verify pinger updated targets
  28. Advance time for new pings
  29. Verify new targets pinged

State Tracking Phase:
  30. Advance time for heartbeats
  31. Verify heartbeats sent
  32. Verify database tracks collector
  33. Simulate staleness (no heartbeat)
  34. Verify collector marked stale

Cleanup Phase:
  35. Stop all actors gracefully
  36. Verify no errors in logs
```

### Logging Strategy

**Principle:** Tests should be debuggable from logs alone.

**Logging levels:**
- Test framework: `INFO` (major phases)
- Protocol layer: `DEBUG` (message flow)
- HELLO/Session: `TRACE` (handshake details)
- Components: `DEBUG` (state changes)

**Log markers:**
```
[TEST SETUP] Creating database service...
[TEST SETUP] Creating collector service...
[DATABASE] IntentConfigActor started with config_file: /tmp/test123/intent.ron
[COLLECTOR] PingerActor started with 2 targets
[PROTOCOL] HELLO handshake initiated
[PROTOCOL] Room negotiation: client offered [intent-config], server offered [memdb, query]
[PROTOCOL] Active rooms: [intent-config]
[CONFIG] Database sending IntentConfig to peer collector-01
[CONFIG] Collector received IntentConfig: 2 targets, rate=1000ms
[PINGER] Starting ping tasks for targets: [8.8.8.8, 1.1.1.1]
[PINGER] Ping result: 8.8.8.8 → 1.5ms
[MEMDB] Collector buffered 1 result
[NETWORK] Sending MemDB batch (5 results) to database
[MEMDB] Database received batch: 5 results for 2 targets
[TEST VERIFY] ✓ Ping data reached database
[CONFIG] Admin updating config: removing target 1.1.1.1
[CONFIG] Collector received update, stopping target 1.1.1.1
[PINGER] Stopped ping task for 1.1.1.1
[TEST VERIFY] ✓ Pinger updated targets
[HEARTBEAT] Collector sending heartbeat (uptime: 5s)
[HEARTBEAT] Database received heartbeat from collector-01
[TEST VERIFY] ✓ All assertions passed
```

---

## Required Changes

### 1. Config Layer Changes

**CollectorConfig:**
- Change `tls: TlsConfig` → `tls: Option<TlsConfig>`
- Update validation to skip TLS checks when None
- Add `fn for_testing(collector_id: impl Into<String>) -> Self`
- Change `heartbeat_interval_secs: u64` → `heartbeat_interval_ms: u64`

**DatabaseConfig:**
- Change `tls: TlsConfig` → `tls: Option<TlsConfig>`
- Update validation to skip TLS checks when None
- Add `fn for_testing() -> Self`
- Change `stale_timeout_secs: u64` → `stale_timeout_ms: u64` (if exists)

**ComponentConfig:**
- Add `fn fast_timing() -> Self` (1s → 100ms timing)
- Change all `*_secs` fields to `*_ms` fields

**Rationale:** These are not test-only changes. They improve the architecture:
- TCP-only mode is valid for development/trusted networks
- Helper constructors useful for documentation and examples
- Millisecond precision needed for responsive systems

### 2. Service Layer Changes

**CollectorService:**
- Make `fn start_all_components(&self)` public
- Make `struct StartedComponents` public
- Make `fn create_connection_manager(&self)` public (already exists)
- Update `run()` to handle `Option<TlsConfig>`

**DatabaseService:**
- Make `fn start_all_components(&self)` public
- Make `struct StartedComponents` public
- Make `fn create_connection_manager(&self)` public
- Update `run()` to handle `Option<TlsConfig>`

**Rationale:** Not test-only. Useful for:
- Custom deployment scenarios
- Embedding in other applications
- Integration with other frameworks
- Service composition patterns

### 3. Time Handling Fix (CRITICAL)

**zzcollector-state/src/state.rs:**
- Change `use std::time::Instant` → `use tokio::time::Instant`
- Change `start_time: Instant` → `start_instant: tokio::time::Instant`
- Change `Instant::now()` → `tokio::time::Instant::now()`
- Update `uptime_secs()` calculation

**CStateRole:**
- Change `heartbeat_interval_secs: u64` → `heartbeat_interval_ms: u64`
- Update all usages

**Rationale:** This is a BUG FIX. Using `std::time::Instant` in async code that might be tested is incorrect. `tokio::time::Instant` is the proper choice for Tokio-based async applications.

### 4. Network Layer (Already OK)

No changes needed! Already supports:
- Plain TCP mode (when TLS config is None)
- Mock transport injection via ConnectionManager
- Transport abstraction working correctly

### 5. Component Layer (Mostly OK)

**PingerActor:**
- ✅ Already supports backend injection
- ✅ MockBackend exists
- ✅ No changes needed

**IntentConfigActor:**
- ✅ Database role can use tempfile for testing
- ⚠️ Consider adding `ConfigPersistence` trait (future improvement)
- ✅ No changes needed for initial E2E test

**MemDBActor:**
- ✅ In-memory storage works for tests
- ✅ No persistence needed initially
- ⚠️ Consider adding `ResultsPersistence` trait (future improvement)
- ✅ No changes needed for initial E2E test

**CStateActor:**
- ❌ Must fix time handling (see section 3)
- ✅ Otherwise ready

---

## Testing Strategy

### Test Organization

**Single comprehensive E2E test:**
- Name: `test_full_e2e_collector_database_protocol`
- File: `tests/e2e_full_protocol_test.rs`
- Length: ~300-500 lines
- Approach: Macro-test covering all scenarios

**Rationale for macro-test:**
- Validates full integration in one run
- Ensures scenarios don't interfere
- Simpler than coordinating multiple tests
- Easier to understand complete flow
- Better logging continuity

### Test Utilities

**Module: `tests/common/test_utils.rs`:**
```rust
- init_test_tracing() -> tracing::subscriber::DefaultGuard
- advance_time_and_yield(duration: Duration)
- create_test_database_config() -> DatabaseConfig
- create_test_collector_config(id: &str) -> CollectorConfig
- verify_hello_handshake_complete(...)
- verify_config_propagated(...)
```

**Not using #[cfg(test)]:**
- Test utilities are only in `tests/` directory
- Production code doesn't know about them
- No conditional compilation needed

### Parallel Execution Safety

**Each test creates:**
- Fresh actor instances
- Separate mock transports
- Independent tempfiles (if needed)
- Isolated configurations

**No shared state:**
- No static/global variables
- No shared files
- No network ports
- No race conditions

**Result:** Safe for nextest parallel execution ✅

---

## Mock Backend Strategy

### ICMP Pinging (PingBackend)

**Already implemented:**
```rust
pub trait PingBackend: Send + Sync {
    fn ping(&self, target: &str, sequence: u32, timeout_ms: u64)
        -> BoxFuture<'_, Option<u32>>;
}

pub struct MockBackend {
    pub next_rtt_us: Option<u32>,
}
```

**Usage in test:**
```rust
let mock_ping = Arc::new(MockBackend::new(Some(1500)));  // 1.5ms RTT
let pinger = PingerBuilder::new()
    .backend(mock_ping)
    .targets(test_targets)
    .start()?;
```

**Result:** No real ICMP, deterministic RTTs ✅

### File System (IntentConfig Persistence)

**Current approach:** Database role requires file path

**Test approach:**
```rust
let temp_dir = tempfile::tempdir()?;
let config_path = temp_dir.path().join("intent.ron");

let role = IntentConfigRole::Database {
    config_file_path: config_path,
};
```

**Result:** Real file I/O to temp directory ✅
- Validates persistence logic
- Temp dir auto-cleaned
- Each test isolated

**Future improvement:** Add `ConfigPersistence` trait for in-memory testing.

### Network Transport (TCP/TLS)

**Already implemented:**
```rust
let (db_transport, collector_transport) = create_mock_pair("e2e");

db_cm_addr.send(HandleTransport {
    transport: Box::new(db_transport),
    config: HelloConfig::default(),
}).await?;
```

**Result:** No TCP/TLS, in-memory channels ✅

---

## Implementation Phases

### Phase 1: Config Foundation (Est: 2-3 hours)
**Goal:** Enable test configs without TLS cert files

**Tasks:**
1. Change `CollectorConfig::tls` to `Option<TlsConfig>`
2. Change `DatabaseConfig::tls` to `Option<TlsConfig>`
3. Update validation logic (skip TLS checks when None)
4. Update `CollectorService::run()` to handle None (use plain TCP)
5. Update `DatabaseService::run()` to handle None (use plain TCP)
6. Add `CollectorConfig::for_testing()` helper
7. Add `DatabaseConfig::for_testing()` helper
8. Add `ComponentConfig::fast_timing()` helper
9. Update existing tests to compile

**Verification:**
- Existing tests still pass
- Can create config without cert files
- Plain TCP mode works in manual testing

### Phase 2: Heartbeat Precision (Est: 2-3 hours)
**Goal:** Change heartbeat timing from seconds to milliseconds

**Tasks:**
1. Change `ComponentConfig::heartbeat_interval_secs` → `heartbeat_interval_ms`
2. Change `CStateRole::heartbeat_interval_secs` → `heartbeat_interval_ms`
3. Update all usages in collector service
4. Update all usages in CStateActor
5. Update config files (*.ron)
6. Update config examples
7. Update documentation
8. Update validation logic

**Verification:**
- Existing tests still pass
- Heartbeat timing works correctly
- Config files parse correctly

### Phase 3: Time Handling Fix (Est: 1-2 hours)
**Goal:** Fix std::time::Instant usage for time mocking compatibility

**Tasks:**
1. Change `zzcollector-state/src/state.rs`:
   - `use std::time::Instant` → `use tokio::time::Instant`
   - `start_time: Instant` → `start_instant: tokio::time::Instant`
   - `Instant::now()` → `tokio::time::Instant::now()`
2. Update `uptime_secs()` calculation
3. Update any other `elapsed()` calls
4. Update tests that reference `start_time`

**Verification:**
- Existing tests still pass
- No compiler warnings
- Works with `tokio::time::pause()` (add quick test)

### Phase 4: Expose Service APIs (Est: 1-2 hours)
**Goal:** Make component creation accessible for testing

**Tasks:**
1. `CollectorService`:
   - Make `start_all_components()` public
   - Make `StartedComponents` struct public
   - Add doc comments
2. `DatabaseService`:
   - Make `start_all_components()` public
   - Make `StartedComponents` struct public
   - Add doc comments
3. Update module visibility if needed
4. Add usage examples in doc comments

**Verification:**
- Can call from external test
- All actor addresses accessible
- No compiler errors

### Phase 5: Test Infrastructure (Est: 2-3 hours)
**Goal:** Create reusable test utilities

**Tasks:**
1. Create `tests/common/mod.rs`
2. Create `tests/common/test_utils.rs`
3. Implement `init_test_tracing()`
4. Implement `advance_time_and_yield()`
5. Implement config creation helpers
6. Add verification helper functions
7. Document all utilities

**Verification:**
- Utilities work in simple test
- Tracing output visible
- Time advancement works

### Phase 6: E2E Test Implementation (Est: 6-8 hours)
**Goal:** Build comprehensive integration test

**Tasks:**
1. Create `tests/e2e_full_protocol_test.rs`
2. Implement test setup (time, tracing, configs)
3. Implement component creation phase
4. Implement transport wiring
5. Implement HELLO verification
6. Implement config distribution test
7. Implement pinger reaction test
8. Implement ping data flow test
9. Implement config update test
10. Implement collector state test
11. Add comprehensive assertions
12. Add detailed logging throughout

**Verification:**
- Test passes on first run (or debug until it does)
- All assertions validate correctly
- Logs provide clear debugging info
- Test runs in <1 second
- Test passes consistently (run 10x)

### Phase 7: Documentation & Refinement (Est: 2-3 hours)
**Goal:** Document everything and polish

**Tasks:**
1. Update E2E_TEST_PLAN.md with actual implementation
2. Document test failure debugging procedures
3. Add comments to test code
4. Create test execution guide
5. Update RUNBOOK.md with test instructions
6. Review all changes for consistency
7. Run full test suite (unit + integration + E2E)

**Verification:**
- All tests pass
- Documentation complete
- Someone else could understand the test

**Total Estimated Time:** 16-24 hours

---

## Risk Assessment

### High Risk Items

**1. Time mocking might not work perfectly**
- **Mitigation:** Test with simple case first
- **Backup plan:** Add artificial delays where needed

**2. Race conditions in actor startup**
- **Mitigation:** Add small delays after component creation
- **Backup plan:** Retry logic or explicit ready checks

**3. Mock transport might have subtle bugs**
- **Mitigation:** Already used in other tests, proven
- **Backup plan:** Add transport verification step

### Medium Risk Items

**4. Config validation might break existing deployments**
- **Mitigation:** Make TLS optional, don't remove it
- **Backup plan:** Add migration guide

**5. Test might be flaky**
- **Mitigation:** Deterministic timing, no real I/O
- **Backup plan:** Add retry logic or better synchronization

### Low Risk Items

**6. Breaking changes to config format**
- **Mitigation:** Backward compatible (Option<T>)
- **Backup plan:** Keep old configs working

**7. Test too slow**
- **Mitigation:** Time mocking ensures speed
- **Backup plan:** Split into smaller tests if needed

---

## Success Criteria

### Must Have (P0)

- ✅ Test runs in single process (no separate binaries)
- ✅ Test uses real service code (not test-only alternatives)
- ✅ Test completes in <1 second
- ✅ Test validates full protocol flow (HELLO → config → ping → storage)
- ✅ Test uses mock transport (no TCP/TLS)
- ✅ Test uses mock ping backend (no ICMP)
- ✅ Test passes consistently (0% flakiness)
- ✅ Test logs provide clear debugging info

### Should Have (P1)

- ✅ Test validates config update propagation
- ✅ Test validates pinger reaction to config changes
- ✅ Test validates collector state tracking
- ✅ Test runs in parallel with other tests (nextest safe)
- ✅ Changes improve production code (not just for tests)
- ✅ Minimal use of `#[cfg(test)]`

### Nice to Have (P2)

- ⭐ Test validates permission checks
- ⭐ Test validates error handling
- ⭐ Test validates edge cases (empty config, invalid targets)
- ⭐ Multiple E2E tests for different scenarios
- ⭐ Performance benchmarks

---

## Future Enhancements

### Short Term (Next Sprint)

**1. Add permission check tests:**
- Collector tries to access wrong room
- Verify session bridge enforces ACLs
- Test unauthorized config changes

**2. Add error handling tests:**
- Simulate network failures
- Simulate serialization errors
- Verify graceful degradation

**3. Add CLI integration:**
- Create `zzping-cli` binary
- Test admin config changes
- Test query subscriptions

### Medium Term (Next Month)

**4. Add persistence trait abstraction:**
- `ConfigPersistence` trait for IntentConfig
- `ResultsPersistence` trait for MemDB
- Enables pure in-memory testing

**5. Add stress testing:**
- High-frequency pings
- Large config files
- Many collectors

**6. Add GUI integration:**
- Web interface for admin
- Real-time dashboards
- Alert configuration

### Long Term (Future)

**7. Add chaos testing:**
- Random component failures
- Network partition simulation
- Clock skew simulation

**8. Add multi-collector tests:**
- Multiple collectors connecting
- Config broadcast validation
- Collector conflict resolution

**9. Add upgrade testing:**
- Rolling updates
- Protocol version compatibility
- State migration

---

## Appendix A: Key Code Locations

### Configuration
- `src/apps/zzping-collector/src/config.rs` - Collector config
- `src/apps/zzping-database/src/config.rs` - Database config
- `config/collector.ron` - Example collector config
- `config/database.ron` - Example database config

### Services
- `src/apps/zzping-collector/src/service.rs` - Collector service
- `src/apps/zzping-database/src/service.rs` - Database service
- `src/apps/zzping-collector/src/network.rs` - Collector network layer
- `src/apps/zzping-database/src/network.rs` - Database network layer

### Components
- `src/components/zzpinger/` - Ping component
- `src/components/zzintent-config/` - Config component
- `src/components/zzmem-db/` - Memory DB component
- `src/components/zzcollector-state/` - Collector state component

### Network Layer
- `src/net/zznet-api/` - Transport abstraction
- `src/net/zznet-api/src/mock.rs` - Mock transport
- `src/net/zznet-transport-tcp/` - TCP transport
- `src/net/zznet-hello/` - HELLO protocol
- `src/net/zznet-session/` - Session management

### Tests
- `src/apps/zzping-database/tests/` - Existing integration tests
- `tests/` - Workspace-level integration tests (where E2E goes)

---

## Appendix B: Related Documents

- `docs/E2E_TEST_PLAN.md` - Original test plan (this investigation supersedes it)
- `docs/design/ZZPing_Network_Layer_Vision.md` - Network architecture vision
- `docs/design/ZZPing_Network_Layer_Implementation_Plan_V2_MockFirst.md` - Mock-first approach
- `docs/TWO_LAYER_AUTHORIZATION_ARCHITECTURE.md` - Auth architecture
- `RUNBOOK.md` - Operations runbook
- `CONTRIBUTING.md` - Contribution guidelines

---

## Appendix C: Glossary

- **E2E Test**: End-to-end test validating full system integration
- **Mock Transport**: In-memory transport implementation for testing
- **Time Mocking**: Tokio feature for controlling async time in tests
- **Macro-Test**: Large comprehensive test covering multiple scenarios
- **Actor**: Actix actor (message-passing concurrency primitive)
- **Component**: Self-contained business logic unit (zzpinger, zzintent-config, etc.)
- **Service**: Top-level orchestrator (CollectorService, DatabaseService)
- **Role**: Configuration variant (Database role vs Collector role)
- **HELLO Protocol**: Initial handshake and room negotiation protocol
- **Room**: Named communication channel (intent-config, memdb, query, etc.)
- **Session**: Established connection after HELLO handshake
- **AuthRole**: Connection-level role (Collector, Database, ClientAdmin)
- **Permission**: Component-specific access control

---

**End of Report**
