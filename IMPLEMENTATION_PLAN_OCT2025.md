# ZZPing Implementation Plan - October 2025

**Date**: October 12, 2025
**Status**: Active Planning
**Author**: AI Assistant (based on project vision and current state analysis)

---

## Executive Summary

This document provides a comprehensive implementation plan to evolve the ZZPing project from its current state (one working component: `zzintent-config`) to a fully functional network monitoring system with multiple components communicating via the ZZNet architecture.

### Current State

**What Exists:**
- ✅ **Old codebase** (gRPC-based, functional): Moved to `src/old/`
- ✅ **ZZNet foundation**: Core network layer crates in `src/net/`
  - `zznet-api`, `zznet-auth`, `zznet-builder`, `zznet-hello`, `zznet-room`, `zznet-session`, `zznet-transport-tcp`
- ✅ **One working component**: `zzintent-config` (demonstrates the architecture)
- ✅ **Component template guide**: `COMPONENT_TEMPLATE_GUIDE.md`
- ✅ **Architectural vision**: Comprehensive documentation

**What's Missing:**
- ❌ Core components needed for minimal functionality:
  - `zzmem-db` (in-memory database for ping data)
  - `zzpinger` (performs actual pings)
  - `zzcollector-state` (collector state management)
- ❌ New application binaries:
  - `zzping-collector` (new version using ZZNet)
  - `zzping-database` (new version using ZZNet)
- ❌ Integration between components
- ❌ End-to-end testing

### The Target

A working system where:
1. **Database process** runs with `zzintent-config` (already works) + `zzmem-db` + `zzcollector-state`
2. **Collector process** runs with `zzpinger` + local state components
3. **Communication** happens via ZZNet rooms over mTLS
4. **Configuration** flows from database to collectors via `intent-config` room
5. **Ping data** flows from collectors to database via `mem-db` room
6. **State updates** flow bidirectionally via `c-state` room

---

## Part 1: Strategic Overview

### 1.1 Core Architectural Principles (Must Follow)

From `ZZPing_Network_Layer_Vision.md`:

**Critical Principle #1: Transport-Agnostic Communication**
- Components communicate using **typed Rust messages**
- SessionManager operates **100% on types, never bytes**
- Serialization happens at transport boundary, not in components

**Critical Principle #2: Same Component, Different Config**
```
❌ WRONG: Different components on each side
   Collector: PingSubmitter
   Database:  PingReceiver

✅ CORRECT: Same component, different role
   Collector: MemDB(role=Collector)
   Database:  MemDB(role=Database)
```

**Critical Principle #3: Rooms are 1:1 Typed Channels**
- A room connects exactly TWO endpoints (one per connection)
- Rooms are per-connection (each TCP connection has its own set)
- Rooms are auto-joined via intersection during connection handshake
- No dynamic join/leave, no broadcasting to multiple peers

**Critical Principle #4: Component Isolation**
- Each component is self-contained (all network code in one crate)
- Components register room handlers with SessionManager
- Components receive typed messages only (never bytes)
- Components work with 0 to N connections (resilient to disconnection)

### 1.2 Development Philosophy

**Test-Driven Development:**
- Write tests BEFORE implementation
- Mock transport first, real transport later
- Components must be testable without network I/O
- Target >85% code coverage per component

**Incremental Delivery:**
- Build one component at a time
- Each component must work standalone with tests
- Integration happens after components are individually validated
- Always have working code (never break existing tests)

**Documentation-First:**
- Update docs BEFORE coding
- Each component gets its own README
- Code comments explain "why" not "what"
- Follow `AGENT_CODING_STANDARDS.md` rigorously

---

## Part 2: Component Breakdown

### 2.1 Component: `zzmem-db` (In-Memory Ping Database)

**Purpose**: Store ping results in memory for later querying/persistence

**Location**: `src/components/zzmem-db/`

**Roles**:
- `Collector`: Sends ping results to database
- `Database`: Receives and stores ping results

