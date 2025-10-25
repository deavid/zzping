# Deprecation Plan: AI-Generated Code Not in Original Vision

**Date**: Oct 25, 2025
**Purpose**: Identify and plan removal of code created by AI agents that doesn't align with the original architectural vision
**Status**: Planning Phase

---

## Overview

AI agents have created **parallel implementations** instead of following the vision. This document identifies what needs to be deprecated and removed.

---

## 1. zznet-builder (ENTIRE CRATE - DEPRECATE)

**Location**: `src/net/zznet-builder/`

**What it is**:
- AI-generated crate for `ServerBuilder` and `ClientBuilder`
- Implements `RoomHandlerFactory` trait pattern
- Custom `RoomHandle` trait for per-room message handling
- Manual serialization/deserialization in application code

**Why it was created**:
- AI agents created this WITHOUT being asked
- Not in original vision documents
- Parallel implementation to avoid migrating to proper pattern

**Why it should be deprecated**:
- ❌ Not in design documents
- ❌ Forces applications to implement custom handlers
- ❌ Requires manual serialization in app code
- ❌ Bypasses the `Room<T>` + SessionManager pattern
- ❌ Creates maintenance burden (two network patterns)

**Proper alternative**:
- Use `zznet-room` with `Room<T>` typed channels
- Components auto-register with SessionManager
- No application-level handlers needed

**Migration path**:
1. Migrate applications to use zznet-room pattern
2. Mark zznet-builder as deprecated
3. Remove after Phase 3 complete

---

## 2. Application RoomHandlerFactory Implementations (DELETE)

### Database Application

**Files to delete**:
- `src/apps/zzping-database/src/room_handlers.rs` (~250 lines)

**What it contains**:
```rust
// Custom factory implementations (NOT in vision)
pub struct IntentConfigRoomHandlerFactory { ... }
impl RoomHandlerFactory<AuthRole> for IntentConfigRoomHandlerFactory { ... }

pub struct MemDBRoomHandlerFactory { ... }
impl RoomHandlerFactory<AuthRole> for MemDBRoomHandlerFactory { ... }

pub struct CStateRoomHandlerFactory { ... }
impl RoomHandlerFactory<AuthRole> for CStateRoomHandlerFactory { ... }

// Custom handler implementations (NOT in vision)
struct DatabaseIntentConfigRoomHandler { ... }
impl RoomHandle for DatabaseIntentConfigRoomHandler {
    fn send_message(&mut self, bytes: Vec<u8>) -> Result<(), SessionError> {
        // Manual bincode deserialization (should be automatic!)
        let msg = bincode::decode(...)?;
        self.actor_addr.do_send(msg);
        Ok(())
    }
}

// Similar for MemDB and CState...
```

**Why this exists**:
- Required by zznet-builder pattern
- AI agents created this to avoid proper Room<T> integration

**Proper alternative**:
- Components use their builders with `.with_session_manager()`
- Room<T> handles serialization automatically via TypedSender<T>
- No manual handler code needed

### Collector Application

**Files to delete**:
- `src/apps/zzping-collector/src/room_handlers.rs` (estimated ~200 lines)

**Same pattern as database**

---

## 3. ServerBuilder/ClientBuilder Usage in Applications (REPLACE)

### Database network.rs (REWRITE)

**File**: `src/apps/zzping-database/src/network.rs`

**Current (wrong) pattern**:
```rust
let intent_factory = Arc::new(IntentConfigRoomHandlerFactory::new(...));
let memdb_factory = Arc::new(MemDBRoomHandlerFactory::new(...));

let builder = ServerBuilder::<AuthRole>::new()
    .bind(&self.bind_addr)
    .as_role(AuthRole::Database)
    .offer_rooms(vec!["intent-config", "memdb", "query"])
    .register_room_handler("intent-config", intent_factory)
    .register_room_handler("memdb", memdb_factory)
    .register_room_handler("query", cstate_factory)
    .with_tls(...)
    .start()
```

**Proper (vision) pattern**:
```rust
// Components create rooms automatically via builders
let intent_room = IntentConfigRoomBuilder::new()
    .role(IntentConfigRole::Database { config_file_path })
    .session_manager(session_manager.clone())
    .build();

let memdb_room = MemDBRoomBuilder::new()
    .role(MemDBRole::Database { storage_path })
    .session_manager(session_manager.clone())
    .build();

// TCP server using transport layer directly
let server = TcpTransportServer::new()
    .bind(&bind_addr)
    .with_tls(tls_config)
    .start();

// Connection handling gives connections to SessionManager
server.on_connection(|transport| {
    session_manager.handle_connection(transport);
});
```

