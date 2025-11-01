# ZZNet-Demo Critical Findings

**Date**: November 1, 2025
**Status**: 🔴 **CRITICAL ISSUE IDENTIFIED**

---

## Critical Finding: Builder API Not Tested

### The Problem

The `zznet-demo` integration test **does not use `zznet-builder` at all**.

The `AppStack` test harness in `tests/full_stack_integration_test.rs` manually wires all actors together:

```rust
impl AppStack {
    pub async fn new(include_component_b: bool) -> Self {
        // Manual wiring - BYPASSES THE BUILDER API:
        let router = RouterActor::new(vec![RoomId::from("room-a")]).start();
        let connection_manager = ConnectionManager::new(router.clone(), allowed_roles.clone()).start();
        let component_a = ComponentAActor::new().start();
        // ... more manual wiring
    }
}
```

### Why This Is Critical

1. **Zero coverage of builder API**: The `AppBuilder`, `ZZNetService`, and `ZZNetConfig` abstractions are completely untested by integration tests

2. **Tests wrong code path**: Production apps use the builder API (see `zzping-database/src/main.rs`), not manual wiring

3. **Violates stated goals**: The original vision was to achieve "100% code coverage in zznet" via integration tests. This is impossible while bypassing major API surfaces.

4. **False confidence**: Tests pass, but the primary developer-facing API could be completely broken

### Real Application Pattern

Production apps like `zzping-collector` and `zzping-database` use this pattern:

```rust
fn main() -> Result<()> {
    AppBuilder::new("MyApp", "1.0.0")
        .with_default_config("config.ron")
        .run_service::<MyService>()  // ← This is never tested!
}

impl ZZNetService for MyService {
    fn new(config: Self::Config) -> Result<Self> {
        // Actual production wiring ← This is never tested!
    }
}
```

**The integration test must test this pattern, not bypass it.**

---

## Impact Assessment

### Untested Code

The following critical APIs have **0% integration test coverage**:

- `AppBuilder::build_and_run()`
- `AppBuilder::run_service()`
- `ZZNetService::new()` trait method
- `ZZNetService::run()` trait method
- `ZZNetConfig::validate()` trait method
- `ZZNetConfig::log_startup_info()` trait method
- Builder-based component wiring patterns

### Consequences

1. **Builder could be broken**: We could ship a broken builder API and not know until production
2. **Documentation mismatch**: Examples and docs show builder usage, but it's not validated
3. **Coverage gap**: `zznet-builder` crate has ~40-60% coverage instead of >95%
4. **Maintenance risk**: Manual wiring in tests must stay in sync with apps (DRY violation)

---

## Solution

See **[05_builder_integration_plan.md](./05_builder_integration_plan.md)** for the complete implementation plan.

### Summary of Fix

**Two-Tier Testing Strategy**:

1. **Tier 1 (PRIMARY)**: Builder-based integration tests
   - Use `ZZNetService` and `ZZNetConfig` traits
   - Wire actors through builder patterns (same as production apps)
   - Validate builder API works end-to-end
   - File: `tests/builder_integration_test.rs`

2. **Tier 2 (SECONDARY)**: Framework internals tests
   - Keep existing manual wiring tests for quick protocol checks
   - Rename to clarify purpose: `tests/framework_internals_test.rs`
   - Use `AppStack` for fast actor-level validation

### Implementation Phases

1. **Phase 1**: Create `DemoAppService` implementing `ZZNetService`
2. **Phase 2**: Port existing tests to use builder API
3. **Phase 3**: Add builder-specific test cases
4. **Phase 4**: Coverage analysis and gap closure
5. **Phase 5**: Reorganize and document test suite

---

## Success Criteria

This issue is resolved when:

- ✅ `zznet-builder` has >95% code coverage from integration tests
- ✅ All `ZZNetService` and `ZZNetConfig` trait methods are tested
- ✅ `DemoAppService::new()` matches production app wiring patterns
- ✅ Tests validate what production applications actually do
- ✅ Coverage reports show no critical gaps in builder API

---

## Priority

🔴 **CRITICAL** - This must be fixed before the next release.

The builder API is the primary developer-facing surface for zznet applications. It cannot be left untested.

---

## References

- **Detailed Plan**: [05_builder_integration_plan.md](./05_builder_integration_plan.md)
- **Original Vision**: [01_vision.md](./01_vision.md)
- **Implementation Plan**: [02_plan.md](./02_plan.md)
- **Builder Research**: [../zznet-builder-research/01_research_report.md](../zznet-builder-research/01_research_report.md)
- **Current Test Code**: `src/apps/zznet-demo/tests/full_stack_integration_test.rs`
- **Production Example**: `src/apps/zzping-database/src/service.rs`