**Messages** (in `network_messages.rs`):
```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MemDBMessage {
    /// Collector → Database: Batch of ping results
    SubmitBatch {
        timestamp_ms: u64,
        results: Vec<PingResult>,
    },

    /// Database → Collector: Acknowledgment
    BatchAck {
        received_count: usize,
        timestamp_ms: u64,
    },

    /// Admin → Database: Query stored data
    Query {
        target: String,
        from_ms: u64,
        to_ms: u64,
    },

    /// Database → Admin: Query response
    QueryResponse {
        results: Vec<StoredPingResult>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PingResult {
    pub target: String,
    pub timestamp_ms: u64,
    pub rtt_us: Option<u32>,  // None = packet lost
    pub sequence: u32,
}
```

**State**:
```rust
struct MemDBActor<T: ApplicationRole> {
    role: MemDBRole,
    data_store: HashMap<String, Vec<StoredPingResult>>,
    session_manager: Option<Rc<SessionManager<MemDBMessage, PermissionWrapper<T>>>>,
    // Health counters
    batches_received: Arc<AtomicU64>,
    results_stored: Arc<AtomicU64>,
}

enum MemDBRole {
    Collector,
    Database { max_results_per_target: usize },
}
```

**Key Behaviors**:
- **Collector role**: Buffers ping results, sends batches to database
- **Database role**: Receives batches, stores in memory, provides query interface
- **Per-connection independence**: Each collector connection maintains separate state
- **Resilience**: Collector buffers during disconnection, sends on reconnect

**Tests Needed**:
- [ ] Unit: Store and retrieve ping results
- [ ] Unit: Buffer overflow handling (drop oldest)
- [ ] Integration: Collector sends batch, database receives and acks
- [ ] Integration: Connection loss and reconnection (no data loss)
- [ ] Integration: Query interface for admins

### 2.2 Component: `zzpinger` (ICMP Ping Engine)

**Purpose**: Send ICMP echo requests and collect responses

**Location**: `src/components/zzpinger/`

**Roles**:
- `Active`: Performs pings based on configuration
- `Passive`: Does not ping (for database/admin processes)

**Messages** (mostly internal, integrates with `zzmem-db`):
```rust
// Internal messages (not sent over network)
pub enum PingerCommand {
    UpdateTargets(Vec<String>),
    UpdateRate(f64),  // pings per second
    Stop,
}

pub enum PingerEvent {
    PingResult(PingResult),
    Error(String),
}
```

**State**:
```rust
struct PingerActor {
    targets: Vec<String>,
    rate_pps: f64,
    socket: RawSocket,  // Requires CAP_NET_RAW
    result_sink: Recipient<PingResult>,  // Sends to MemDB
    // Stats
    pings_sent: Arc<AtomicU64>,
    pings_received: Arc<AtomicU64>,
}
```

**Key Behaviors**:
- Listens for config updates from `zzintent-config`
- Sends ICMP echo requests at specified rate
- Collects responses and sends to `zzmem-db`
- Handles packet loss detection (timeout-based)
- Thread-safe (uses raw socket in dedicated thread)

**Tests Needed**:
- [ ] Unit: Rate limiting (send exactly N pings/sec)
- [ ] Unit: Timeout detection (mark as lost)
- [ ] Integration: Responds to config changes
- [ ] Integration: Sends results to MemDB
- [ ] System: Real ping to localhost (requires privileges)

### 2.3 Component: `zzcollector-state` (Collector State Management)

**Purpose**: Manage collector identity, health, and registration with database

**Location**: `src/components/zzcollector-state/`

**Roles**:
- `Collector`: Reports health and status to database
- `Database`: Tracks active collectors and their health

**Messages**:
```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CStateMessage {
    /// Collector → Database: Register and report health
    Heartbeat {
        collector_id: String,
        uptime_secs: u64,
        pings_sent: u64,
        last_config_update_ms: u64,
    },

    /// Database → Collector: Acknowledgment
    HeartbeatAck {
        timestamp_ms: u64,
    },

    /// Database → Admin: List of active collectors
    CollectorList {
        collectors: Vec<CollectorInfo>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollectorInfo {
    pub id: String,
    pub last_seen_ms: u64,
    pub uptime_secs: u64,
    pub pings_sent: u64,
}
```

**Key Behaviors**:
- Collector sends heartbeat every N seconds
- Database tracks last-seen time for each collector
- Database marks collectors as "stale" if no heartbeat for X seconds
- Admin can query list of active collectors