### Collector network code (REWRITE)

**Similar pattern - needs same migration**

---

## 4. Unnecessary Transport Abstractions (EVALUATE)

### Potential over-abstractions created by AI

**Need to check**:
- Are there extra wrapper layers not in vision?
- Are there "helper" modules that bypass proper patterns?
- Are there custom message routers that duplicate SessionManager?

**Action**: Review and simplify

---

## 5. Documentation Created for Wrong Patterns (UPDATE)

### Documents that describe zznet-builder pattern

**Need to update/deprecate**:
- Any docs describing `RoomHandlerFactory`
- Any guides on implementing custom `RoomHandle`
- Any examples using `ServerBuilder`/`ClientBuilder`

**Replace with**:
- Component builder pattern examples
- Room<T> usage guide
- SessionManager integration guide

---

## Migration Timeline

### Phase 3a: Migrate Database Application (3-4 days)
1. **Day 1**: Remove zznet-builder dependency, add zznet-room
2. **Day 1-2**: Rewrite network.rs to use proper pattern
3. **Day 2**: Delete room_handlers.rs
4. **Day 3**: Test with real connections
5. **Day 4**: Verify all functionality works

### Phase 3b: Migrate Collector Application (2-3 days)
1. **Day 1**: Remove zznet-builder dependency, add zznet-room
2. **Day 1-2**: Rewrite network code
3. **Day 2**: Delete room_handlers.rs
4. **Day 3**: Test with database connection

### Phase 3c: Deprecate zznet-builder (1 day)
1. Add deprecation warnings to all public APIs
2. Update Cargo.toml with deprecation notice
3. Document migration path in README
4. Plan for removal in future version

### Phase 3d: Cleanup (1 day)
1. Remove deprecated documentation
2. Update all examples to use proper pattern
3. Update RUNBOOK.md
4. Update ARCHITECTURE diagrams

**Total estimated time**: 7-9 days

---

## Success Criteria

### Code Metrics
- ❌ Zero uses of `zznet-builder` in applications
- ❌ Zero `RoomHandlerFactory` implementations
- ❌ Zero `RoomHandle` implementations in apps
- ✅ All components use builder pattern
- ✅ All rooms use `Room<T>` with `TypedSender<T>`
- ✅ SessionManager is the ONLY message router

### Functional Requirements
- ✅ All tests pass
- ✅ Database accepts collector connections
- ✅ Messages flow end-to-end
- ✅ TLS works correctly
- ✅ Reconnection works

### Documentation
- ✅ No references to deprecated patterns
- ✅ Examples show proper builder usage
- ✅ Architecture docs match implementation

---

## Risk Assessment

### High Risk Items
1. **TLS integration**: Need to ensure transport layer still handles TLS properly
2. **Connection lifecycle**: SessionManager needs proper connection/disconnection handling
3. **Room negotiation**: HELLO protocol integration with SessionManager

### Mitigation
- Test each piece incrementally
- Keep old code until new code proven
- Extensive integration testing
- Manual verification with real connections

---

## Questions to Resolve

1. **Does zznet-transport-tcp provide server capability?**
   - If NO: Need to implement TCP server in transport layer
   - If YES: Can directly use it

2. **How does HELLO integrate with SessionManager?**
   - Need clear handoff point from HELLO to SessionManager
   - Document the connection lifecycle

3. **Where does TLS configuration belong?**
   - Transport layer? (likely)
   - Application layer? (no, should be abstracted)

4. **What about ConnectionManager actor?**
   - Is this also AI-generated? (need to check)
   - Is it in the vision? (need to verify)

---

## Next Steps

1. ✅ Document what needs deprecation (this document)
2. ⏸️ Answer open questions above
3. ⏸️ Create detailed Phase 3 implementation plan
4. ⏸️ Begin migration with database app
5. ⏸️ Migrate collector app
6. ⏸️ Deprecate zznet-builder
7. ⏸️ Final cleanup

---

**Status**: Awaiting confirmation before proceeding with migration
