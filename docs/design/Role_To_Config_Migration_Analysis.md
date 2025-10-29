# Role Enum to Fine-Grained Config Migration Analysis

**Date:** October 29, 2025
**Status:** Analysis Phase
**Goal:** Remove role enums from components and replace with SOLID configuration properties

---

## Overview

Components currently use "role" enums (e.g., `IntentConfigRole::Database`, `MemDBRole::Collector`) which violate SOLID principles by making components aware of application-level concerns. We need to refactor these to fine-grained configuration properties.

**Core Principle:** Components should know about *what they do* (configuration properties), not *who they are* (roles like Database/Collector).

---

## Component Analysis

### 1. IntentConfig Component

#### Current Role Enum
```rust
pub enum IntentConfigRole {
    Database { config_file_path: PathBuf },
    Collector,
}
```

#### Actual Behaviors
- **Database**: Persists config to disk, accepts config change requests, sends updates to peers
- **Collector**: Receives config updates from peers, no persistence

#### Proposed Config Structure
```rust
pub struct IntentConfigConfig {
    /// Whether to persist configuration to disk
    pub persist_config: bool,

    /// Path to config file (only used if persist_config is true)
    pub config_file_path: Option<PathBuf>,

    /// Whether to accept configuration change requests
    pub accept_config_changes: bool,

    /// Whether to send config updates to peers (push model)
    pub broadcast_config_updates: bool,
}
```

#### Migration Strategy
1. Replace `IntentConfigRole` enum with `IntentConfigConfig` struct
2. Update `IntentConfigActor::new_with_role(role)` → `IntentConfigActor::new(config)`
3. Replace all `self.role.is_database()` checks with `self.config.persist_config` etc.
4. Update `IntentConfigBuilder` API:
   - Old: `.role(IntentConfigRole::Database { ... })`
   - New: `.persist_config(true).config_file_path(path).accept_config_changes(true)`
5. Update app-level code to construct proper configs instead of roles

#### App-Level Config Mapping
**Database app:**
```rust
IntentConfigConfig {
    persist_config: true,
    config_file_path: Some(PathBuf::from("data/intent.ron")),
    accept_config_changes: true,
    broadcast_config_updates: true,
}
```

**Collector app:**
```rust
IntentConfigConfig {
    persist_config: false,
    config_file_path: None,
    accept_config_changes: false,
    broadcast_config_updates: false,
}
```

---

### 2. MemDB Component

#### Current Role Enum
```rust
pub enum MemDBRole {
    Database {
        max_results_per_target: usize,
        persistence_path: Option<PathBuf>,
    },
    Collector {
        buffer_size: usize,
    },
}
```

#### Actual Behaviors
- **Database**: Stores received batches, provides query interface, limited storage per target
- **Collector**: Buffers local results, sends batches when full, no query interface

#### Proposed Config Structure
```rust
pub struct MemDBConfig {
    /// Maximum results to buffer before action (batch send or storage limit)
    pub buffer_size: usize,

    /// Maximum results to store per target (0 = unlimited)
    /// Only used when accepting batches from network
    pub max_results_per_target: usize,

    /// Path for persistence (None = in-memory only)
    pub persistence_path: Option<PathBuf>,

    /// Whether to accept batch submissions from network
    pub accept_batches: bool,

    /// Whether to provide query interface
    pub allow_queries: bool,
}
```

#### Migration Strategy
1. Replace `MemDBRole` enum with `MemDBConfig` struct
2. Update `MemDBActor::new_with_role(role)` → `MemDBActor::new(config)`
3. Replace role checks with config property checks
4. Update `MemDBBuilder` API with individual config setters
5. Update message handlers to check config properties instead of role

#### App-Level Config Mapping
**Database app:**
```rust
MemDBConfig {
    buffer_size: 0,  // Not buffering locally
    max_results_per_target: 10000,
    persistence_path: Some(PathBuf::from("data/memdb.bin")),
    accept_batches: true,
    allow_queries: true,
}
```

**Collector app:**
```rust
MemDBConfig {
    buffer_size: 50,
    max_results_per_target: 0,  // Not storing
    persistence_path: None,
    accept_batches: false,
    allow_queries: false,
}
```

---

### 3. CState (Collector State) Component

