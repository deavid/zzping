# ZZNet-Demo Builder Integration Plan

**Date**: November 1, 2025
**Status**: Planning
**Priority**: CRITICAL

---

## Executive Summary

**Critical Finding**: The current `zznet-demo` integration test completely bypasses the `zznet-builder` high-level API. The `AppStack` test harness manually wires all actors together (`RouterActor`, `ConnectionManager`, etc.) using the low-level actor APIs. While this proves the underlying framework components work when manually assembled, it creates a **major gap** in test coverage:

- **What IS tested**: Low-level actor wiring, message passing, protocol mechanics
- **What is NOT tested**: The `zznet-builder` API itself - the intended developer-facing entry point

This means that the builder could be completely broken, or have serious usability issues, and the integration test would still pass. Real applications use the builder API, not manual wiring.

**Impact**:
- The builder's `AppBuilder::build_and_run()` and `AppBuilder::run_service()` methods are untested
- The `ZZNetService` and `ZZNetConfig` traits are untested in an integration context
- A production app following builder patterns could fail in ways the test doesn't catch
- The stated goal of "100% code coverage via integration test" is impossible without testing the builder

---

## Current State Analysis

### What AppStack Does Today

The `AppStack` struct in `tests/full_stack_integration_test.rs` manually constructs:

```rust
pub struct AppStack {
    connection_manager: Addr<ConnectionManager>,
    component_a: Addr<ComponentAActor>,
    component_b: Option<Addr<ComponentBActor>>,
    allowed_roles: HashSet<Role>,
}

impl AppStack {
    pub async fn new(include_component_b: bool) -> Self {
        // Manual wiring:
        let router = RouterActor::new(vec![RoomId::from("room-a")]).start();
        let connection_manager = ConnectionManager::new(router.clone(), allowed_roles.clone()).start();
        let component_a = ComponentAActor::new().start();
        let network_manager = ComponentANetworkManager::new(component_a.clone(), router.clone()).start();
        // ... manual subscription and wiring logic
    }
}
```

**Problems**:
1. This code path never exercises `AppBuilder::build_and_run()` or `AppBuilder::run_service()`
2. The `ZZNetService::new()` and `ZZNetService::run()` trait methods are never called
3. The `ZZNetConfig::validate()` and `ZZNetConfig::log_startup_info()` methods are never called
4. Configuration loading via `load_ron_config()` is never tested
5. The builder's CLI argument handling (`StandardCliArgs`) is never tested
6. The builder's logging initialization is never tested

### What Real Applications Do

Real production apps like `zzping-collector` and `zzping-database` use this pattern:

```rust
fn main() -> Result<()> {
    AppBuilder::new("MyApp", "1.0.0")
        .with_default_config("config.ron")
        .run_service::<MyService>()
}
```

Where `MyService` implements `ZZNetService` and does all the wiring internally.

**The integration test should validate this pattern, not bypass it.**

---

## Architectural Analysis

### Why Manual Wiring Is Wrong for Integration Tests

The original design documents (`docs/zznet-demo/01_vision.md`, `02_plan.md`) explicitly state:

> "The objective: I can later run this integration test alone and gather coverage data from just running it alone without running any other test - anything that does not run is either code that's not needed or something that's missing wiring properly. We will aim for 100% code coverage in zznet in this approach."

**Manual wiring violates this goal** because:
1. It skips the builder layer entirely
2. Real apps don't manually instantiate `RouterActor`, `ConnectionManager`, etc.
3. We're testing a code path that no production code actually uses

### The DRY Violation

From `docs/zznet-builder-research/01_research_report.md`, the builder was created specifically to eliminate this duplication:

> "Both `zzping-collector` and `zzping-database` follow an almost identical structure... 60% duplicated logic... The proposed builder would reduce application code by an estimated 40-60%."

By manually wiring in tests, we're:
1. Duplicating the exact wiring logic the builder was designed to eliminate
2. Creating maintenance burden (test wiring must stay in sync with apps)
3. Missing test coverage on the abstraction layer that real apps depend on

---

## Proposed Solution: Two-Tier Testing Strategy

### Tier 1: Builder-Based Integration Tests (PRIMARY)

Create **new** integration tests that use the `zznet-builder` API as the primary entry point.

#### Approach

