# ZZPing Implementation Quick Start Guide

**Date**: October 12, 2025
**For**: Getting started with the implementation plan

---

## TL;DR - Where We're Going

We're building a network monitoring system where:
- **Collectors** ping targets and send data to database
- **Database** stores data and distributes configuration
- Everything communicates via **ZZNet** (typed messages over mTLS)
- Currently **only `zzintent-config` exists** - we need to build the rest

---

## Current State vs. Target

### What Exists ✅

```
src/net/               ← ZZNet foundation (done)
  zznet-api/
  zznet-auth/
  zznet-builder/
  zznet-hello/
  zznet-room/
  zznet-session/
  zznet-transport-tcp/

src/components/        ← Components (1 done, 3+ needed)
  zzintent-config/     ✅ Working example

src/old/              ← Old gRPC version (ignore)
```

### What We Need to Build ❌

```
src/components/
  zzmem-db/           ❌ Ping data storage
  zzpinger/           ❌ ICMP ping engine
  zzcollector-state/  ❌ Collector health tracking

src/apps/
  zzping-collector/   ❌ New collector binary
  zzping-database/    ❌ New database binary
```

---

## The Architecture in 5 Minutes

### Key Insight #1: Same Component, Different Roles

```
❌ OLD THINKING: Different components on each side
   Collector: PingSubmitter
   Database:  PingReceiver

✅ NEW THINKING: Same component, configured differently
   Process A: MemDB(role=Collector)  ← buffers, sends batches
   Process B: MemDB(role=Database)   ← receives, stores
```

**Why?** All communication code lives in ONE place. Easy to test, easy to understand.

### Key Insight #2: Rooms are 1:1 Typed Channels

```
Collector Process              Database Process
┌─────────────┐               ┌─────────────┐
│ MemDB       │ ←─ "memdb" ──→│ MemDB       │
│ (Collector) │               │ (Database)  │
└─────────────┘               └─────────────┘
    ONE TCP CONNECTION
    ONE TYPED CHANNEL PER ROOM
```

**Not** a broadcast channel. **Not** a conference call. Just a **phone line between two endpoints**.

### Key Insight #3: SessionManager is Transport-Agnostic

```
┌─────────────────────────────────┐
│  Components (Business Logic)    │
│  - MemDB, Pinger, etc.          │
└─────────────────────────────────┘
           ↕ TypedMessage
┌─────────────────────────────────┐
│  SessionManager                 │  ← NEVER touches bytes!
│  - Routes typed messages        │
│  - Manages connections          │
└─────────────────────────────────┘
           ↕ TypedMessage
┌─────────────────────────────────┐
│  Serialization Layer            │  ← Converts types ↔ bytes
└─────────────────────────────────┘
           ↕ Vec<u8>
┌─────────────────────────────────┐
│  Transport (TCP/TLS)            │
└─────────────────────────────────┘
```

**Why?** Can test entire system with mock transport (no network I/O).

---

## The Three Core Components We Need

### Component 1: `zzmem-db` (In-Memory Ping Database)

**What it does:**
- **Collector side**: Buffers ping results, sends batches to database
- **Database side**: Receives batches, stores in memory

**Messages:**
```rust
enum MemDBMessage {
    SubmitBatch { results: Vec<PingResult> },
    BatchAck { received_count: usize },
    Query { target: String, from_ms: u64, to_ms: u64 },
    QueryResponse { results: Vec<...> },
}
```

**Key insight:** Same code handles both sending (collector) and receiving (database).

### Component 2: `zzpinger` (Ping Engine)

**What it does:**
- Sends ICMP echo requests
- Collects responses
- Detects timeouts (packet loss)
- Sends results to `zzmem-db`

**Not networked:** This component doesn't talk over ZZNet directly. It:
1. Receives config updates from `zzintent-config` (local actor message)
2. Sends results to `zzmem-db` (local actor message)

### Component 3: `zzcollector-state` (Collector State)

