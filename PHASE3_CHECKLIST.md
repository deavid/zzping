# Phase 3 Implementation Checklist: `zzcollector-state` Component

**Target:** Week 3 (Following Phase 2 `zzpinger` completion)
**Current Status:** Ready to begin - Phase 1 & 2 complete
**Goal:** Create the collector state management component with >85% test coverage

---

## Overview

The `zzcollector-state` component manages collector identity, health reporting, and registration with the database. It handles:
- Collector identity and stable ID management
- Periodic heartbeat transmission to database
- Health metrics aggregation from other components
- Connection lifecycle awareness (session management)
- State persistence across restarts

**Key Integration Points:**
- Receives health data from `zzpinger` and `zzmem-db`
- Sends heartbeat messages to database via SessionManager
- Provides collector identity for mTLS certificate CN
- Maintains uptime and connection state

---

## Week 3 Task List - Copy this and check off as you go!

---

## Day 1: Project Setup and Messages

### Morning: Crate Structure
- [ ] Create directory: `src/components/zzcollector-state/`
- [ ] Create `Cargo.toml` with dependencies:
  ```toml
  [package]
  name = "zzcollector-state"
  version = "0.1.0"
  edition = "2021"

  [dependencies]
  actix = "0.13"
  tokio = { version = "1.0", features = ["full", "time"] }
  serde = { version = "1.0", features = ["derive"] }
  ron = "0.8"
  tracing = "0.1"
  chrono = "0.4"
  zznet-session = { path = "../../net/zznet-session" }
  zznet-auth = { path = "../../net/zznet-auth" }

  [dev-dependencies]
  ntest = "0.9"
  tempfile = "3.0"
  ```
- [ ] Create `src/lib.rs` with module declarations
- [ ] Create files: `messages.rs`, `network_messages.rs`, `actor.rs`, `builder.rs`, `api.rs`, `role.rs`, `state.rs`

### Afternoon: Define Network Messages
- [ ] In `network_messages.rs`, define:
  ```rust
  #[derive(Serialize, Deserialize, Debug, Clone)]
  pub enum CStateMessage {
      /// Collector → Database: Register and report health
      Heartbeat {
          collector_id: String,
          uptime_secs: u64,
          pings_sent: u64,
          pings_received: u64,
          batches_sent: u64,
          last_config_update_ms: u64,
          connection_nonce: u64,
      },

      /// Database → Collector: Acknowledgment with server time
      HeartbeatAck {
          timestamp_ms: u64,
          server_time_ms: u64,
      },

      /// Database → Admin: List of active collectors (admin only)
      CollectorList {
          collectors: Vec<CollectorInfo>,
      },

      /// Admin → Database: Request collector list
      QueryCollectors,
  }

  #[derive(Serialize, Deserialize, Debug, Clone)]
  pub struct CollectorInfo {
      pub id: String,
      pub last_seen_ms: u64,
      pub uptime_secs: u64,
      pub pings_sent: u64,
      pub pings_received: u64,
      pub connection_nonce: u64,
  }
  ```
- [ ] Implement `RoomMessageTrait` for `CStateMessage`
- [ ] Write serialization tests for all message types

### Evening: Internal Messages
- [ ] In `messages.rs`, define internal actor messages:
  ```rust
  /// Command to update health metrics from other components
  #[derive(Message)]
  #[rtype(result = "()")]
  pub struct UpdateHealthMetrics {
      pub pings_sent: Option<u64>,
      pub pings_received: Option<u64>,
      pub batches_sent: Option<u64>,
      pub last_config_update_ms: Option<u64>,
  }

  /// Query current collector state
  #[derive(Message)]
  #[rtype(result = "CollectorState")]
  pub struct GetCollectorState;

  /// Force immediate heartbeat (for testing)
  #[derive(Message)]
  #[rtype(result = "Result<(), CStateError>")]
  pub struct ForceHeartbeat;

  /// Get health information
  #[derive(Message)]
  #[rtype(result = "CStateHealth")]
  pub struct GetHealth;
  ```