```rust
// NEW: tests/builder_integration_test.rs

/// Configuration for the demo application
#[derive(Debug, Deserialize, Serialize)]
struct DemoAppConfig {
    our_role: String,
    offered_rooms: Vec<String>,
    allowed_roles: Vec<String>,
}

impl ZZNetConfig for DemoAppConfig {
    fn validate(&self) -> Result<()> {
        if self.our_role.is_empty() {
            anyhow::bail!("our_role cannot be empty");
        }
        Ok(())
    }

    fn log_startup_info(&self) {
        tracing::info!("Role: {}, Rooms: {:?}", self.our_role, self.offered_rooms);
    }
}

/// Demo service that implements ZZNetService
struct DemoAppService {
    router: Addr<RouterActor>,
    connection_manager: Addr<ConnectionManager>,
    component_a: Addr<ComponentAActor>,
    component_b: Option<Addr<ComponentBActor>>,
}

#[async_trait]
impl ZZNetService for DemoAppService {
    type Config = DemoAppConfig;
    type Error = anyhow::Error;

    fn new(config: Self::Config) -> Result<Self> {
        // This is where the REAL wiring happens - same as production apps
        let router = RouterActor::new(
            config.offered_rooms.iter().map(|r| RoomId::from(r.as_str())).collect()
        ).start();

        let allowed_roles: HashSet<Role> = config.allowed_roles
            .iter()
            .map(|r| Role::new(r))
            .collect();

        let connection_manager = ConnectionManager::new(
            router.clone(),
            allowed_roles
        ).start();

        let component_a = ComponentAActor::new().start();
        let network_manager = ComponentANetworkManager::new(
            component_a.clone(),
            router.clone()
        ).start();

        component_a.do_send(SetNetworkManager { network_manager });

        Ok(Self {
            router,
            connection_manager,
            component_a,
            component_b: None,
        })
    }

    async fn run(self) -> Result<(), Self::Error> {
        tracing::info!("DemoAppService is running");
        // Keep the service alive
        Ok(())
    }
}

#[actix::test]
async fn test_builder_two_stack_communication() {
    // Create two temporary config files
    let config_a = DemoAppConfig {
        our_role: "collector".to_string(),
        offered_rooms: vec!["room-a".to_string()],
        allowed_roles: vec!["database".to_string()],
    };

    let config_b = DemoAppConfig {
        our_role: "database".to_string(),
        offered_rooms: vec!["room-a".to_string()],
        allowed_roles: vec!["collector".to_string()],
    };

    // Spawn two builder-based services
    let (tx_a, rx_a) = tokio::sync::oneshot::channel();
    let (tx_b, rx_b) = tokio::sync::oneshot::channel();

    let handle_a = tokio::spawn(async move {
        // This exercises the REAL builder API
        let service = DemoAppService::new(config_a).unwrap();
        tx_a.send(service.component_a.clone()).unwrap();
        service.run().await.unwrap();
    });

    let handle_b = tokio::spawn(async move {
        let service = DemoAppService::new(config_b).unwrap();
        tx_b.send(service.component_a.clone()).unwrap();
        service.run().await.unwrap();
    });

    // Get component addresses from running services
    let comp_a_addr = rx_a.await.unwrap();
    let comp_b_addr = rx_b.await.unwrap();

    // Connect the services using mock transport
    // (Details of connection setup similar to current AppStack::connect_to)

    // Run test scenarios
    comp_a_addr.do_send(SendPing { data: "test".to_string() });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let counter = comp_b_addr.send(GetCounter).await.unwrap();
    assert_eq!(counter, 1);

    // Cleanup
    handle_a.abort();
    handle_b.abort();
}
```

**Benefits**:
1. Tests the actual `ZZNetService` trait implementation
2. Exercises configuration validation (`ZZNetConfig::validate()`)
3. Uses the same wiring logic as production applications
4. Tests the service lifecycle (`new()` → `run()`)
5. Validates that the builder patterns work end-to-end

### Tier 2: Low-Level Manual Wiring Tests (SECONDARY)

Keep the existing `AppStack` tests as **supplementary unit/integration tests** for framework internals.

**Purpose**: Test actor-level mechanics without builder overhead
**Scope**: Quick smoke tests for protocol correctness
**Status**: Rename/reorganize as "framework internals tests"

---

## Implementation Plan

