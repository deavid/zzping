# ZZNet-Builder: Complete DRY Elimination Plan

**Date**: October 31, 2025
**Status**: 🔴 CURRENT IMPLEMENTATION IS INCOMPLETE - Planning phase for REAL 1.0

## Problem Statement

The current `zznet-builder` implementation (v0.5 at best) **only eliminates setup boilerplate** but does NOT eliminate the **service lifecycle pattern duplication**. Both apps still have ~40 lines of duplicated code in their main.rs files.

## Current Duplication Analysis

### What AppBuilder Currently Handles (✅ Done)
1. ✅ Crypto provider installation
2. ✅ CLI argument parsing
3. ✅ Logging initialization
4. ✅ Startup banner logging
5. ✅ Configuration file loading
6. ✅ Actix runtime setup

### What's STILL Duplicated (❌ TODO)

#### In main.rs (Both Apps - Nearly Identical):

```rust
// COLLECTOR                              // DATABASE
config.validate()                    ←→   config.validate()
.context("Configuration validation   ←→   .context("Configuration validation
failed")?;                           ←→   failed")?;

tracing::info!("Collector ID: ...");  ≈   tracing::info!("Binding to...");
tracing::info!("Database: ...");      ≈   (app-specific logging)

let service = CollectorService::new( ←→   let service = DatabaseService::new(
    config)                          ←→       config)
.context("Failed to create          ←→   .context("Failed to create
    collector service")?;            ←→       database service")?;

service.run().await                  ←→   if let Err(e) = service.run().await {
.context("Collector service          ←→       tracing::error!("...");
    failed")?;                       ←→       return Err(anyhow::anyhow!("..."));
                                     ←→   }

tracing::info!("...started           ←→   tracing::info!("...started
    successfully; entering run-loop");←→      successfully; entering run-loop");

// Block until shutdown signal       ←→   // Block until shutdown signal
let (_tx, rx) = tokio::sync::        ←→   let (_tx, rx) = tokio::sync::
    oneshot::channel::<()>();        ←→       oneshot::channel::<()>();
let _ = rx.await;                    ←→   let _ = rx.await;

tracing::info!("...shutdown          ←→   tracing::info!("...shutdown
    complete");                      ←→       complete");
Ok(())                               ←→   Ok(())
```

**Lines of duplication per app**: ~30-35 lines
**Total duplication**: ~60-70 lines across two apps
**Duplication percentage**: ~80% of main.rs is identical patterns

#### Pattern Analysis:

| Pattern | Collector | Database | Status |
|---------|-----------|----------|--------|
| Config validation | ✓ | ✓ | ❌ Duplicated |
| Config-specific logging | ✓ | ✓ | ❌ Duplicated |
| Service creation | ✓ | ✓ | ❌ Duplicated |
| Service run + error handling | ✓ | ✓ | ❌ Duplicated |
| Success logging | ✓ | ✓ | ❌ Duplicated |
| Shutdown channel creation | ✓ | ✓ | ❌ Duplicated |
| Shutdown waiting | ✓ | ✓ | ❌ Duplicated |
| Shutdown logging | ✓ | ✓ | ❌ Duplicated |

## Service Architecture Analysis

### Current Service Pattern (Both Apps)

```rust
pub struct XxxService {
    config: XxxConfig,
}

impl XxxService {
    pub fn new(config: XxxConfig) -> Result<Self> {
        config.validate()?;  // ← Validation in constructor
        Ok(Self { config })
    }

    pub async fn run(self) -> Result<()> {
        tracing::info!("Service starting");

        // 1. Create builders
        let builders = self.create_builders()?;

        // 2. Start components
        let started = Self::start_components(builders).await?;

        // 3. Network wiring
        let network = self.create_network()?;
        network.run(&started).await?;

        // Returns immediately after starting - does NOT block!
        Ok(())
    }
}
```

**Key Issue**: `run()` returns immediately after starting everything. The main.rs must manually:
1. Call `run()` and check errors
2. Log success
3. Create shutdown channel
4. Wait forever on channel
5. Log shutdown