- [ ] Define `CollectorState` and `CStateHealth` structs
- [ ] Write basic message tests
- [ ] Commit: "feat(zzcollector-state): Add message definitions"

---

## Day 2: Role Configuration and State

### Morning: Define Roles
- [ ] In `role.rs`, define:
  ```rust
  #[derive(Debug, Clone)]
  pub enum CStateRole {
      /// Collector: Reports health to database
      Collector {
          collector_id: String,
          heartbeat_interval_secs: u64,
      },
      /// Database: Tracks active collectors
      Database {
          stale_timeout_secs: u64,
          max_collectors: Option<usize>,
      },
      /// Admin: Queries collector list
      Admin,
  }
  ```
- [ ] Add helper methods: `is_collector()`, `is_database()`, `is_admin()`
- [ ] Write tests for role behavior

### Afternoon: State Management
- [ ] In `state.rs`, create:
  ```rust
  /// Collector-side state
  pub struct CollectorStateData {
      pub collector_id: String,
      pub connection_nonce: u64,
      pub start_time: std::time::Instant,
      pub pings_sent: u64,
      pub pings_received: u64,
      pub batches_sent: u64,
      pub last_config_update_ms: u64,
      pub last_heartbeat_sent_ms: u64,
      pub last_heartbeat_ack_ms: u64,
  }

  /// Database-side state for tracking collectors
  pub struct DatabaseStateData {
      pub collectors: HashMap<String, TrackedCollector>,
  }

  pub struct TrackedCollector {
      pub id: String,
      pub last_seen_ms: u64,
      pub uptime_secs: u64,
      pub pings_sent: u64,
      pub pings_received: u64,
      pub connection_nonce: u64,
      pub peer_id: String,  // SessionManager peer ID
  }
  ```
- [ ] Implement state update methods
- [ ] Write state management tests

### Evening: Connection Nonce Generation
- [ ] Implement nonce generation (random u64)
- [ ] Ensure nonce is generated once at startup
- [ ] Add tests for nonce uniqueness
- [ ] Commit: "feat(zzcollector-state): Add role and state structures"

---

## Day 3: Actor Implementation (Collector Side)

### Morning: Basic Actor Structure
- [ ] In `actor.rs`, create:
  ```rust
  pub struct CStateActor<T: ApplicationRole> {
      role: CStateRole,
      collector_state: Option<CollectorStateData>,
      database_state: Option<DatabaseStateData>,
      session_manager: Option<Rc<SessionManager<CStateMessage, PermissionWrapper<T>>>>,
      heartbeat_task: Option<tokio::task::JoinHandle<()>>,
      // Health metrics
      heartbeats_sent: Arc<AtomicU64>,
      heartbeats_acked: Arc<AtomicU64>,
      heartbeats_failed: Arc<AtomicU64>,
  }
  ```
- [ ] Implement `Actor` trait
- [ ] Implement `Default` and constructors for each role

### Afternoon: Heartbeat Timer
- [ ] Implement periodic heartbeat task:
  ```rust
  async fn heartbeat_loop(
      addr: Addr<CStateActor<T>>,
      interval_secs: u64,
  ) {
      let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
      loop {
          interval.tick().await;
          if let Err(e) = addr.send(ForceHeartbeat).await {
              // Actor stopped, exit loop
              break;
          }
      }
  }
  ```
- [ ] Start heartbeat task in `started()` lifecycle hook
- [ ] Stop heartbeat task in `stopped()` lifecycle hook
- [ ] Write tests for heartbeat timing

### Evening: Heartbeat Sending
- [ ] Implement `Handler<ForceHeartbeat>`:
  - Collect current metrics
  - Create `Heartbeat` message
  - Send via SessionManager to database peer
  - Update last_heartbeat_sent timestamp
  - Increment counter
- [ ] Handle send errors gracefully (log, increment failed counter)
- [ ] Write tests for heartbeat sending
- [ ] Commit: "feat(zzcollector-state): Implement collector heartbeat"

