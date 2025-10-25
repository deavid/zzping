# Phase 3: Real Implementation Plan - Full Application Migration

**Date**: Oct 25, 2025
**Status**: Ready to Execute
**Goal**: Migrate database and collector applications from AI-generated zznet-builder pattern to actual vision architecture
**Timeline**: 7-9 days (full architectural migration)

---

## Executive Summary

### What We're Actually Doing

**NOT**: "Eliminating boilerplate"
**ACTUALLY**: **Complete architectural migration from parallel AI-generated pattern to original vision**

### The Problem

AI agents created a **parallel implementation** (`zznet-builder` with `RoomHandlerFactory`) instead of using the vision architecture (`zznet-room` with `Room<T>`). Applications use this wrong pattern.

### The Solution

1. Remove zznet-builder dependencies
2. Migrate to proper `zznet-room` + `Room<T>` + SessionManager pattern
3. Use component builders with `.with_session_manager()`
4. Delete ~450 lines of wrong pattern code
5. Replace with ~100 lines of proper vision code

---

## Current vs Target Architecture

### Current (WRONG - AI Generated)

```rust
// Application manually implements handlers
struct MemDBRoomHandlerFactory { ... }

impl RoomHandlerFactory<AuthRole> for MemDBRoomHandlerFactory {
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle> {
        Box::new(MemDBRoomHandler { ... })
    }
}

struct MemDBRoomHandler { ... }

impl RoomHandle for MemDBRoomHandler {
    fn send_message(&mut self, bytes: Vec<u8>) -> Result<(), SessionError> {
        // MANUAL DESERIALIZATION (wrong!)
        let msg = bincode::decode(...)?;
        self.actor.do_send(msg);
        Ok(())
    }
}

// Register with AI-generated ServerBuilder
ServerBuilder::new()
    .register_room_handler("memdb", Arc::new(factory))
    .start()
```

**Problems**:
- ❌ Manual serialization/deserialization in application
- ❌ Custom handler boilerplate per component
- ❌ Bypasses Room<T> infrastructure
- ❌ Not in vision documents

### Target (CORRECT - Original Vision)

```rust
// Component handles its own networking via builder
let memdb_room = MemDBRoomBuilder::new()
    .role(MemDBRole::Database { storage_path })
    .session_manager(session_manager.clone())
    .build();  // Auto-registers with SessionManager, handles serialization

// TCP server uses transport layer directly
let mut server = TcpTransportServer::with_tls(&bind_addr, tls_config).await?;

loop {
    let transport = server.accept().await?;

    // Hand connection to SessionManager via HELLO
    tokio::spawn(async move {
        if let Ok(hello_actor) = HelloActor::new(transport, session_manager.clone()) {
            // HELLO negotiates rooms, then SessionManager takes over
            hello_actor.run().await;
        }
    });
}
```

**Advantages**:
- ✅ Zero application boilerplate
- ✅ Automatic serialization via TypedSender<T>
- ✅ Component self-registration
- ✅ Matches vision documents
- ✅ Testable without network

---

## Phase 3 Breakdown

### Phase 3a: Database Application Migration (4 days)

#### Day 1: Foundation & Dependency Changes

**Morning: Analysis**
- [ ] Read full network layer design docs
- [ ] Map current database network flow
- [ ] Identify all components and their rooms
- [ ] Document current TLS configuration flow

**Afternoon: Dependency Migration**
- [ ] Update `src/apps/zzping-database/Cargo.toml`:
  - Remove: `zznet-builder = ...`
  - Add: `zznet-room = { path = "../../net/zznet-room" }`
  - Verify other deps (zznet-session, zznet-hello, zznet-transport-tcp)
- [ ] Run `cargo check --bin zzping-database` to see compilation errors
- [ ] Document all errors for next steps

**Evening: Plan detailed migration**
- [ ] Create file-by-file migration checklist
- [ ] Identify integration points (HELLO, SessionManager, Transport)

#### Day 2: Component Migration

**Morning: Update Component Setup**

**File**: `src/apps/zzping-database/src/service.rs`

Current `create_builders()`:
```rust
let intent_config = IntentConfigBuilder::new()
    .role(IntentConfigRole::Database { config_file_path });
// No session_manager!
```