**What it does:**
- **Collector side**: Sends heartbeat to database
- **Database side**: Tracks which collectors are active

**Messages:**
```rust
enum CStateMessage {
    Heartbeat { collector_id: String, uptime_secs: u64, ... },
    HeartbeatAck { timestamp_ms: u64 },
}
```

---

## The Implementation Sequence

### Week 1: Build `zzmem-db`
1. Define messages
2. Implement actor with both roles
3. Write tests (mock SessionManager)
4. Documentation

**Output:** Working component, >85% coverage

### Week 2: Build `zzpinger`
1. Implement ICMP socket handling
2. Rate limiting
3. Integration with `zzmem-db`
4. Tests

**Output:** Working component, >85% coverage

### Week 3: Build `zzcollector-state`
1. Define messages
2. Implement heartbeat logic
3. Tests
4. Documentation

**Output:** Working component, >85% coverage

### Week 4: Build Collector Application
1. Create binary crate
2. Wire all components together
3. SessionManager setup (client mode)
4. End-to-end tests with mock database

**Output:** Working collector binary

### Week 5: Build Database Application
1. Create binary crate
2. Wire all components together
3. SessionManager setup (server mode)
4. Handle multiple collectors

**Output:** Working database binary

### Week 6: Integration & Testing
1. Real mTLS connections
2. Multiple collectors
3. Load testing
4. Documentation

**Output:** Production-ready system

---

## How to Follow `zzintent-config` Pattern

### 1. Crate Structure

```
src/components/your-component/
├── Cargo.toml
├── src/
│   ├── lib.rs              ← Module declarations
│   ├── messages.rs         ← Internal messages
│   ├── network_messages.rs ← Messages sent over network
│   ├── actor.rs            ← Component implementation
│   ├── builder.rs          ← Builder pattern
│   ├── api.rs              ← Public API wrapper
│   ├── role.rs             ← Role configuration
│   ├── permissions.rs      ← Permission model (if needed)
│   └── permission_wrapper.rs ← Auth wrapper
└── tests/
    ├── unit_tests.rs
    └── integration_tests.rs
```

### 2. Message Pattern

```rust
// messages.rs - Internal (not sent over network)
pub struct GetCurrentState;
pub struct UpdateState { pub data: ... }

// network_messages.rs - Sent over network
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum YourComponentMessage {
    Request { ... },
    Response { ... },
}

impl RoomMessageTrait for YourComponentMessage {
    fn room_id() -> RoomId { RoomId::from("your-room") }
}
```

### 3. Actor Pattern

```rust
pub struct YourActor<T: ApplicationRole> {
    role: YourRole,
    state: YourState,
    session_manager: Option<Rc<SessionManager<YourMsg, PermissionWrapper<T>>>>,
    // Health counters (atomic for async access)
    counter: Arc<AtomicU64>,
}

impl<T: ApplicationRole> Actor for YourActor<T> {
    type Context = Context<Self>;
}

impl<T: ApplicationRole> Handler<YourMessage> for YourActor<T> {
    type Result = ResponseFuture<Result<..., ...>>;

    fn handle(&mut self, msg: YourMessage, _ctx: &mut Self::Context) -> Self::Result {
        // Handle message
    }
}
```

### 4. Builder Pattern

```rust
pub struct YourBuilder<T: ApplicationRole> {
    role: YourRole,
    session_manager: Option<Rc<SessionManager<YourMsg, PermissionWrapper<T>>>>,
}

impl<T: ApplicationRole> YourBuilder<T> {
    pub fn new(role: YourRole) -> Self { ... }

    pub fn with_session_manager(mut self, sm: Rc<SessionManager<...>>) -> Self {
        self.session_manager = Some(sm);
        self
    }

    pub fn start(self) -> Result<Addr<YourActor<T>>, Error> {
        // Validate configuration
        // Start actor
        Ok(YourActor::new(...).start())
    }
}
```