---

## Day 4: Actor Implementation (Database Side)

### Morning: Heartbeat Reception
- [ ] Implement handler for incoming `Heartbeat` message:
  ```rust
  impl<T> Handler<CStateMessage> for CStateActor<T> {
      fn handle(&mut self, msg: CStateMessage, ctx: &mut Context<Self>) {
          match msg {
              CStateMessage::Heartbeat { .. } => {
                  // Update or insert collector info
                  // Send HeartbeatAck
              }
              // ... other messages
          }
      }
  }
  ```
- [ ] Update collector tracking in `DatabaseStateData`
- [ ] Send `HeartbeatAck` response
- [ ] Write tests for heartbeat reception

### Afternoon: Stale Collector Detection
- [ ] Implement periodic cleanup task:
  ```rust
  async fn cleanup_stale_collectors(
      addr: Addr<CStateActor<T>>,
      check_interval_secs: u64,
      stale_timeout_secs: u64,
  ) {
      let mut interval = tokio::time::interval(Duration::from_secs(check_interval_secs));
      loop {
          interval.tick().await;
          // Send message to actor to check and remove stale collectors
      }
  }
  ```
- [ ] Mark collectors as stale if no heartbeat for X seconds
- [ ] Remove stale collectors from tracking map
- [ ] Write tests for stale detection

### Evening: Query Interface
- [ ] Implement `Handler<QueryCollectors>`:
  - Return list of active collectors
  - Filter out stale entries
- [ ] Implement permission checks (admin only)
- [ ] Write tests for query interface
- [ ] Commit: "feat(zzcollector-state): Implement database tracking"

---

## Day 5: Metrics Integration and API

### Morning: Health Metrics Handler
- [ ] Implement `Handler<UpdateHealthMetrics>`:
  ```rust
  impl<T> Handler<UpdateHealthMetrics> for CStateActor<T> {
      fn handle(&mut self, msg: UpdateHealthMetrics, _ctx: &mut Context<Self>) {
          if let Some(state) = &mut self.collector_state {
              if let Some(val) = msg.pings_sent {
                  state.pings_sent = val;
              }
              // ... update other metrics
          }
      }
  }
  ```
- [ ] Write tests for metrics updates

### Afternoon: Builder Pattern
- [ ] In `builder.rs`, create:
  ```rust
  pub struct CStateBuilder<T: ApplicationRole> {
      role: CStateRole,
      session_manager: Option<Rc<SessionManager<...>>>,
  }

  impl<T> CStateBuilder<T> {
      pub fn new(role: CStateRole) -> Self { ... }
      pub fn session_manager(mut self, sm: Rc<SessionManager<...>>) -> Self { ... }
      pub fn start(self) -> Result<Addr<CStateActor<T>>, CStateError> { ... }
  }
  ```
- [ ] Add validation in `start()` method
- [ ] Write builder tests

### Evening: Public API
- [ ] In `api.rs`, create:
  ```rust
  pub struct CStateHandle {
      addr: Addr<CStateActor<...>>,
  }

  impl CStateHandle {
      pub async fn update_metrics(&self, metrics: UpdateHealthMetrics) -> Result<...>
      pub async fn get_state(&self) -> Result<CollectorState>
      pub async fn get_health(&self) -> Result<CStateHealth>
      pub async fn force_heartbeat(&self) -> Result<...>
  }
  ```
- [ ] Implement convenience methods
- [ ] Write API tests
- [ ] Commit: "feat(zzcollector-state): Add builder and API"

---

## Day 6: Documentation

### Morning: Inline Documentation
- [ ] Add docstrings to all public items following `AGENT_CODING_STANDARDS.md`:
  - [ ] `CStateMessage` enum and variants
  - [ ] `CStateActor` struct
  - [ ] `CStateRole` enum
  - [ ] Builder methods
  - [ ] API methods
  - [ ] State structures
- [ ] Explain "why" not "what"
- [ ] No forbidden patterns