**Tests Needed**:
- [ ] Unit: Heartbeat timer works
- [ ] Integration: Collector registers with database
- [ ] Integration: Database detects stale collectors
- [ ] Integration: Collector reconnection updates state

---

## Part 3: Implementation Phases

### Phase 1: `zzmem-db` Component (Week 1)

**Goal**: Create the in-memory database component with full test coverage

**Tasks**:
1. [ ] **Day 1-2: Setup and Messages**
   - Create crate structure following `COMPONENT_TEMPLATE_GUIDE.md`
   - Define `MemDBMessage` enum in `network_messages.rs`
   - Define internal messages in `messages.rs`
   - Write message serialization tests

2. [ ] **Day 3-4: Actor Implementation**
   - Implement `MemDBActor` with both roles
   - Implement message handlers
   - Write unit tests (no network)

3. [ ] **Day 5: Integration Tests**
   - Test with mock SessionManager
   - Test connection lifecycle (connect/disconnect/reconnect)
   - Test per-connection state isolation

4. [ ] **Day 6-7: Documentation and Polish**
   - Write README with examples
   - Add inline documentation
   - Run coverage report (target >85%)
   - Code review and refinement

**Deliverable**: Working `zzmem-db` component with >85% test coverage

### Phase 2: `zzpinger` Component (Week 2)

**Goal**: Create the ping engine component

**Tasks**:
1. [ ] **Day 1-2: Message Integration**
   - Define `PingerCommand` and `PingerEvent`
   - Integrate with `zzmem-db` message types
   - Set up component structure

2. [ ] **Day 3-5: Core Implementation**
   - Implement ICMP socket handling
   - Implement rate limiting
   - Implement timeout detection
   - Write unit tests

3. [ ] **Day 6: Integration**
   - Test with mock `zzintent-config` (config updates)
   - Test with mock `zzmem-db` (result submission)
   - Test rate changes and target updates

4. [ ] **Day 7: Documentation**
   - Write README
   - Document privilege requirements (CAP_NET_RAW)
   - Add examples

**Deliverable**: Working `zzpinger` component with >85% test coverage

### Phase 3: `zzcollector-state` Component (Week 3)

**Goal**: Create collector state management component

**Tasks**:
1. [ ] **Day 1-2: Messages and Structure**
   - Define `CStateMessage` enum
   - Create actor structure
   - Write message tests

2. [ ] **Day 3-4: Implementation**
   - Implement heartbeat timer
   - Implement state tracking
   - Write unit tests

3. [ ] **Day 5: Integration Tests**
   - Test with mock SessionManager
   - Test stale detection
   - Test reconnection

4. [ ] **Day 6-7: Documentation**
   - Write README
   - Document behavior
   - Add examples

**Deliverable**: Working `zzcollector-state` component with >85% test coverage

### Phase 4: Collector Application (Week 4)

**Goal**: Create the new collector binary that integrates all collector-side components

**Location**: `src/apps/zzping-collector/` (new)

**Tasks**:
1. [ ] **Day 1-2: Application Structure**
   - Create binary crate
   - Define configuration file format
   - Set up logging and error handling

2. [ ] **Day 3-4: Component Integration**
   - Start all components (`zzintent-config`, `zzpinger`, `zzmem-db`, `zzcollector-state`)
   - Wire components together (e.g., IntentConfig → Pinger → MemDB)
   - Set up SessionManager with proper rooms

3. [ ] **Day 5: Connection Management**
   - Implement connection to database
   - Handle reconnection logic
   - Test with mock database

4. [ ] **Day 6-7: End-to-End Testing**
   - Test full startup sequence
   - Test configuration updates
   - Test ping execution
   - Test data submission

**Deliverable**: Working collector binary (with mock database)

### Phase 5: Database Application (Week 5)

**Goal**: Create the new database binary

**Location**: `src/apps/zzping-database/` (new)

**Tasks**:
1. [ ] **Day 1-2: Application Structure**
   - Create binary crate
   - Define configuration
   - Set up TLS server

2. [ ] **Day 3-4: Component Integration**
   - Start database-side components
   - Set up SessionManager as server
   - Handle multiple collector connections

3. [ ] **Day 5: Persistence**
   - Add disk persistence for ping data
   - Implement config file management
   - Test persistence

4. [ ] **Day 6-7: End-to-End Testing**
   - Test with real collector
   - Test multiple collectors
   - Test connection failures and recovery