### Phase 1: Create Builder-Based Test Infrastructure

**Goal**: Implement the `ZZNetService` and `ZZNetConfig` traits for the demo app components.

#### Tasks:

1. **Create `DemoAppConfig` struct**
   - File: `src/apps/zznet-demo/src/config.rs`
   - Fields: `our_role`, `offered_rooms`, `allowed_roles`, `include_component_b`
   - Implement `ZZNetConfig` trait with proper validation
   - Add `Deserialize` and `Serialize` derives

2. **Create `DemoAppService` struct**
   - File: `src/apps/zznet-demo/src/service.rs`
   - Fields: All actor addresses needed for the test
   - Implement `ZZNetService` trait:
     - `new(config)`: Wire all actors (RouterActor, ConnectionManager, Components)
     - `run()`: Keep service alive, return handles to test code
   - Provide test-friendly API to get component addresses for assertions

3. **Add Builder Dependency**
   - Update `src/apps/zznet-demo/Cargo.toml`
   - Add: `zznet-builder = { path = "../../net/zznet-builder" }`

4. **Create Mock Transport Helpers**
   - File: `src/apps/zznet-demo/src/test_harness.rs`
   - Helper to connect two `DemoAppService` instances via `create_mock_pair()`
   - Function to spawn two services and return their component addresses

**Success Criteria**:
- `DemoAppService::new()` successfully wires all actors
- `DemoAppService::run()` keeps the service running
- Test code can obtain addresses to send messages and make assertions
- Configuration validation works correctly

---

### Phase 2: Port Existing Tests to Builder API

**Goal**: Rewrite the four existing integration tests to use `DemoAppService` instead of `AppStack`.

#### Tests to Port:

1. **`test_ping_pong_between_component_a()`**
   - Before: Uses `AppStack::new(false)`
   - After: Uses `DemoAppService::new(config)` with `include_component_b: false`
   - Validates: Builder-based wiring works for basic A↔A communication

2. **`test_component_a_publishes_to_component_b()`**
   - Before: Uses `AppStack::new(true)` for ComponentB
   - After: Uses `DemoAppService::new(config)` with `include_component_b: true`
   - Validates: Builder properly wires pub/sub between components

3. **`test_component_b_sends_message_via_component_a()`**
   - Before: Manual wiring
   - After: Builder-based service with B→A→network flow
   - Validates: Complex message routing through builder-wired components

4. **`test_unauthorized_connection_is_rejected()`**
   - Before: `AppStack::new_with_roles()`
   - After: Two `DemoAppService` instances with mismatched `allowed_roles` configs
   - Validates: Builder-based services correctly enforce authorization

#### Implementation Pattern:

```rust
#[actix::test]
async fn test_builder_ping_pong_between_component_a() {
    // Create two services using builder patterns
    let (service_a, comp_a_addr_a) = spawn_demo_service(DemoAppConfig {
        our_role: "collector".to_string(),
        offered_rooms: vec!["room-a".to_string()],
        allowed_roles: vec!["database".to_string()],
        include_component_b: false,
    }).await;

    let (service_b, comp_a_addr_b) = spawn_demo_service(DemoAppConfig {
        our_role: "database".to_string(),
        offered_rooms: vec!["room-a".to_string()],
        allowed_roles: vec!["collector".to_string()],
        include_component_b: false,
    }).await;

    // Connect services
    connect_services(&service_a, &service_b).await;

    // Run test
    comp_a_addr_a.do_send(SendPing { data: "test".to_string() });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let counter = comp_a_addr_b.send(GetCounter).await.unwrap();
    assert_eq!(counter, 1);
}
```

**Success Criteria**:
- All four existing tests pass using builder-based services
- Test code remains readable and maintainable
- No functionality is lost in the transition

---

### Phase 3: Add Builder-Specific Test Cases

**Goal**: Add new tests that specifically validate builder features that can't be tested with manual wiring.

#### New Test Cases:

1. **`test_builder_config_validation_rejects_invalid()`**
   - Create `DemoAppConfig` with invalid data (empty `our_role`, invalid `allowed_roles`)
   - Call `config.validate()`
   - Assert that validation fails with expected error messages
   - **Covers**: `ZZNetConfig::validate()` implementation

2. **`test_builder_config_validation_accepts_valid()`**
   - Create valid `DemoAppConfig`
   - Call `config.validate()`
   - Assert success
   - **Covers**: `ZZNetConfig::validate()` happy path