### 5. Testing Pattern

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;

    #[actix::test]
    #[timeout(100)]  // Fast tests!
    async fn test_basic_functionality() {
        let actor = YourBuilder::new(YourRole::TestRole)
            .start()
            .expect("Failed to start actor");

        let result = actor.send(YourMessage).await;
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_with_session_manager() {
        let mock_sm = MockSessionManager::new();

        let actor = YourBuilder::new(YourRole::TestRole)
            .with_session_manager(Rc::new(mock_sm))
            .start()
            .expect("Failed to start");

        // Test network communication
    }
}
```

---

## Key Rules to Remember

### ✅ DO

1. **Test first, implement later** - Write tests before code
2. **Mock transport for testing** - No real network in unit tests
3. **Same component, different roles** - One crate, configured differently
4. **Document "why" not "what"** - Code is the "what", comments explain "why"
5. **Use atomic counters for health** - Thread-safe metrics
6. **Return Result, don't panic** - Graceful error handling
7. **Follow the template** - Look at `zzintent-config` as example

### ❌ DON'T

1. **Don't touch bytes in SessionManager** - It's transport-agnostic
2. **Don't create sender/receiver pairs** - Same component handles both
3. **Don't broadcast to multiple peers** - Rooms are 1:1
4. **Don't skip tests** - Tests validate architecture
5. **Don't couple components** - Each must work standalone
6. **Don't use `Arguments:`, `Returns:` in docstrings** - Forbidden pattern
7. **Don't use `#[ignore]` in doc examples** - Tests must run

---

## Files You Must Read

**Before writing any code:**
1. `ZZPing_Network_Layer_Vision.md` - Understand the architecture
2. `COMPONENT_TEMPLATE_GUIDE.md` - Follow the pattern
3. `AGENT_CODING_STANDARDS.md` - Code style rules
4. `CLARIFICATION_Room_Negotiation.md` - How rooms work
5. `CLARIFICATION_Connection_Topology.md` - Connection model

**While implementing:**
- Look at `src/components/zzintent-config/` - Working example
- Refer to `IMPLEMENTATION_PLAN_OCT2025.md` - Detailed plan

---

## Common Questions

**Q: Why same component on both sides?**
A: All protocol logic in one place. Easy to test, easy to reason about.

**Q: Why can't SessionManager touch bytes?**
A: Validates transport-agnostic design. Can test without network.

**Q: Why aren't rooms like IRC channels?**
A: Different use case. We need 1:1 typed channels, not broadcast.

**Q: How do I test without network?**
A: Use mock SessionManager that connects two actors via channels.

**Q: What if I need to send to multiple peers?**
A: Explicitly send to each peer's room. No implicit broadcast.

**Q: Why so much documentation?**
A: Complex system. Docs prevent mistakes, help future contributors.

---

## Getting Started Checklist

- [ ] Read the 5 essential documents listed above
- [ ] Understand the architecture (test yourself: explain rooms, SessionManager, component roles)
- [ ] Review `zzintent-config` code thoroughly
- [ ] Set up development environment (run existing tests)
- [ ] Create a branch for Phase 1 (`zzmem-db`)
- [ ] Start with message definitions and tests
- [ ] Follow the component template exactly

---

## When You Get Stuck

1. **Re-read the vision documents** - Answer is probably there
2. **Look at `zzintent-config`** - Working example
3. **Check implementation plan** - Detailed steps
4. **Ask specific questions** - With context and what you've tried

---

## Success Metrics

Each component is "done" when:
- [ ] All tests pass
- [ ] Coverage >85%
- [ ] Documentation complete
- [ ] Follows template pattern
- [ ] No compiler warnings
- [ ] Code review passed

---

**Ready to start? Begin with Week 1: `zzmem-db`**

See `IMPLEMENTATION_PLAN_OCT2025.md` Part 3, Phase 1 for detailed steps.

Good luck! 🚀