**Deliverable**: Working database binary

### Phase 6: Full Integration (Week 6)

**Goal**: End-to-end testing with real network

**Tasks**:
1. [ ] **Day 1-2: Certificate Setup**
   - Update `generate_certs.sh`
   - Generate test certificates
   - Test mTLS connection

2. [ ] **Day 3-4: Integration Testing**
   - Run collector + database together
   - Test configuration flow
   - Test ping data flow
   - Test reconnection scenarios

3. [ ] **Day 5: Load Testing**
   - Test with high ping rates
   - Test with multiple collectors
   - Monitor memory usage

4. [ ] **Day 6-7: Documentation**
   - Update SETUP.md
   - Write deployment guide
   - Create troubleshooting guide

**Deliverable**: Fully working system with documentation

---

## Part 4: Testing Strategy

### 4.1 Unit Testing

**Every component must have:**
- Message serialization/deserialization tests
- Actor handler tests (using actix test framework)
- State transition tests
- Error handling tests

**Example test structure:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[actix::test]
    #[timeout(100)]  // Fast tests
    async fn test_handle_message() {
        // Test individual message handler
    }

    #[actix::test]
    async fn test_state_persistence() {
        // Test state management
    }
}
```

### 4.2 Integration Testing

**Test component interactions:**
- Component A sends message → Component B receives
- Connection lifecycle (connect/disconnect/reconnect)
- Per-connection state isolation

**Use mock SessionManager:**
```rust
#[actix::test]
async fn test_component_communication() {
    let mock_session = MockSessionManager::new();

    let comp_a = ComponentA::builder()
        .with_session_manager(mock_session.clone())
        .start()?;

    let comp_b = ComponentB::builder()
        .with_session_manager(mock_session.clone())
        .start()?;

    // Send message from A to B
    comp_a.send_message(...).await?;

    // Verify B received it
    assert_eq!(comp_b.get_state().await?, expected);
}
```

### 4.3 End-to-End Testing

**System-level tests:**
- Full collector + database with real network
- Configuration changes propagate correctly
- Ping data flows and persists
- Reconnection works without data loss

### 4.4 Coverage Requirements

**Minimum coverage per component:**
- Unit tests: >90%
- Integration tests: >80%
- Overall: >85%

**Tools:**
```bash
# Run coverage
cargo llvm-cov --workspace --html