## Complete DRY Solution Design

### Goal: Reduce main.rs to 3-5 Lines

```rust
fn main() -> Result<()> {
    AppBuilder::new("ZZPing Collector", env!("CARGO_PKG_VERSION"))
        .run_service::<CollectorService, CollectorConfig>()
}
```

OR even simpler:

```rust
fn main() -> Result<()> {
    zznet_builder::run_app::<CollectorService, CollectorConfig>(
        "ZZPing Collector",
        env!("CARGO_PKG_VERSION")
    )
}
```

### Required Traits

#### 1. **Trait: `ZZNetConfig`** (Configuration Protocol)

```rust
pub trait ZZNetConfig: DeserializeOwned + Send + 'static {
    /// Validate configuration after loading
    fn validate(&self) -> anyhow::Result<()>;

    /// Log configuration details for startup
    /// Default implementation does nothing
    fn log_startup_info(&self) {
        tracing::debug!("Configuration loaded (no details logged)");
    }
}
```

#### 2. **Trait: `ZZNetService`** (Service Lifecycle Protocol)

```rust
pub trait ZZNetService: Sized + Send + 'static {
    type Config: ZZNetConfig;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Create a new service from validated config
    fn new(config: Self::Config) -> Result<Self, Self::Error>;

    /// Start the service (should return immediately after starting)
    async fn run(self) -> Result<(), Self::Error>;

    /// Get service name for logging (default uses type name)
    fn service_name() -> &'static str {
        std::any::type_name::<Self>()
    }
}
```

### Implementation Checklist

#### Phase 1: Trait Definitions ✅
- [ ] Create `src/net/zznet-builder/src/traits.rs`
- [ ] Define `ZZNetConfig` trait
- [ ] Define `ZZNetService` trait
- [ ] Add comprehensive documentation
- [ ] Add trait examples in docs

#### Phase 2: AppBuilder Enhancement 🔧
- [ ] Add `run_service<S: ZZNetService>()` method
- [ ] Implement full service lifecycle:
  - [ ] Load config (already done)
  - [ ] Validate config via trait
  - [ ] Log startup via `config.log_startup_info()`
  - [ ] Create service via `S::new(config)`
  - [ ] Run service via `service.run()`
  - [ ] Handle errors with context
  - [ ] Log success: `"{} started successfully"`
  - [ ] Create shutdown signal handler
  - [ ] Wait for shutdown (Ctrl+C / SIGTERM)
  - [ ] Log shutdown: `"{} shutdown complete"`
- [ ] Add `run_service_with_signals<S>()` variant for custom signal handling
- [ ] Update lib.rs with new API examples

#### Phase 3: Implement Traits for Collector 🎯
- [ ] Implement `ZZNetConfig` for `CollectorConfig`:
  - [ ] Already has `validate()` - just add trait bound
  - [ ] Implement `log_startup_info()`:
    ```rust
    fn log_startup_info(&self) {
        tracing::info!("Collector ID: {}", self.collector_id);
        tracing::info!("Database: {}:{}",
            self.database_host, self.database_port);
    }
    ```
- [ ] Implement `ZZNetService` for `CollectorService`:
  - [ ] Already has `new()` - just add trait bound
  - [ ] Already has `run()` - just add trait bound
  - [ ] Add `service_name()`: return `"ZZPing Collector"`
- [ ] Refactor `main.rs` to single line:
  ```rust
  fn main() -> Result<()> {
      AppBuilder::new("ZZPing Collector", env!("CARGO_PKG_VERSION"))
          .run_service::<CollectorService, CollectorConfig>()
  }
  ```

#### Phase 4: Implement Traits for Database 🎯
- [ ] Implement `ZZNetConfig` for `DatabaseConfig`:
  - [ ] Already has `validate()` - just add trait bound
  - [ ] Implement `log_startup_info()`:
    ```rust
    fn log_startup_info(&self) {
        tracing::info!("Binding to {}:{}",
            self.bind_host, self.bind_port);
    }
    ```