### Afternoon: README
- [ ] Create `src/components/zzcollector-state/README.md` with:
  - [ ] Purpose and overview
  - [ ] Role descriptions (Collector, Database, Admin)
  - [ ] Message protocol details
  - [ ] Heartbeat mechanism explanation
  - [ ] Stale detection algorithm
  - [ ] Integration with other components
  - [ ] Usage examples (all roles)
  - [ ] Testing instructions

### Evening: Examples
- [ ] Create `examples/` directory
- [ ] Add example: `collector_heartbeat.rs` (collector role)
- [ ] Add example: `database_tracking.rs` (database role)
- [ ] Add example: `admin_query.rs` (admin role)
- [ ] Commit: "docs(zzcollector-state): Add comprehensive documentation"

---

## Day 7: Polish and Review

### Morning: Code Quality
- [ ] Run `cargo fmt` on all files
- [ ] Run `cargo clippy -- -D warnings` and fix all issues
- [ ] Ensure all compiler warnings are resolved
- [ ] Review error handling (no panics, proper `Result` types)

### Afternoon: Coverage Report
- [ ] Run: `./coverage-report.sh zzcollector-state`
- [ ] Check coverage: target >85%
- [ ] Write additional tests for uncovered code
- [ ] Focus on error paths and edge cases

### Evening: Final Review
- [ ] Re-read all documentation
- [ ] Check consistency with `COMPONENT_TEMPLATE_GUIDE.md`
- [ ] Verify adherence to `AGENT_CODING_STANDARDS.md`
- [ ] Run full test suite:
  ```bash
  cargo test --package zzcollector-state --lib
  cargo test --package zzcollector-state --examples
  ```
- [ ] Create PR: "feat(zzcollector-state): Complete collector state component"

---

## Success Criteria (Before Moving to Week 4)

### Code Quality
- [ ] All tests pass
- [ ] Code coverage >85%
- [ ] No compiler warnings
- [ ] No clippy warnings
- [ ] Follows coding standards

### Functionality
- [ ] Collector role sends periodic heartbeats
- [ ] Database role tracks collectors
- [ ] Stale detection works
- [ ] Query interface works (admin role)
- [ ] Metrics integration works
- [ ] Connection lifecycle handled correctly

### Documentation
- [ ] README complete with examples
- [ ] All public APIs documented
- [ ] Inline documentation follows standards
- [ ] Examples compile and run

### Architecture
- [ ] Component follows template pattern
- [ ] Same code handles all roles
- [ ] Works with mock SessionManager
- [ ] No coupling to other components (except message types)
- [ ] Transport-agnostic

---

## Common Pitfalls to Avoid

### ❌ Don't Do This
1. **Hardcoding heartbeat interval** - Make it configurable
2. **Blocking in heartbeat task** - Use async/await properly
3. **Not handling SessionManager absence** - Graceful degradation for tests
4. **Forgetting to stop background tasks** - Clean up in `stopped()` hook
5. **Panicking on send failures** - Log and increment error counter
6. **Not testing stale detection** - Critical for reliability
7. **Coupling to specific peer IDs** - Work with any peer that offers the room

### ✅ Do This Instead
1. **Configure via CStateRole** - Pass interval in role config
2. **Spawn dedicated tokio tasks** - Don't block actor handlers
3. **Allow None SessionManager** - For unit tests without network
4. **Abort tasks in stopped()** - Prevent zombie tasks
5. **Return Result, log errors** - Resilient to network issues
6. **Test with mock time** - Use `tokio::time::pause()` in tests
7. **Use room-based messaging** - Let SessionManager handle routing

---

## Technical Considerations

### Connection Nonce Generation

**Purpose:** Disambiguate multiple connections from the same logical collector

```rust
use rand::Rng;

fn generate_connection_nonce() -> u64 {
    rand::thread_rng().gen()
}
```

**Usage:**
- Generated once at collector startup
- Sent in every heartbeat
- Database uses it to detect process restarts
- Format: `(collector_id, connection_nonce)` uniquely identifies a session