3. **`test_builder_service_construction_from_config()`**
   - Create valid config
   - Call `DemoAppService::new(config)`
   - Assert that all actors are properly started (non-null addresses)
   - Assert that router has correct offered rooms
   - **Covers**: `ZZNetService::new()` implementation

4. **`test_builder_service_run_lifecycle()`**
   - Create service
   - Spawn task calling `service.run()`
   - Assert that service keeps running (doesn't exit immediately)
   - Send shutdown signal
   - Assert service stops cleanly
   - **Covers**: `ZZNetService::run()` lifecycle

5. **`test_builder_config_serialization_roundtrip()`**
   - Create `DemoAppConfig`
   - Serialize to RON using `ron::to_string()`
   - Deserialize back using `ron::from_str()`
   - Assert configs are equal
   - **Covers**: Config file loading simulation

6. **`test_builder_multiple_services_isolation()`**
   - Create 3+ `DemoAppService` instances
   - Verify each has independent actor addresses
   - Verify no cross-contamination of state
   - **Covers**: Service isolation guarantees

**Success Criteria**:
- All new tests pass
- Builder-specific code paths are exercised
- Coverage reports show builder trait methods are tested

---

### Phase 4: Coverage Analysis and Gap Closure

**Goal**: Run coverage analysis to identify any remaining untested code in `zznet-builder`.

#### Tasks:

1. **Generate Coverage Report**
   - Run: `cargo llvm-cov --html --open --package zznet-demo`
   - Examine coverage for `zznet-builder` crate specifically
   - Identify any uncovered lines in `builder.rs`, `traits.rs`

2. **Add Missing Tests**
   - For each uncovered code path, determine if it's:
     - **Critical**: Add integration test to cover it
     - **Error handling**: Add negative test case
     - **Dead code**: Mark for removal/refactoring

3. **Document Coverage Results**
   - File: `docs/zznet-demo/06_coverage_report.md`
   - Report coverage percentages for each zznet-* crate
   - List any intentional gaps (e.g., error paths that can't be triggered in tests)
   - Document strategy for reaching 100% coverage

4. **Verify Production Patterns**
   - Compare `DemoAppService::new()` wiring with `zzping-database/src/service.rs`
   - Ensure patterns match exactly
   - Document any intentional differences

**Success Criteria**:
- `zznet-builder` has >95% code coverage from integration tests
- All critical builder APIs are exercised
- Coverage report documents any remaining gaps with justification

---

### Phase 5: Reorganize Test Suite

**Goal**: Clearly separate builder-based integration tests from low-level framework tests.

#### Structure:

```
src/apps/zznet-demo/
├── src/
│   ├── lib.rs                          # Public API
│   ├── config.rs                       # DemoAppConfig (ZZNetConfig)
│   ├── service.rs                      # DemoAppService (ZZNetService)
│   ├── test_harness.rs                 # Helpers for spawning/connecting services
│   ├── component_a.rs                  # (existing)
│   ├── component_b.rs                  # (existing)
│   └── messages.rs                     # (existing)
├── tests/
│   ├── builder_integration_test.rs     # NEW: Builder-based tests (PRIMARY)
│   ├── framework_internals_test.rs     # RENAMED: Low-level tests (SECONDARY)
│   └── coverage_validation_test.rs     # NEW: Coverage-specific tests
└── README.md                           # Updated documentation
```

#### Changes:

1. **Rename `full_stack_integration_test.rs` → `framework_internals_test.rs`**
   - Update module documentation to clarify purpose
   - Mark as "low-level framework validation tests"
   - Keep existing `AppStack` struct for quick protocol tests

2. **Create `builder_integration_test.rs`**
   - Contains all builder-based tests from Phases 2-3
   - Primary test file for coverage goals
   - Module doc: "These tests validate the zznet-builder API as used by production applications"

3. **Create `coverage_validation_test.rs`**
   - Meta-tests that check coverage percentages
   - Fail if critical code paths are uncovered
   - Document coverage expectations

4. **Update `README.md`**
   - Explain the purpose of zznet-demo
   - Document the two-tier testing strategy
   - Provide instructions for running coverage reports
   - Show example of how to use `DemoAppService` as a template for new apps

**Success Criteria**:
- Test organization is clear and self-documenting
- Developers understand which tests to run for which purposes
- README provides value as documentation for the builder patterns

---

## Expected Outcomes

### Coverage Improvements

**Before** (manual wiring):
- `zznet-builder`: ~40-60% coverage (only utility functions covered by app unit tests)
- `ZZNetService` trait: 0% coverage
- `ZZNetConfig` trait: 0% coverage
- `AppBuilder::build_and_run()`: 0% coverage
- `AppBuilder::run_service()`: 0% coverage

**After** (builder-based tests):
- `zznet-builder`: >95% coverage
- `ZZNetService` trait: 100% coverage
- `ZZNetConfig` trait: 100% coverage
- `AppBuilder::build_and_run()`: Partially covered (full coverage requires CLI test)
- `AppBuilder::run_service()`: 100% coverage

### Architectural Validation

By testing through the builder:
1. We validate that the patterns documented in `zznet-builder-research` actually work
2. We ensure real applications can successfully use the builder API
3. We catch API usability issues before they reach production
4. We provide a living example of how to structure a zznet application

### Developer Experience

The updated `zznet-demo` becomes:
1. **Reference implementation** - developers can copy `DemoAppService` as a template
2. **Confidence builder** - comprehensive tests prove the framework works end-to-end
3. **Documentation** - code serves as executable documentation of best practices
4. **Debugging tool** - isolated demo app for reproducing issues

---

## Testing Strategy Summary

| Test Type | File | Purpose | Uses Builder? |
|-----------|------|---------|---------------|
| **Builder Integration** | `builder_integration_test.rs` | Validate builder API as used by production apps | ✅ Yes (PRIMARY) |
| **Framework Internals** | `framework_internals_test.rs` | Quick smoke tests for protocol mechanics | ❌ No (manual wiring) |
| **Coverage Validation** | `coverage_validation_test.rs` | Meta-tests for coverage goals | N/A |

---

## Risk Analysis

### Risk: Builder API Overhead

**Concern**: Using the builder might add complexity to tests.

**Mitigation**:
- Keep test harness helpers (`spawn_demo_service`, `connect_services`) simple
- Builder actually reduces complexity by eliminating manual wiring duplication
- Tests become MORE readable by using high-level APIs

### Risk: Test Performance

**Concern**: Builder-based tests might be slower.

**Mitigation**:
- Integration tests are already async and relatively slow
- Builder overhead is negligible (just trait method calls)
- Keep fast framework tests separate in `framework_internals_test.rs`

### Risk: Maintenance Burden

**Concern**: Two test suites to maintain.

**Mitigation**:
- `framework_internals_test.rs` becomes stable (rarely changes)
- `builder_integration_test.rs` is the primary focus for new features
- Clear documentation prevents confusion

---

## Success Metrics

This plan succeeds when:

1. ✅ **Coverage Goal Met**: `zznet-builder` has >95% code coverage from integration tests
2. ✅ **Production Patterns Tested**: `DemoAppService::new()` matches patterns in `zzping-database`
3. ✅ **Builder Traits Exercised**: `ZZNetService` and `ZZNetConfig` are fully tested
4. ✅ **Tests Are Maintainable**: Clear separation between builder and framework tests
5. ✅ **Documentation Value**: `zznet-demo` serves as reference implementation
6. ✅ **Gap Closed**: The critical finding is resolved - builder API is validated

---

## Next Steps

1. **Review this plan** with the team
2. **Execute Phase 1**: Implement `DemoAppConfig` and `DemoAppService`
3. **Execute Phase 2**: Port existing tests to builder-based approach
4. **Run coverage analysis** after Phase 2 to validate improvement
5. **Continue through Phases 3-5** until success metrics are met
6. **Document learnings** in final coverage report

---

## Conclusion

The current test suite's manual wiring approach is a **critical architectural oversight**. By bypassing the builder API, we're testing a code path that production applications don't use, leaving the actual developer-facing API untested and unvalidated.

This plan addresses the gap by:
1. Creating builder-based integration tests that mirror production patterns
2. Maintaining low-level tests for framework internals
3. Achieving the stated goal of 100% coverage via integration testing
4. Providing a reference implementation for future zznet applications

**The test must test what the code does, not what we wish it did.** Real apps use the builder. The test must use the builder.