- [ ] Implement `ZZNetService` for `DatabaseService`:
  - [ ] Already has `new()` - adjust error type handling
  - [ ] Already has `run()` - adjust error type handling
  - [ ] Add `service_name()`: return `"ZZPing Database"`
- [ ] Refactor `main.rs` to single line:
  ```rust
  fn main() -> Result<()> {
      AppBuilder::new("ZZPing Database", env!("CARGO_PKG_VERSION"))
          .run_service::<DatabaseService, DatabaseConfig>()
  }
  ```

#### Phase 5: Shutdown Signal Handling 🛑
- [ ] Create `src/net/zznet-builder/src/signals.rs`
- [ ] Implement signal handler builder:
  ```rust
  pub struct ShutdownSignals {
      sigterm: tokio::signal::unix::Signal,
      sigint: tokio::signal::unix::Signal,
  }

  impl ShutdownSignals {
      pub fn new() -> Result<Self>;
      pub async fn wait(&mut self);
  }
  ```
- [ ] Integrate into `run_service()`:
  - [ ] Create shutdown signals
  - [ ] Wait on signals
  - [ ] Log which signal was received
- [ ] Add Windows support (Ctrl+C only)
- [ ] Add tests for signal handling (mock signals)

#### Phase 6: Error Handling Refinement 🚨
- [ ] Review error propagation in `run_service()`
- [ ] Ensure all errors have proper context:
  - [ ] Config loading: `"Failed to load configuration from {path}"`
  - [ ] Config validation: `"Configuration validation failed"`
  - [ ] Service creation: `"Failed to create {} service"`
  - [ ] Service run: `"{} service failed"`
  - [ ] Signal setup: `"Failed to setup shutdown signals"`
- [ ] Add error logging before propagation
- [ ] Test error paths

#### Phase 7: Testing 🧪
- [ ] Unit tests for traits:
  - [ ] Mock config implementing `ZZNetConfig`
  - [ ] Mock service implementing `ZZNetService`
  - [ ] Test `run_service()` happy path
  - [ ] Test `run_service()` error paths
- [ ] Integration tests:
  - [ ] Create minimal test app using `run_service()`
  - [ ] Test startup sequence
  - [ ] Test shutdown sequence
  - [ ] Test error handling
- [ ] Update existing app tests:
  - [ ] Collector tests should still pass
  - [ ] Database tests should still pass
- [ ] **Target: 350+ tests passing** (currently 342)

#### Phase 8: Documentation 📚
- [ ] Update `lib.rs` with new API:
  - [ ] Remove old `build_and_run()` examples
  - [ ] Add `run_service()` examples
  - [ ] Show before/after comparison
- [ ] Update module docs:
  - [ ] `builder.rs` - new methods
  - [ ] `traits.rs` - comprehensive trait docs
  - [ ] `signals.rs` - signal handling docs
- [ ] Create migration guide:
  - [ ] How to migrate from `build_and_run()` to `run_service()`
  - [ ] How to implement the traits
  - [ ] Common pitfalls
- [ ] Update `03_full_implementation_complete.md` to reflect ACTUAL completion

#### Phase 9: Cleanup and Polish 🧹
- [ ] Remove old backup files (`main_old.rs`, `main_new.rs`)
- [ ] Remove `build_and_run()` method (breaking change - or deprecate)
- [ ] Run `cargo fmt` on all modified files
- [ ] Run `cargo clippy` and fix warnings
- [ ] Check for any remaining TODOs
- [ ] Update CHANGELOG

#### Phase 10: Final Validation ✅
- [ ] Run full test suite: `cargo nextest run`
- [ ] Build release binaries: `cargo build --release`
- [ ] Test collector binary manually
- [ ] Test database binary manually
- [ ] Run connectivity test
- [ ] Check code coverage
- [ ] Final line count comparison:
  - [ ] Collector main.rs: Before ~43 lines → After ~3-5 lines
  - [ ] Database main.rs: Before ~42 lines → After ~3-5 lines
  - [ ] **Total reduction: ~85 lines → ~10 lines (88% reduction)**

