# Understanding: Network Layer Integration Flow

**Date**: Oct 25, 2025
**Purpose**: Document how HELLO, SessionManager, and Components integrate
**Based on**: Code review of zznet-hello and zznet-session

---

## The Connection Flow

### Phase 1: Transport Connection
```
TCP/TLS Connection Established
    ↓
TcpTransportServer.accept() returns TransportConnection
    ↓
Application spawns handler for this connection
```

### Phase 2: HELLO Handshake
```
HelloActor created with:
- TransportConnection
- HelloConfig (offered_rooms, auth_role)
- Optional: ConnectionManager address

HelloActor runs state machine:
1. Send HELLO frame (version, role, offered_rooms)
2. Receive HELLO frame from peer
3. Validate protocol version
4. Authorize peer (via authorizer callback)
5. Compute room intersection
6. If intersection empty → disconnect
7. If success → send HandshakeComplete to ConnectionManager
```

### Phase 3: SessionManager Integration
```
ConnectionManager receives HandshakeComplete:
- peer_id: String
- role: TRole
- negotiated_rooms: Vec<RoomId>
- outbound_tx: mpsc sender for room messages

ConnectionManager calls:
session_manager.add_peer(peer_id, negotiated_rooms, outbound_tx)

This creates PeerSession in SessionManager
```

### Phase 4: Room Handler Wiring (CURRENT WRONG PATTERN)
```
ConnectionManager calls room_handler_wirer callback:
- Application wires RoomHandlerFactory to peer
- This is the AI-GENERATED pattern we're REMOVING

PROPER PATTERN (after Phase 3):
- Components already registered with SessionManager via builders
- No wiring needed - automatic!
```

### Phase 5: Message Flow
```
Inbound (Network → Component):
Transport → HelloActor → SessionManager → Component's Room<T> → Actor

Outbound (Component → Network):
Actor → Room<T> → SessionManager → ConnectionManager → HelloActor → Transport
```

---

## Key Components

### ConnectionManager
**Role**: Coordinates multiple HelloActors and SessionManager
**Pattern**: One ConnectionManager per process (singleton)
**Lifecycle**: Lives for entire application runtime

