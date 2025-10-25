# Deep Investigation: SessionManager Actor Migration

**Date**: October 25, 2025
**Status**: Comprehensive Analysis
**Goal**: Understand the complete path from Arc<Mutex<>> to Actor pattern

---

## Table of Contents

1. [Current State Analysis](#current-state-analysis)
2. [SessionManager API Surface](#sessionmanager-api-surface)
3. [All Usage Locations](#all-usage-locations)
4. [Actor Design](#actor-design)
5. [Migration Path](#migration-path)
6. [Complexity Analysis](#complexity-analysis)
7. [Risk Assessment](#risk-assessment)
8. [Decision Tree](#decision-tree)

---

## Current State Analysis

### SessionManager Structure

```rust
// src/net/zznet-session/src/session_manager.rs
pub struct SessionManager<TRole: ApplicationRole> {
    peers: HashMap<PeerId, PeerSession<TRole>>,
    offered_rooms: Vec<RoomId>,
    max_peers: Option<usize>,
    max_rooms_per_peer: Option<usize>,
    room_handlers: HashMap<RoomId, (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>)>,
}
```

**Current Pattern**: Plain Rust struct, NOT an Actix actor

**Sharing Mechanism**: `Arc<tokio::sync::Mutex<SessionManager<TRole>>>`
- Used in: ConnectionManager, Components (IntentConfig, MemDB, CState), Applications

### Why Arc<Mutex<>> Was Chosen

Looking at the code comments:

```rust
// From connection_manager.rs line 44-50:
/// DESIGN: Wrapped in Arc<Mutex<...>> to enable sharing with components.
/// - ConnectionManager needs &mut access for add_peer(), connect_peer()
/// - Components need &self access for broadcast_to_room()
/// - Arc<Mutex<...>> provides interior mutability for both use cases
/// - All share the SAME SessionManager instance
/// - This allows messages to flow: Network → SessionManager ← Components
session_manager: Arc<Mutex<SessionManager<TRole>>>,
```

**Reasoning**:
1. Multiple owners need access (ConnectionManager + multiple components)
2. Some operations need &mut (add_peer, connect_peer)
3. Some operations need &self (send_to_room, get_peer_role)
4. Arc enables shared ownership
5. Mutex enables interior mutability

---

## SessionManager API Surface

### Complete Method Inventory

#### Constructors
```rust
pub fn new(offered_rooms: Vec<RoomId>) -> Self
pub fn new_with_limits(offered_rooms, max_peers, max_rooms_per_peer) -> Self
```

#### Peer Management (&mut self)
```rust
pub fn add_peer(&mut self, peer_id, peer_session) -> Result<(), SessionError>
pub async fn connect_peer(&mut self, peer_id, outbound_tx, inbound_rx) -> Result<(), SessionError>
pub fn disconnect_peer(&mut self, peer_id) -> Result<(), SessionError>
pub fn remove_peer(&mut self, peer_id) -> Result<(), SessionError>
pub async fn add_room_to_peer(&mut self, peer_id, room_id, room_handle) -> Result<(), SessionError>
```

#### Room Management (&mut self)
```rust
pub fn set_offered_rooms(&mut self, rooms: Vec<RoomId>)
pub fn handle_publish_rooms(&mut self, peer_id, requested_rooms) -> Result<Vec<RoomId>, SessionError>
```

#### Query Operations (&self)
```rust
pub fn peer_state(&self, peer_id) -> Option<ConnectionState>
pub fn is_peer_connected(&self, peer_id) -> bool
pub fn peer_ids(&self) -> Vec<PeerId>
pub fn get_peer_role(&self, peer_id) -> Option<&TRole>
pub fn get_peer_role_cloned(&self, peer_id) -> Option<TRole>
pub fn get_peer_identity(&self, peer_id) -> Option<&PeerIdentity>
pub fn peers_with_role(&self, role) -> Vec<PeerId>
pub fn connected_peer_count(&self) -> usize
pub fn offered_rooms(&self) -> &[RoomId]
pub fn get_peer_sender(&self, peer_id) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>>
pub fn subscribe_peer_inbound(&self, peer_id) -> Result<mpsc::Receiver<(RoomId, Vec<u8>)>, SessionError>
pub fn peer_joined_rooms(&self, peer_id) -> Result<&[RoomId], SessionError>
pub fn is_room_joined_with_peer(&self, peer_id, room_id) -> bool
```

#### Message Operations (async)
```rust
pub async fn send_to_room(&self, peer_id, room_id, bytes: Vec<u8>) -> Result<(), SessionError>
```

**Total Methods**: 25 public methods
- **Mutating**: 8 methods (&mut self)
- **Query**: 16 methods (&self)
- **Async**: 3 methods

---

## All Usage Locations

### 1. ConnectionManager (zznet-hello)

**File**: `src/net/zznet-hello/src/connection_manager.rs`

**Usage**: Primary owner and coordinator

```rust
pub struct ConnectionManager<TRole> {
    session_manager: Arc<Mutex<SessionManager<TRole>>>,
    hello_actors: HashMap<PeerId, Addr<HelloActor>>,
    // ...
}
```

**Operations Used**:
- `add_peer()` - When handshake completes
- `connect_peer()` - Wiring transport channels
- `disconnect_peer()` - On connection drop
- Passes Arc<Mutex<>> to room_handler_wirer callback
- Passes Arc<Mutex<>> to HelloActor instances

**Lock Count**: ~10 locks per connection lifecycle

---

### 2. IntentConfigActor (Component)

**File**: `src/components/zzintent-config/src/actor.rs`

**Usage**: Broadcasting config updates to collectors

```rust
pub struct IntentConfigActor<T: ApplicationRole> {
    session_manager: Option<Arc<Mutex<SessionManager<PermissionWrapper<T>>>>>,
    // ...
}
```

**Operations Used**:
- `peers_with_role()` - Find collectors to broadcast to
- `send_to_room()` - Send ConfigUpdate to each peer

**Lock Pattern**:
```rust
// Line 165-200: Broadcast pattern
let peers = session_manager.lock().unwrap().peers_with_role(&role);
for peer_id in peers {
    session_manager.lock().unwrap().send_to_room(&peer_id, &room_id, bytes).await?;
}
```

**Lock Count**: 1 + N locks (N = number of collector peers)

**Issue**: `#[allow(clippy::await_holding_lock)]` - holds lock across await

---

### 3. MemDBActor (Component)

**File**: `src/components/zzmem-db/src/actor.rs`

**Usage**: Sending batch data to specific peers

```rust
pub struct MemDBActor<T: ApplicationRole> {
    session_manager: Option<Arc<Mutex<SessionManager<PermissionWrapper<T>>>>>,
    // ...
}
```

**Operations Used**:
- `is_room_joined_with_peer()` - Check if peer joined memdb room
- `get_peer_sender()` - Get channel to send to specific peer

**Lock Pattern**:
```rust
// Line 223-251: Direct peer sending
let sm_lock = session_manager.lock().unwrap();
let joined = sm_lock.is_room_joined_with_peer(&peer_id, &memdb_room);
if joined {
    let sender = sm_lock.get_peer_sender(&peer_id).unwrap();
    // Use sender...
}
```

**Lock Count**: 1 long-held lock per batch send

---

### 4. CStateActor (Component)

**File**: `src/components/zzcollector-state/src/actor.rs`

**Usage**: Minimal - primarily uses Room<T>

```rust
// Via builder only:
pub struct CStateBuilder<TRole> {
    session_manager: Option<Arc<Mutex<SessionManager<TRole>>>>,
    // ...
}
```

**Operations Used**: None directly (uses Room<T> instead)

**Status**: ✅ Best practice - already migrated away from direct SessionManager access

---

### 5. Applications (Database & Collector)

**File**: `src/apps/zzping-database/src/service.rs`

**Usage**: Creating and distributing SessionManager

```rust
pub struct StartedComponents {
    session_manager: Arc<tokio::sync::Mutex<SessionManager<AuthRole>>>,
    // ...
}
```

**Operations**:
- Creates SessionManager once
- Clones Arc<Mutex<>> for each component
- Passes to ConnectionManager

**Lock Count**: 0 (just passes references)

---

## Actor Design

### Message Types Needed

Based on the 25 methods, we need message types for each operation:

#### Peer Management Messages
```rust
#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct AddPeer<TRole: ApplicationRole> {
    pub peer_id: PeerId,
    pub peer_session: PeerSession<TRole>,
}

#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct ConnectPeer {
    pub peer_id: PeerId,
    pub outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    pub inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
}

#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct DisconnectPeer {
    pub peer_id: PeerId,
}

#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct RemovePeer {
    pub peer_id: PeerId,
}

#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct AddRoomToPeer {
    pub peer_id: PeerId,
    pub room_id: RoomId,
    pub room_handle: Box<dyn RoomHandle>,
}
```

#### Room Management Messages
```rust
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetOfferedRooms {
    pub rooms: Vec<RoomId>,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<RoomId>, SessionError>")]
pub struct HandlePublishRooms {
    pub peer_id: PeerId,
    pub requested_rooms: Vec<RoomId>,
}
```

#### Query Messages
```rust
#[derive(Message)]
#[rtype(result = "Option<ConnectionState>")]
pub struct GetPeerState {
    pub peer_id: PeerId,
}

#[derive(Message)]
#[rtype(result = "bool")]
pub struct IsPeerConnected {
    pub peer_id: PeerId,
}

#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeerIds;

#[derive(Message)]
#[rtype(result = "Option<TRole>")]
pub struct GetPeerRole<TRole: ApplicationRole> {
    pub peer_id: PeerId,
}

#[derive(Message)]
#[rtype(result = "Vec<PeerId>")]
pub struct GetPeersWithRole<TRole: ApplicationRole> {
    pub role: TRole,
}

#[derive(Message)]
#[rtype(result = "usize")]
pub struct GetConnectedPeerCount;

#[derive(Message)]
#[rtype(result = "Vec<RoomId>")]
pub struct GetOfferedRooms;

#[derive(Message)]
#[rtype(result = "Option<mpsc::Sender<(RoomId, Vec<u8>)>>")]
pub struct GetPeerSender {
    pub peer_id: PeerId,
}

#[derive(Message)]
#[rtype(result = "Result<mpsc::Receiver<(RoomId, Vec<u8>)>, SessionError>")]
pub struct SubscribePeerInbound {
    pub peer_id: PeerId,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<RoomId>, SessionError>")]
pub struct GetPeerJoinedRooms {
    pub peer_id: PeerId,
}

#[derive(Message)]
#[rtype(result = "bool")]
pub struct IsRoomJoinedWithPeer {
    pub peer_id: PeerId,
    pub room_id: RoomId,
}
```

#### Message Sending
```rust
#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct SendToRoom {
    pub peer_id: PeerId,
    pub room_id: RoomId,
    pub bytes: Vec<u8>,
}
```

**Total Message Types**: 20 messages

---

### Actor Implementation

```rust
// src/net/zznet-session/src/session_manager.rs

use actix::prelude::*;

impl<TRole> Actor for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("SessionManager actor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("SessionManager actor stopped");
    }
}

// Handler implementations (example for AddPeer)
impl<TRole> Handler<AddPeer<TRole>> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = Result<(), SessionError>;

    fn handle(&mut self, msg: AddPeer<TRole>, _ctx: &mut Context<Self>) -> Self::Result {
        self.add_peer(msg.peer_id, msg.peer_session)
    }
}

// Handler for SendToRoom (async)
impl<TRole> Handler<SendToRoom> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,
{
    type Result = ResponseFuture<Result<(), SessionError>>;

    fn handle(&mut self, msg: SendToRoom, _ctx: &mut Context<Self>) -> Self::Result {
        let fut = self.send_to_room(&msg.peer_id, &msg.room_id, msg.bytes);
        Box::pin(fut)
    }
}

// ... 18 more handler implementations
```

---

## Migration Path

### Phase 1: Add Actor Implementation (No Breaking Changes)

**Goal**: Make SessionManager an actor WITHOUT changing existing Arc<Mutex<>> usage

**Steps**:
1. Implement `Actor` trait for SessionManager
2. Define all 20 message types
3. Implement all 20 handlers
4. Add tests for actor behavior
5. Keep all existing public methods (for Arc<Mutex<>> users)

**Result**: SessionManager can be used BOTH ways:
- As Arc<Mutex<SessionManager>> (current)
- As Addr<SessionManager> (new)

**Breaking**: NO - fully backwards compatible

**Effort**: 2-3 days

---

### Phase 2: Migrate ConnectionManager

**Goal**: ConnectionManager uses Addr<SessionManager> instead of Arc<Mutex<>>

**Changes**:
```rust
// Before
pub struct ConnectionManager<TRole> {
    session_manager: Arc<Mutex<SessionManager<TRole>>>,
    // ...
}

// After
pub struct ConnectionManager<TRole> {
    session_manager: Addr<SessionManager<TRole>>,
    // ...
}
```

**All ConnectionManager operations become message sends**:
```rust
// Before
let mut sm = self.session_manager.lock().await;
sm.add_peer(peer_id, peer_session)?;

// After
self.session_manager.send(AddPeer {
    peer_id,
    peer_session,
}).await??;
```

**Ripple Effects**:
- HelloActor would receive Addr<SessionManager>
- room_handler_wirer callback signature changes
- Applications pass Addr<> instead of Arc<Mutex<>>

**Breaking**: YES - API change for ConnectionManager constructors

**Effort**: 3-4 days

---

### Phase 3: Migrate IntentConfigActor

**Goal**: IntentConfig uses Addr<SessionManager> for broadcasts

**Changes**:
```rust
// Before
pub struct IntentConfigActor<T> {
    session_manager: Option<Arc<Mutex<SessionManager<PermissionWrapper<T>>>>>,
    // ...
}

// After
pub struct IntentConfigActor<T> {
    session_manager: Option<Addr<SessionManager<PermissionWrapper<T>>>>,
    // ...
}
```

**Broadcast pattern becomes**:
```rust
// Before (with lock)
let peers = session_manager.lock().unwrap().peers_with_role(&role);
for peer_id in peers {
    session_manager.lock().unwrap().send_to_room(&peer_id, &room_id, bytes).await?;
}

// After (message passing)
let peers = session_manager.send(GetPeersWithRole { role }).await?;
for peer_id in peers {
    session_manager.send(SendToRoom {
        peer_id,
        room_id: room_id.clone(),
        bytes: bytes.clone(),
    }).await??;
}
```

**Benefits**:
- ✅ No more `#[allow(clippy::await_holding_lock)]`
- ✅ No lock contention
- ✅ Type-safe message passing

**Breaking**: YES - Component builder API changes

**Effort**: 2-3 days

---

### Phase 4: Migrate MemDBActor

**Goal**: MemDB uses Addr<SessionManager>

**Similar changes to IntentConfig**

**Effort**: 2-3 days

---

### Phase 5: Update Applications

**Goal**: Applications create and distribute Addr<SessionManager>

**Changes**:
```rust
// Before
let session_manager = Arc::new(Mutex::new(SessionManager::new(rooms)));
let cm = ConnectionManager::new_with_session_manager(Arc::clone(&session_manager), auth);

// After
let session_manager = SessionManager::new(rooms).start();
let cm = ConnectionManager::new_with_session_manager(session_manager.clone(), auth);
```

**Effort**: 1-2 days

---

### Phase 6: Remove Arc<Mutex<>> Support (Breaking)

**Goal**: Clean up - remove old Arc<Mutex<>> compatibility

**Changes**:
- Remove all `pub fn` methods from SessionManager (only handlers remain)
- Force all access through message passing
- Update documentation

**Breaking**: YES - removes backwards compatibility

**Effort**: 1 day

---

## Complexity Analysis

### Code Changes Required

| Component | Files | LOC Changes | Complexity |
|-----------|-------|-------------|------------|
| SessionManager (Phase 1) | 2 | +500 | High (20 handlers) |
| ConnectionManager (Phase 2) | 3 | +200, -100 | Medium |
| IntentConfigActor (Phase 3) | 4 | +150, -50 | Medium |
| MemDBActor (Phase 4) | 4 | +150, -50 | Medium |
| Applications (Phase 5) | 4 | +100, -100 | Low |
| Cleanup (Phase 6) | 2 | -500 | Low |
| **Total** | **19** | **+1100, -800** | **High** |

**Net Change**: +300 LOC (more explicit message types)

---

### Testing Requirements

#### Unit Tests Needed
- 20 message handler tests (one per message type)
- Actor lifecycle tests (start, stop, restart)
- Error propagation tests
- Concurrent message tests

**Estimate**: 30-40 new tests

#### Integration Tests Needed
- Full connection lifecycle with actor
- Multi-peer broadcast scenarios
- Component-to-SessionManager message flow
- Stress test with many concurrent messages

**Estimate**: 10-15 integration tests

**Total Test Effort**: 3-4 days

---

## Risk Assessment

### High Risks

#### 1. Message Passing Overhead
**Risk**: Actor mailbox overhead could slow down message routing

**Mitigation**:
- Benchmark before/after
- Profile under load
- Actor mailbox is highly optimized (likely negligible)

**Likelihood**: Low
**Impact**: Medium

#### 2. Complex Async Interactions
**Risk**: Message ordering and async timing issues

**Example**: Broadcast requires querying peers then sending N messages
```rust
// Must happen sequentially:
let peers = sm.send(GetPeersWithRole).await?;  // Message 1
for peer in peers {
    sm.send(SendToRoom).await?;  // N messages
}
```

**Mitigation**:
- Careful sequencing in handlers
- Comprehensive integration tests
- Add batch broadcast message type

**Likelihood**: Medium
**Impact**: High

#### 3. Backwards Compatibility Breakage
**Risk**: Existing code breaks during migration

**Mitigation**:
- Phased approach (Phase 1 is non-breaking)
- Keep Arc<Mutex<>> support initially
- Thorough testing at each phase

**Likelihood**: High (intentional)
**Impact**: High

### Medium Risks

#### 4. Generic Type Complexity
**Risk**: Actor + generic TRole creates complex type constraints

**Example**:
```rust
impl<TRole> Handler<AddPeer<TRole>> for SessionManager<TRole>
where
    TRole: ApplicationRole + 'static,  // Need 'static for Actor
{
    // ...
}
```

**Mitigation**:
- Document trait bounds clearly
- Use type aliases where possible
- Add examples in docs

**Likelihood**: Medium
**Impact**: Low

#### 5. Error Handling Changes
**Risk**: Double-unwrap pattern with actor results

**Example**:
```rust
// Actor returns Result<Result<(), Error>, MailboxError>
let result = sm.send(msg).await??;  // Double ?
```

**Mitigation**:
- Helper methods for common patterns
- Clear documentation
- Possibly flatten to single Result

**Likelihood**: High
**Impact**: Low

---

## Decision Tree

### Question 1: Should we migrate at all?

**YES if**:
- ✅ Want pure actor model architecture
- ✅ Have 3-4 weeks for migration
- ✅ Lock contention is measurable issue
- ✅ Want better testability

**NO if**:
- ❌ Current system works well
- ❌ Limited development bandwidth
- ❌ Higher priority features exist
- ❌ Risk tolerance is low

### Question 2: If YES, what approach?

#### Option A: Full Migration (Recommended)
**Phases**: 1-6 (all phases)
**Timeline**: 3-4 weeks
**Result**: Pure actor model, no Arc<Mutex<>>
**Risk**: High

#### Option B: Hybrid Approach
**Phases**: 1-2 only (SessionManager as actor, ConnectionManager migrated)
**Timeline**: 1-2 weeks
**Result**: Core network layer as actor, components keep Arc<Mutex<>>
**Risk**: Medium

#### Option C: Incremental
**Phases**: 1, then pause and evaluate
**Timeline**: 3-4 days
**Result**: Actor available, but Arc<Mutex<>> still works
**Risk**: Low

---

## Recommendation Matrix

### For Production System (Current State)

**Recommendation**: **Option C: Incremental**

**Reasoning**:
1. System is stable and functional
2. Lock contention not proven to be issue
3. Limited immediate benefit
4. High risk for low proven ROI

**Next Steps**:
1. Implement Phase 1 (actor interface)
2. Benchmark actor vs mutex
3. Profile under realistic load
4. Decide on Phases 2-6 based on data

---

### For New Feature Development

**Recommendation**: **Option A: Full Migration**

**Reasoning**:
1. Clean slate for architectural decisions
2. Establish pure actor pattern
3. Better long-term maintainability
4. Investment in architectural quality

**Timeline**: 3-4 weeks of focused work

---

### For Limited Bandwidth

**Recommendation**: **Accept current Arc<Mutex<>> pattern**

**Reasoning**:
1. Functional system
2. Migration doesn't add user features
3. Effort better spent elsewhere
4. Can revisit when bandwidth increases

**Action**: Document architectural debt, add to backlog

---

## Appendix: Code Examples

### Example 1: Current Broadcast Pattern (with lock)

```rust
// src/components/zzintent-config/src/actor.rs
fn send_config_update_to_peers_impl(&self, ctx: &mut Context<Self>) {
    if let Some(session_manager) = &self.session_manager {
        let msg = IntentConfigNetworkMsg::ConfigUpdate { /* ... */ };
        let bytes = bincode::serde::encode_to_vec(&msg, config)?;

        // Lock 1: Get peers
        let peers = session_manager.lock().unwrap().peers_with_role(&role);

        // Lock 2-N: Send to each peer
        ctx.spawn(async move {
            for peer_id in peers {
                session_manager.lock().unwrap()
                    .send_to_room(&peer_id, &room_id, bytes.clone())
                    .await?;
            }
        }.into_actor(self));
    }
}
```

**Lock Count**: 1 + N (N = number of peers)
**Issue**: Holds lock across await points

---

### Example 2: Actor Pattern (message passing)

```rust
// Future implementation with Addr<SessionManager>
async fn send_config_update_to_peers_impl(&self, ctx: &mut Context<Self>) {
    if let Some(session_manager) = &self.session_manager {
        let msg = IntentConfigNetworkMsg::ConfigUpdate { /* ... */ };
        let bytes = bincode::serde::encode_to_vec(&msg, config)?;

        // Message 1: Get peers
        let peers = session_manager.send(GetPeersWithRole { role }).await?;

        // Messages 2-N: Send to each peer
        ctx.spawn(async move {
            for peer_id in peers {
                session_manager.send(SendToRoom {
                    peer_id,
                    room_id: room_id.clone(),
                    bytes: bytes.clone(),
                }).await??;
            }
        }.into_actor(self));
    }
}
```

**Lock Count**: 0
**Messages**: 1 + N actor messages
**Benefits**: No locks, clean async, type-safe

---

### Example 3: Optimized Broadcast Message

```rust
// Could add specialized broadcast message to reduce overhead:

#[derive(Message)]
#[rtype(result = "Result<(), SessionError>")]
pub struct BroadcastToRole<TRole: ApplicationRole> {
    pub role: TRole,
    pub room_id: RoomId,
    pub bytes: Vec<u8>,
}

impl<TRole> Handler<BroadcastToRole<TRole>> for SessionManager<TRole> {
    type Result = ResponseFuture<Result<(), SessionError>>;

    fn handle(&mut self, msg: BroadcastToRole<TRole>, _ctx: &mut Context<Self>) -> Self::Result {
        let peers = self.peers_with_role(&msg.role);

        Box::pin(async move {
            for peer_id in peers {
                self.send_to_room(&peer_id, &msg.room_id, msg.bytes.clone()).await?;
            }
            Ok(())
        })
    }
}
```

**Benefits**: Single message for broadcast operation

---

## Conclusion

Converting SessionManager to an actor is **architecturally desirable** but **not critical**. The current Arc<Mutex<>> pattern works and is well-documented.

**Key Decision Factors**:
1. **Bandwidth**: Do you have 3-4 weeks for this?
2. **Priority**: Is architectural purity more important than features?
3. **Evidence**: Is lock contention a measured problem?

**Recommended Path**:
- **Short term**: Accept Arc<Mutex<>>, document debt
- **Medium term**: Implement Phase 1 (actor interface), gather metrics
- **Long term**: Full migration when bandwidth allows

**This is a refactoring investment, not a bug fix.**

---

**Status**: Analysis complete
**Next**: Decision by project owner
**Effort if YES**: 3-4 weeks
**Effort if NO**: 0 (document only)