## Success Metrics

### Before (Current State)
- **Collector main.rs**: 43 lines (with AppBuilder)
- **Database main.rs**: 42 lines (with AppBuilder)
- **Total**: 85 lines
- **Duplication**: ~30 lines per app (~80% identical patterns)
- **Test count**: 342 tests

### After (Target State)
- **Collector main.rs**: 3-5 lines
- **Database main.rs**: 3-5 lines
- **Total**: 6-10 lines
- **Duplication**: 0 lines (100% abstracted)
- **Test count**: 350+ tests
- **Lines eliminated**: ~75 lines (88% reduction)

### Quality Gates
- ✅ All existing tests pass (342+)
- ✅ New tests for traits and lifecycle (8+)
- ✅ Zero duplication between app main.rs files
- ✅ Clean error messages with context
- ✅ Graceful shutdown on signals
- ✅ Comprehensive documentation
- ✅ No breaking changes to service implementations
- ✅ Both apps run correctly in production

## API Examples

### Final Goal - Collector main.rs:
```rust
use anyhow::Result;
use zznet_builder::AppBuilder;
use zzping_collector::{config::CollectorConfig, service::CollectorService};

fn main() -> Result<()> {
    AppBuilder::new("ZZPing Collector", env!("CARGO_PKG_VERSION"))
        .run_service::<CollectorService, CollectorConfig>()
}
```

### Final Goal - Database main.rs:
```rust
use anyhow::Result;
use zznet_builder::AppBuilder;
use zzping_database::{config::DatabaseConfig, service::DatabaseService};

fn main() -> Result<()> {
    AppBuilder::new("ZZPing Database", env!("CARGO_PKG_VERSION"))
        .run_service::<DatabaseService, DatabaseConfig>()
}
```

**That's 5 lines each. Total: 10 lines. Down from 85 lines.**

## Alternative: Even Simpler API

```rust
// One-liner via helper function
fn main() -> Result<()> {
    zznet_builder::run::<CollectorService, CollectorConfig>(
        "ZZPing Collector",
        env!("CARGO_PKG_VERSION")
    )
}
```

**That's 3 lines each. Total: 6 lines. 93% reduction.**

## Risk Analysis

### Low Risk ✅
- Trait implementation for existing configs (just add trait bounds)
- Trait implementation for existing services (just add trait bounds)
- Signal handling (well-tested pattern)
- Error context enhancement (additive)

### Medium Risk ⚠️
- Service lifecycle abstraction (need to test edge cases)
- Error type conversion (services have different error types)
- Shutdown timing (ensure clean shutdown)

### High Risk 🔴
- Breaking changes to public API (if we remove `build_and_run`)
- Service implementation changes (should avoid if possible)

### Mitigation
- Keep `build_and_run()` as deprecated, not removed
- Make traits work with existing service methods
- Comprehensive testing before migration
- Keep old main.rs as `main_old.rs` for rollback

## Timeline Estimate

- **Phase 1-2** (Traits + Builder): 30 minutes
- **Phase 3-4** (App migrations): 30 minutes
- **Phase 5-6** (Signals + errors): 30 minutes
- **Phase 7** (Testing): 45 minutes
- **Phase 8-9** (Docs + cleanup): 30 minutes
- **Phase 10** (Validation): 15 minutes

**Total: ~3 hours for REAL 1.0 implementation**

## Conclusion

The current implementation is **incomplete**. While it handles setup boilerplate, it does NOT eliminate the service lifecycle duplication that accounts for 80% of main.rs code.

**This plan delivers the REAL 1.0** by:
1. Defining service/config traits
2. Abstracting the complete lifecycle
3. Reducing main.rs to 3-5 lines each
4. Eliminating 88% of boilerplate
5. Achieving ZERO duplication between apps

**Only when this checklist is 100% complete can we claim "1.0 quality".**