### Heartbeat Timing

**Collector side:**
- Default interval: 5 seconds
- Configurable via role
- Should be < 50% of stale timeout

**Database side:**
- Stale timeout: 15 seconds (3x heartbeat interval)
- Cleanup check interval: 10 seconds
- Configurable via role

### Time Synchronization

**Challenge:** Collector and database clocks may differ

**Solution:**
- Database includes `server_time_ms` in `HeartbeatAck`
- Collector can detect clock skew
- Use timestamps for ordering, not absolute time comparison

### State Persistence

**Collector side:**
- No persistence needed (ephemeral state)
- Connection nonce regenerated on restart

**Database side:**
- Optional: persist collector list to disk
- Reload on database restart
- Clear stale entries on startup

---

## Integration Points

### With zzpinger
```rust
// Pinger reports metrics to CState
let cstate = CStateHandle::new(...);
cstate.update_metrics(UpdateHealthMetrics {
    pings_sent: Some(pinger_health.total_pings_sent),
    pings_received: Some(pinger_health.total_pings_received),
    ..Default::default()
}).await?;
```

### With zzmem-db
```rust
// MemDB reports batch submission metrics
cstate.update_metrics(UpdateHealthMetrics {
    batches_sent: Some(memdb_health.successful_sends),
    ..Default::default()
}).await?;
```

### With zzintent-config
```rust
// IntentConfig reports last config update
cstate.update_metrics(UpdateHealthMetrics {
    last_config_update_ms: Some(timestamp),
    ..Default::default()
}).await?;
```

---

## Quick Commands Reference

```bash
# Create component structure
mkdir -p src/components/zzcollector-state/src
cd src/components/zzcollector-state

# Run tests
cargo test --package zzcollector-state --lib

# Run with output
cargo test --package zzcollector-state --lib -- --nocapture

# Run specific test
cargo test --package zzcollector-state --lib test_heartbeat_sending

# Check coverage
./coverage-report.sh zzcollector-state

# Format code
cargo fmt --package zzcollector-state

# Check lints
cargo clippy --package zzcollector-state -- -D warnings

# Build docs
cargo doc --package zzcollector-state --open
```

---

## Testing Strategy

### Unit Tests (No Network)
```rust
#[actix::test]
async fn test_heartbeat_generation() {
    let actor = CStateActor::new(CStateRole::Collector { ... });
    let state = actor.collector_state.unwrap();
    assert_eq!(state.collector_id, "test-collector");
}
```

### Integration Tests (Mock SessionManager)
```rust
#[actix::test]
async fn test_heartbeat_flow() {
    let mock_session = MockSessionManager::new();

    let collector = CStateBuilder::new(CStateRole::Collector { ... })
        .session_manager(mock_session.clone())
        .start()?;

    // Trigger heartbeat
    collector.send(ForceHeartbeat).await?;

    // Verify message sent
    let sent = mock_session.get_sent_messages();
    assert!(matches!(sent[0], CStateMessage::Heartbeat { .. }));
}
```

### Time-Based Tests
```rust
#[actix::test]
#[tokio::time::pause]
async fn test_stale_detection() {
    let database = CStateBuilder::new(CStateRole::Database {
        stale_timeout_secs: 15,
    }).start()?;

    // Simulate heartbeat
    database.send(CStateMessage::Heartbeat { ... }).await?;

    // Advance time past stale timeout
    tokio::time::advance(Duration::from_secs(20)).await;

    // Check collector is marked stale
    let collectors = database.send(QueryCollectors).await?;
    assert!(collectors.is_empty());
}
```

---

## Week 3 Completion Checklist

**Before proceeding to Week 4 (Collector Application), verify:**

- [ ] All Day 1-7 tasks completed
- [ ] All Success Criteria met
- [ ] PR created and ready for review
- [ ] No blockers or unresolved issues
- [ ] Coverage report generated and >85%
- [ ] Documentation reviewed

---

**Good luck!** 🚀