**Key methods**:
- `new_with_session_manager(session_manager, authorizer)`: Proper constructor
- `with_room_handler_wirer(callback)`: Sets wiring callback (WE'RE REMOVING THIS)
- Handles `HandshakeComplete` message from HelloActor
- Spawns new HelloActor for each connection

### HelloActor
**Role**: Runs HELLO handshake, then routes messages
**Pattern**: One HelloActor per connection
**Lifecycle**: Lives for duration of connection

**Key methods**:
- `new(transport, config)`: Create for one connection
- `with_session_manager(addr)`: Sets ConnectionManager address
- Runs state machine for handshake
- Sends `HandshakeComplete` when done
- Routes room messages bidirectionally

### SessionManager
**Role**: Transport-agnostic message routing
**Pattern**: One SessionManager per process (shared via Arc<Mutex<>>)
**Lifecycle**: Lives for entire application runtime

**Key methods**:
- `add_peer(peer_id, rooms, outbound_tx)`: Called by ConnectionManager
- `send_to_room(peer_id, room_id, message)`: Called by components
- `broadcast_to_room(room_id, message)`: Called by components
- `peer_connected()`, `peer_disconnected()`: Lifecycle events

### Room<T>
**Role**: Typed bidirectional channel for one room on one connection
**Pattern**: Created by component builders
**Lifecycle**: Lives with component actor

**Key aspects**:
- Uses TypedSender<T> for automatic serialization
- Registers with SessionManager (or should!)
- Receives SessionEvent::Active/Inactive

---

## The Problem: room_handler_wirer

### Current (Wrong) Pattern

Applications provide a callback to wire RoomHandlerFactory:

```rust
let wirer = Arc::new(move |sm, peer_id| {
    Box::pin(async move {
        // Create factory for each component
        let intent_factory = IntentConfigRoomHandlerFactory::new(...);
        let memdb_factory = MemDBRoomHandlerFactory::new(...);

        // Register with peer session
        sm.lock().await.register_handlers(peer_id, ...);

        Ok(())
    })
});

connection_manager.with_room_handler_wirer(wirer);
```

**Problems**:
- ❌ Manual wiring per connection
- ❌ Application has to implement factories
- ❌ Bypasses Room<T> infrastructure
- ❌ Requires locking SessionManager

### Proper (Vision) Pattern

Components auto-register during builder:

```rust
// In application startup (once):
let session_manager = Arc::new(tokio::sync::Mutex::new(
    SessionManager::new()
));

// Each component registers itself
let intent_room = IntentConfigRoomBuilder::new()
    .role(IntentConfigRole::Database { ... })
    .session_manager(session_manager.clone())
    .build();  // ← Auto-registers with SessionManager

// NO room_handler_wirer callback needed!
```

**Advantages**:
- ✅ Zero wiring code
- ✅ Components self-register
- ✅ Works via Room<T> pattern
- ✅ No per-connection overhead

---

## What Needs to Change

### 1. Remove room_handler_wirer Usage
**Where**: All applications (database, collector)
**Action**: Delete the wiring callback code

### 2. Use SessionManager Directly
**Where**: Component builders
**Action**: Already have `.with_session_manager()` - just use it properly

### 3. ConnectionManager Simplification
**Where**: zznet-hello crate
**Action**: Mark `with_room_handler_wirer` as deprecated
**Future**: Remove after Phase 3 complete

### 4. Application Startup Pattern
**Old**:
```rust
// Create components
let intent = IntentConfigBuilder::new().start()?;

// Create ConnectionManager with wiring callback
let cm = ConnectionManager::new_with_session_manager(sm, auth)
    .with_room_handler_wirer(wiring_callback);  // ← DELETE THIS

// Start server
server.accept_connections(cm);
```

**New**:
```rust
// Create SessionManager FIRST
let sm = Arc::new(tokio::sync::Mutex::new(SessionManager::new()));

// Create components WITH SessionManager
let intent = IntentConfigRoomBuilder::new()
    .session_manager(sm.clone())
    .build();  // ← Auto-registers

// Create ConnectionManager WITHOUT wiring callback
let cm = ConnectionManager::new_with_session_manager(sm.clone(), auth);
// NO .with_room_handler_wirer() call!

// Start server
server.accept_connections(cm);
```

---

## Migration Path for Phase 3

### Step 1: Create SessionManager Early
Move SessionManager creation to before components

### Step 2: Update Component Builders
Change from creating actors to creating rooms with SessionManager

### Step 3: Remove room_handler_wirer
Delete the wiring callback entirely

### Step 4: Simplify ConnectionManager Usage
Just pass SessionManager, no wiring

### Step 5: Delete RoomHandlerFactory Code
Remove all factory implementations (~450 lines)

---

## Open Questions ANSWERED

### Q1: How does HELLO integrate with SessionManager?
**A**: HelloActor sends HandshakeComplete to ConnectionManager, which calls `session_manager.add_peer()`

### Q2: Where does room negotiation happen?
**A**: In HELLO handshake (phase 2). Rooms are computed via intersection, then passed to SessionManager in HandshakeComplete.

### Q3: How do components get SessionEvent notifications?
**A**: Via Room<T> which is registered with SessionManager. SessionManager broadcasts events to all registered rooms.

### Q4: What about ConnectionManager actor?
**A**: It's NEEDED but the `room_handler_wirer` callback is NOT. ConnectionManager stays, wiring callback goes.

---

## Summary

**The pattern is**:
1. Create SessionManager (shared)
2. Create components with .session_manager() (auto-register)
3. Create ConnectionManager with SessionManager (no wiring callback)
4. Accept connections → HelloActor → HandshakeComplete → SessionManager.add_peer()
5. Messages flow automatically via registered rooms

**What we remove**:
- room_handler_wirer callback
- RoomHandlerFactory implementations
- Manual per-connection wiring

**Result**: ~450 lines deleted, clean architecture aligned with vision! ✅