Target:
```rust
// Create SessionManager FIRST
let session_manager = Arc::new(tokio::sync::Mutex::new(
    SessionManager::<AuthRole>::new()
));

// Pass to all component builders
let intent_room = IntentConfigRoomBuilder::new()
    .role(IntentConfigRole::Database { config_file_path })
    .session_manager(session_manager.clone())
    .build();

let memdb_room = MemDBRoomBuilder::new()
    .role(MemDBRole::Database { storage_path, max_results })
    .session_manager(session_manager.clone())
    .build();

let cstate_room = CStateRoomBuilder::new()
    .role(CStateRole::Database { stale_timeout_secs, max_collectors })
    .session_manager(session_manager.clone())
    .build();
```

Tasks:
- [ ] Create SessionManager in `create_builders()`
- [ ] Change component builders to build rooms
- [ ] Update `StartedComponents` struct to include session_manager
- [ ] Fix all compilation errors

**Afternoon: Delete Wrong Pattern**

**File**: `src/apps/zzping-database/src/room_handlers.rs`

Tasks:
- [ ] **DELETE ENTIRE FILE** (~250 lines)
- [ ] Remove from `lib.rs` or `mod.rs`
- [ ] Remove all imports of this module

#### Day 3: Network Layer Migration

**Morning: Rewrite network.rs**

**File**: `src/apps/zzping-database/src/network.rs`

Current (~90 lines using ServerBuilder):
```rust
let builder = ServerBuilder::<AuthRole>::new()
    .bind(&self.bind_addr)
    .register_room_handler("intent-config", intent_factory)
    .register_room_handler("memdb", memdb_factory)
    .with_tls(...)
    .start()
```

Target (~60 lines using TcpTransportServer + HELLO):
```rust
pub async fn run(&self, components: &StartedComponents) -> Result<(), String> {
    let tls_config = if let Some(tls) = &self.tls_config {
        Some(TlsConfig { ... })
    } else {
        None
    };

    let mut server = TcpTransportServer::new(&self.bind_addr, tls_config).await
        .map_err(|e| format!("Failed to create server: {:?}", e))?;

    info!("Database server listening on {}", self.bind_addr);

    // Accept connections loop
    loop {
        match server.accept().await {
            Ok(transport) => {
                let sm = components.session_manager.clone();

                tokio::spawn(async move {
                    // HELLO handshake, then hand to SessionManager
                    if let Err(e) = handle_connection(transport, sm).await {
                        error!("Connection error: {:?}", e);
                    }
                });
            }
            Err(e) => {
                error!("Accept error: {:?}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn handle_connection(
    transport: Box<dyn TransportConnection>,
    session_manager: Arc<tokio::sync::Mutex<SessionManager<AuthRole>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Extract peer identity from TLS
    let peer_identity = transport.peer_identity();

    // Run HELLO handshake
    let hello_actor = HelloActor::new(
        transport,
        session_manager,
        peer_identity,
    )?;

    hello_actor.run().await?;

    Ok(())
}
```

Tasks:
- [ ] Rewrite `DatabaseNetwork::run()` to use TcpTransportServer
- [ ] Implement connection accept loop
- [ ] Integrate HELLO handshake
- [ ] Pass connections to SessionManager
- [ ] Remove all ServerBuilder usage
- [ ] Fix compilation errors

**Afternoon: Integration Testing**
- [ ] Add connection lifecycle logging
- [ ] Test with manual collector connection
- [ ] Verify HELLO handshake works
- [ ] Verify room negotiation happens
- [ ] Check SessionManager receives connections

#### Day 4: Testing & Validation

**Morning: Unit Tests**
- [ ] Run `cargo test --bin zzping-database`
- [ ] Fix any test failures
- [ ] Update tests to new pattern if needed

**Afternoon: Integration Testing**
- [ ] Start database application manually
- [ ] Verify it binds to port
- [ ] Check TLS configuration loads
- [ ] Attempt connection with test client
- [ ] Verify HELLO handshake
- [ ] Verify room messages flow