# Check coverage
cargo llvm-cov --workspace --summary-only
```

---

## Part 5: Documentation Deliverables

### 5.1 Component Documentation

**Each component needs:**
- `README.md` with:
  - Purpose and responsibilities
  - Message types and protocols
  - Configuration options
  - Examples
  - Testing instructions

- Inline documentation following `AGENT_CODING_STANDARDS.md`:
  - Docstrings on all public items
  - Explain "why" not "what"
  - No forbidden patterns (no `Arguments:`, `Returns:` lists)

### 5.2 Application Documentation

**For each binary:**
- Configuration file format and examples
- Command-line flags
- Setup instructions (certificates, permissions)
- Troubleshooting guide

### 5.3 System Documentation

**Update existing docs:**
- `SETUP.md`: New setup instructions for ZZNet version
- `README.md`: Updated architecture overview
- `CONTRIBUTING.md`: Testing guidelines for new components

**New docs:**
- `DEPLOYMENT.md`: Production deployment guide
- `TROUBLESHOOTING.md`: Common issues and solutions

---

## Part 6: Risk Mitigation

### 6.1 Technical Risks

**Risk**: Component communication breaks under real network conditions
- **Mitigation**: Extensive mock-based testing first, then gradual network integration
- **Validation**: Integration tests with mock transport must pass before real TCP

**Risk**: Performance issues with high ping rates
- **Mitigation**: Benchmark early, profile regularly
- **Validation**: Load testing in Phase 6

**Risk**: Data loss during disconnection
- **Mitigation**: Buffering in components, explicit reconnection handling
- **Validation**: Connection lifecycle tests

### 6.2 Process Risks

**Risk**: Architecture violations (components becoming coupled)
- **Mitigation**: Code reviews, strict adherence to vision document
- **Validation**: Regular architecture reviews

**Risk**: Test coverage dropping
- **Mitigation**: CI checks, coverage requirements in PR reviews
- **Validation**: Automated coverage reports

**Risk**: Documentation getting stale
- **Mitigation**: Update docs in same PR as code changes
- **Validation**: Documentation review in PRs

---

## Part 7: Success Criteria

### 7.1 Phase Completion Criteria

**Each phase must deliver:**
- [ ] All code compiles without warnings
- [ ] All tests pass (unit + integration)
- [ ] Coverage >85% (per component)
- [ ] Documentation complete
- [ ] Code review passed
- [ ] No regression in existing tests

### 7.2 Final System Criteria

**The system is complete when:**
- [ ] Collector can connect to database via mTLS
- [ ] Configuration flows from database to collector
- [ ] Collector performs pings based on config
- [ ] Ping data flows to database
- [ ] Data persists across restarts
- [ ] Reconnection works without data loss
- [ ] Multiple collectors can connect simultaneously
- [ ] System runs for 24h without issues
- [ ] Documentation is complete and accurate

---

## Part 8: Timeline Summary

**Total estimated time: 6 weeks**

| Phase | Component | Duration | Dependencies |
|-------|-----------|----------|--------------|
| 1 | `zzmem-db` | 1 week | None |
| 2 | `zzpinger` | 1 week | Phase 1 |
| 3 | `zzcollector-state` | 1 week | None |
| 4 | Collector app | 1 week | Phases 1, 2, 3 |
| 5 | Database app | 1 week | Phases 1, 3 |
| 6 | Integration | 1 week | Phases 4, 5 |

**Parallel work opportunities:**
- Phases 1 and 3 can be done in parallel (different components)
- Phase 2 depends on Phase 1 (needs MemDB messages)
- Phases 4 and 5 can be partially parallel

**Critical path:** Phase 1 → Phase 2 → Phase 4 → Phase 6

---

## Part 9: Next Steps

### Immediate Actions (This Week)

1. **Review this plan** with project stakeholders
2. **Set up tracking** (GitHub issues/project board)
3. **Prepare development environment**
   - Ensure all dependencies installed
   - Run existing tests to verify baseline
   - Set up code coverage tools

### Week 1 Kickoff (Phase 1)

1. **Create `zzmem-db` branch**
2. **Set up crate structure** following template
3. **Define messages** and write serialization tests
4. **Begin actor implementation**

### Questions to Answer

Before starting implementation:
- [ ] Is 6-week timeline acceptable?
- [ ] Are there any components missing from this plan?
- [ ] Should we prioritize differently?
- [ ] Are there existing tests we should preserve?
- [ ] What's the deployment target (dev/staging/prod)?

---

## Appendix A: Key Architectural Constraints

**From Vision Documents - DO NOT VIOLATE:**

1. **SessionManager is transport-agnostic** (never touches bytes)
2. **Rooms are 1:1 per connection** (not broadcast channels)
3. **Same component code on both sides** (configured differently)
4. **Components are connection-agnostic** (work with 0..N connections)
5. **Test with mock transport first** (validates abstraction)
6. **All network code lives in component crate** (not split)
7. **Rooms auto-join via intersection** (no dynamic join/leave)

**If implementation violates these, STOP and redesign.**

---

## Appendix B: Reference Documents

**Must read before implementing:**
- `ZZPing_Network_Layer_Vision.md` - Core architectural vision
- `COMPONENT_TEMPLATE_GUIDE.md` - Component structure template
- `AGENT_CODING_STANDARDS.md` - Code style and documentation rules
- `CLARIFICATION_*.md` - Specific architectural clarifications
- `CONTRIBUTING.md` - Testing and contribution guidelines

**Implementation reference:**
- `src/components/zzintent-config/` - Working example component

---

## Appendix C: Common Pitfalls to Avoid

1. **Don't put serialization in SessionManager** - It's transport-agnostic
2. **Don't create separate sender/receiver components** - Same component, different roles
3. **Don't broadcast to multiple peers** - Rooms are 1:1
4. **Don't skip tests** - Tests validate architecture
5. **Don't couple components** - Each must work standalone
6. **Don't forget health metrics** - Use atomic counters for observability
7. **Don't panic on errors** - Return `Result`, log errors
8. **Don't skip documentation** - Code without docs is incomplete

---

*End of Implementation Plan*