#### Current Role Enum
```rust
pub enum CStateRole {
    Collector {
        collector_id: String,
        heartbeat_interval_ms: u64,
    },
    Database {
        stale_timeout_secs: u64,
        max_collectors: Option<usize>,
    },
    Admin,
}
```

#### Actual Behaviors
- **Collector**: Sends periodic heartbeats with ID
- **Database**: Tracks collector states, detects stale collectors
- **Admin**: Queries collector states

#### Proposed Config Structure
```rust
pub struct CStateConfig {
    /// If set, send heartbeats with this ID
    pub collector_id: Option<String>,

    /// Heartbeat interval (only used if collector_id is Some)
    pub heartbeat_interval_ms: u64,

    /// If set, track collector states and detect stale
    pub track_collectors: bool,

    /// Timeout for marking collectors stale (only used if track_collectors)
    pub stale_timeout_secs: u64,

    /// Maximum collectors to track (None = unlimited)
    pub max_collectors: Option<usize>,

    /// Whether to provide query interface
    pub allow_queries: bool,
}
```

#### Migration Strategy
1. Replace `CStateRole` enum with `CStateConfig` struct
2. Update `CStateActor::new(role)` → `CStateActor::new(config)`
3. Replace role checks:
   - `role.is_collector()` → `config.collector_id.is_some()`
   - `role.is_database()` → `config.track_collectors`
   - `role.is_admin()` → `config.allow_queries && !config.track_collectors`
4. Update `CStateBuilder` API
5. Update message handlers to check config properties

#### App-Level Config Mapping
**Database app:**
```rust
CStateConfig {
    collector_id: None,
    heartbeat_interval_ms: 0,
    track_collectors: true,
    stale_timeout_secs: 30,
    max_collectors: Some(100),
    allow_queries: true,
}
```

**Collector app:**
```rust
CStateConfig {
    collector_id: Some("collector-01".into()),
    heartbeat_interval_ms: 5000,
    track_collectors: false,
    stale_timeout_secs: 0,
    max_collectors: None,
    allow_queries: false,
}
```

**Admin app (hypothetical):**
```rust
CStateConfig {
    collector_id: None,
    heartbeat_interval_ms: 0,
    track_collectors: false,
    stale_timeout_secs: 0,
    max_collectors: None,
    allow_queries: true,
}
```

---

## Implementation Plan

### Phase 1: IntentConfig Migration
1. Create `IntentConfigConfig` struct
2. Update actor to use config instead of role
3. Update builder API
4. Update all call sites (database app, collector app)
5. Remove deprecated `IntentConfigRole` enum
6. Run tests and validate behavior unchanged

### Phase 2: MemDB Migration
1. Create `MemDBConfig` struct
2. Update actor to use config instead of role
3. Update builder API
4. Update message handlers (check config properties)
5. Update all call sites
6. Remove deprecated `MemDBRole` enum
7. Run tests and validate behavior unchanged

### Phase 3: CState Migration
1. Create `CStateConfig` struct
2. Update actor to use config instead of role
3. Update builder API
4. Update message handlers (check config properties)
5. Update all call sites
6. Remove deprecated `CStateRole` enum
7. Run tests and validate behavior unchanged

### Phase 4: Validation
1. Run full test suite
2. Verify no deprecation warnings remain
3. Manual integration testing (database + collector)
4. Update documentation

---

## Benefits

1. **SOLID Compliance:** Components only know about their own capabilities, not application roles
2. **Flexibility:** Can mix behaviors (e.g., persist config + no queries)
3. **Testability:** Easier to test specific behaviors in isolation
4. **Clarity:** Config names clearly express *what* not *who*
5. **Maintainability:** No cross-component coupling via role concepts

---

## Risks & Mitigation

**Risk:** Breaking existing tests and integration
**Mitigation:** Migrate one component at a time, run tests at each step

**Risk:** Verbose configuration at app level
**Mitigation:** Provide helper functions for common patterns (e.g., `MemDBConfig::for_database()`)

**Risk:** Missing edge cases in behavior mapping
**Mitigation:** Comprehensive test coverage, careful code review

---

## Next Steps

1. Review this analysis with team
2. Start with IntentConfig (smallest surface area)
3. Create PR per component migration
4. Update architecture docs after completion