**Evening: Documentation**
- [ ] Update database README
- [ ] Document new startup flow
- [ ] Create troubleshooting guide
- [ ] Update code comments

**Deliverables**:
- ✅ Database app uses vision pattern
- ✅ ~250 lines deleted
- ✅ All tests passing
- ✅ Documented and verified

---

### Phase 3b: Collector Application Migration (3 days)

#### Day 5: Collector Foundation

**Same pattern as database, but simpler** (collector is client, not server):

**Morning**:
- [ ] Update `src/apps/zzping-collector/Cargo.toml`
- [ ] Remove zznet-builder dependency
- [ ] Add zznet-room dependency
- [ ] Run `cargo check --bin zzping-collector`

**Afternoon**:
- [ ] Update `create_builders()` to include session_manager
- [ ] Migrate component builders to room builders
- [ ] Delete `room_handlers.rs`

#### Day 6: Network Migration

**Morning**:
- [ ] Rewrite collector network code
- [ ] Use `TcpTransportClient` instead of ClientBuilder
- [ ] Implement reconnection loop
- [ ] Integrate HELLO handshake

Target pattern:
```rust
loop {
    match TcpTransportClient::with_tls(&database_addr, tls_config)
        .connect().await
    {
        Ok(transport) => {
            info!("Connected to database");

            // Run HELLO, then hand to SessionManager
            let hello = HelloActor::new(
                transport,
                session_manager.clone(),
                None,  // Client doesn't have peer_identity yet
            )?;

            hello.run().await?;

            // Wait for disconnect
            // (SessionManager will notify components via SessionEvent::Inactive)
        }
        Err(e) => {
            warn!("Connection failed: {}, retrying in 5s", e);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
```

**Afternoon**:
- [ ] Test collector → database connection
- [ ] Verify HELLO handshake
- [ ] Verify rooms auto-joined
- [ ] Test message flow both directions

#### Day 7: Validation & Testing

**Morning**:
- [ ] Run full collector test suite
- [ ] Fix any failures
- [ ] Test with real database connection

**Afternoon**:
- [ ] End-to-end testing:
  - [ ] Collector starts
  - [ ] Connects to database
  - [ ] HELLO succeeds
  - [ ] Rooms joined
  - [ ] Ping data flows to database
  - [ ] Intent config updates flow to collector
  - [ ] Collector state updates flow to database
- [ ] Verify reconnection works
- [ ] Test connection loss handling

**Deliverables**:
- ✅ Collector app uses vision pattern
- ✅ ~200 lines deleted
- ✅ All tests passing
- ✅ Full end-to-end functionality

---

### Phase 3c: Deprecation & Cleanup (2 days)

#### Day 8: Deprecate zznet-builder

**Morning: Mark as Deprecated**
- [ ] Add to `src/net/zznet-builder/src/lib.rs`:
```rust
#![deprecated(
    since = "0.2.0",
    note = "This crate was created by AI agents and does not follow the architectural vision. \
            Use zznet-room with Room<T> pattern instead. \
            See docs/design/ZZNet_Component_Framework_Vision.md for proper pattern."
)]
```

- [ ] Update `src/net/zznet-builder/Cargo.toml`:
```toml
[package]
name = "zznet-builder"
version = "0.2.0"  # Bump version
description = "DEPRECATED: Use zznet-room instead"
```

- [ ] Create `src/net/zznet-builder/DEPRECATED.md`:
```markdown
# DEPRECATED

This crate is deprecated and will be removed in a future version.

## Why?

This crate was created by AI agents without following the architectural vision.
It implements a pattern not present in the design documents.

## Migration

Use `zznet-room` with the `Room<T>` pattern instead:

[Include migration examples]
```

**Afternoon: Update Documentation**
- [ ] Find all references to zznet-builder in docs
- [ ] Replace with proper zznet-room examples
- [ ] Update architecture diagrams
- [ ] Update RUNBOOK.md

#### Day 9: Final Cleanup

**Morning: Documentation Sweep**
- [ ] Update main README.md
- [ ] Update all component READMEs with proper examples
- [ ] Remove any zznet-builder examples
- [ ] Add "deprecated" warnings to any remaining references

**Afternoon: Review & Polish**
- [ ] Review all changed files
- [ ] Ensure consistent coding style
- [ ] Update CHANGELOG.md
- [ ] Create migration guide for any external users

**Evening: Final Verification**
- [ ] Run full test suite: `cargo test`
- [ ] Run database and collector together
- [ ] Verify all functionality
- [ ] Create completion report

**Deliverables**:
- ✅ zznet-builder marked deprecated
- ✅ All documentation updated
- ✅ Migration guide created
- ✅ No references to old pattern in examples

---

## Open Questions to Resolve

### Q1: How does HELLO integrate with SessionManager?

**Need to understand**:
- Does HelloActor take a SessionManager reference?
- After HELLO succeeds, how does SessionManager get the connection?
- Who manages the transport connection lifecycle?

**Action**: Check zznet-hello implementation

### Q2: Where does room negotiation happen?

**Options**:
A. In HELLO protocol (rooms exchanged during handshake)
B. In SessionManager (after HELLO completes)
C. Automatically via component registration

**Action**: Check vision documents and zznet-hello code

### Q3: How do components get SessionEvent notifications?

**Need to understand**:
- Does SessionManager broadcast events?
- Do components subscribe?
- How does Room<T> receive Active/Inactive events?

**Action**: Check zznet-session and zznet-room integration

### Q4: What about the ConnectionManager actor?

**Status**: Used in current code
**Question**: Is this AI-generated or in vision?
**Action**: Check if needed or should be removed

---

## Risk Assessment

### High Risks

**1. HELLO Integration Unknown** ⚠️
- Risk: Don't fully understand how HELLO hands off to SessionManager
- Mitigation: Read code and test incrementally

**2. Room Negotiation Details** ⚠️
- Risk: Unclear how rooms are auto-joined after HELLO
- Mitigation: Trace through existing POC code

**3. TLS Configuration** ⚠️
- Risk: Might break TLS during migration
- Mitigation: Test with certs early

### Medium Risks

**4. Test Coverage**
- Risk: Tests might be specific to old pattern
- Mitigation: Update tests as we go

**5. Reconnection Logic**
- Risk: Reconnection might work differently
- Mitigation: Test connection loss scenarios

### Low Risks

**6. Component Builders**
- Risk: Most components already have builders
- Mitigation: Well-understood pattern

---

## Success Criteria

### Code Quality
- [ ] Zero uses of zznet-builder in production code
- [ ] All components use Room<T> pattern
- [ ] SessionManager is only message router
- [ ] No manual serialization in applications
- [ ] Clean, vision-aligned architecture

### Functionality
- [ ] All tests pass (503 total)
- [ ] Database accepts connections
- [ ] Collector connects and reconnects
- [ ] Messages flow end-to-end
- [ ] TLS works correctly
- [ ] Room negotiation automatic

### Documentation
- [ ] No references to deprecated patterns
- [ ] Examples show proper usage
- [ ] Migration guide complete
- [ ] Architecture docs match code

---

## Timeline Summary

| Phase | Duration | Description |
|-------|----------|-------------|
| 3a | 4 days | Database application migration |
| 3b | 3 days | Collector application migration |
| 3c | 2 days | Deprecation and cleanup |
| **Total** | **9 days** | **Full architectural alignment** |

---

## Prerequisites Before Starting

### Must Answer First
1. ✅ Read full vision documents
2. ⏸️ Understand HELLO ↔ SessionManager integration
3. ⏸️ Trace room negotiation flow
4. ⏸️ Check if ConnectionManager is needed
5. ⏸️ Verify all transport capabilities

### Must Have Ready
1. ✅ Test certificates for TLS
2. ✅ Full test suite passing
3. ⏸️ HELLO integration understanding
4. ⏸️ Backup of current working code

---

## Next Immediate Steps

1. **Read and understand** zznet-hello integration
2. **Trace** how POC connects components (if it does)
3. **Answer** the open questions above
4. **Create** detailed Day 1 task list
5. **Begin** database migration

---

**Status**: ⏸️ Ready to begin after answering open questions

**Command to start**:
```bash
git checkout -b phase-3-real-migration
```

Let's do this properly! 🚀
