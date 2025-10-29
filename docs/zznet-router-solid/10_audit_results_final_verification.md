# Final Verification Audit Report: Deviation Catalog

**Date:** October 29, 2025
**Auditor:** GitHub Copilot
**Objective:** Produce a definitive list of all code locations that deviate from the "Ground Truth 3.0" vision

---

## Task 1: Catalog all usage of `Box<dyn RoomHandle>`

### Objective
Find every instance where the incorrect trait-object wiring is used instead of the actor `Recipient` pattern.

### Methodology
Performed project-wide search for the exact string `Box<dyn RoomHandle>` and cataloged all occurrences in source code (excluding documentation files).

### Results: Deviations from Principle #6: Trait Object Wiring

#### 1. **Type Alias Definition**

**File:** `/home/deavid/git/rust/zzping/src/net/zznet-router/src/peer_channels.rs`
**Line:** 9
**Code:**
```rust
type SessionRooms = Arc<TokioMutex<HashMap<RoomId, Box<dyn RoomHandle>>>>;
```
**Context:** Type alias for storing rooms in a peer's session
**Severity:** High - Defines the fundamental storage pattern for room connections

---

#### 2. **PeerChannelsBuilder Struct Field**

**File:** `/home/deavid/git/rust/zzping/src/net/zznet-router/src/peer_channels.rs`
**Line:** 16
**Code:**
```rust
pub struct PeerChannelsBuilder {
    peer_id: PeerId,
    rooms: HashMap<RoomId, Box<dyn RoomHandle>>,
}
```
**Context:** Builder pattern stores rooms as trait objects before building PeerChannels
**Severity:** High - Builder accumulates rooms using the wrong pattern

---

#### 3. **PeerChannelsBuilder::add_room Method Parameter**

**File:** `/home/deavid/git/rust/zzping/src/net/zznet-router/src/peer_channels.rs`
**Line:** 32
**Code:**
```rust
pub fn add_room(
    &mut self,
    room_id: RoomId,
    room: Box<dyn RoomHandle>,
) -> Result<(), SessionError>
```
**Context:** Function argument accepting trait object instead of Recipient
**Severity:** High - Public API enforces trait object pattern

---

#### 4. **RoomHandle Trait Documentation**

**File:** `/home/deavid/git/rust/zzping/src/net/zznet-room/src/room_adapter.rs`
**Line:** 14
**Code:**
```rust
/// `Box<dyn RoomHandle>` in peer connection state.
```
**Context:** Documentation comment describing the trait's intended use
**Severity:** Low - Documentation only, but reflects design intent

---

#### 5. **RoomManager Trait Return Type**

**File:** `/home/deavid/git/rust/zzping/src/net/zznet-room/src/room_manager.rs`
**Line:** 58
**Code:**
```rust
async fn create_for_peer(
    &self,
    peer_id: PeerId,
    permission: Permission,
    room_id: &RoomId,
) -> Result<Option<Box<dyn RoomHandle>>, CreateError>;
```
**Context:** Trait method return type
**Severity:** Critical - The factory pattern returns trait objects, enforcing this throughout all implementations

---

#### 6. **CreateRoomForPeer Message Type**

**File:** `/home/deavid/git/rust/zzping/src/net/zznet-room/src/room_manager.rs`
**Line:** 65
**Code:**
```rust
#[derive(actix::Message)]
#[rtype(result = "Result<Option<Box<dyn RoomHandle>>, CreateError>")]
pub struct CreateRoomForPeer {
    pub peer_id: PeerId,
    pub permission: Permission,
    pub room_id: RoomId,
}
```
**Context:** Actix message type declaration
**Severity:** Critical - Actor message infrastructure built around trait objects

---

### Summary: Trait Object Usage

**Total Occurrences in Source Code:** 6 locations

**Files Affected:**
1. `/home/deavid/git/rust/zzping/src/net/zznet-router/src/peer_channels.rs` (3 occurrences)
2. `/home/deavid/git/rust/zzping/src/net/zznet-room/src/room_adapter.rs` (1 occurrence)
3. `/home/deavid/git/rust/zzping/src/net/zznet-room/src/room_manager.rs` (2 occurrences)

**Key Insight:** The trait object pattern is deeply embedded in the architecture:
- **Storage layer:** `PeerChannels` uses `HashMap<RoomId, Box<dyn RoomHandle>>`
- **Factory pattern:** `RoomManager::create_for_peer()` returns `Box<dyn RoomHandle>`
- **Builder pattern:** `PeerChannelsBuilder` accumulates trait objects
- **Message system:** Actix messages carry trait objects as payload

**Refactoring Scope:** To eliminate this pattern, the following must be changed:
1. `RoomManager` trait to return `Recipient<RoomMessage>` instead of `Box<dyn RoomHandle>`
2. `PeerChannels` internal storage to use `HashMap<RoomId, Recipient<...>>`
3. All component implementations of `RoomManager`
4. The `RoomHandle` trait itself should be deprecated/removed

---

## Task 2: Analyze RouterActor's Public API and Call Sites

### Objective
Verify the extent of the Router's incorrect runtime routing responsibilities and identify all code that uses this anti-pattern.

### Part A: Confirm Handler Implementations

**File:** `/home/deavid/git/rust/zzping/src/net/zznet-router/src/actor.rs`

#### SendToPeer Handler

**Lines:** 194-221
**Code:**
```rust
/// Send message to peer room
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SendToPeer {
    pub peer_id: PeerId,
    pub room_id: RoomId,
    pub bytes: Vec<u8>,
}

impl Handler<SendToPeer> for RouterActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: SendToPeer, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_id = msg.peer_id;
        let room_id = msg.room_id;
        let bytes = msg.bytes;

        Box::pin(async move {
            let router = router_arc.lock().await;
            router
                .send_to_room(&peer_id, &room_id, bytes)
                .await
                .map_err(|e| format!("Failed to send to peer: {:?}", e))
        })
    }
}
```

**Status:** ✅ **CONFIRMED** - Handler exists for runtime data routing

---

#### BroadcastToPeers Handler

**Lines:** 225-249
**Code:**
```rust
/// Broadcast to peers
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct BroadcastToPeers {
    pub peer_ids: Vec<PeerId>,
    pub room_id: RoomId,
    pub bytes: Vec<u8>,
}

impl Handler<BroadcastToPeers> for RouterActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: BroadcastToPeers, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        let peer_ids = msg.peer_ids;
        let room_id = msg.room_id;
        let bytes = msg.bytes;

        Box::pin(async move {
            let router = router_arc.lock().await;
            router
                .broadcast_to_peers(&peer_ids, &room_id, bytes)
                .await
                .map_err(|e| format!("Failed to broadcast: {:?}", e))
        })
    }
}
```

**Status:** ✅ **CONFIRMED** - Handler exists for runtime broadcast routing

---

### Part B: Find Call Sites

**Methodology:** Searched project-wide for:
- `.send(SendToPeer`
- `.send(BroadcastToPeers`
- `SendToPeer {`
- `BroadcastToPeers {`

### Results: Deviations from Principle #2: Router's Runtime Role

#### Call Site Analysis

**Active Component Usage:** **NONE FOUND**

The search revealed that `SendToPeer` and `BroadcastToPeers` are:
1. **Defined** in `/home/deavid/git/rust/zzping/src/net/zznet-router/src/actor.rs`
2. **Used in tests** in `/home/deavid/git/rust/zzping/src/net/zznet-router/tests/integration_tests.rs`
3. **Documented** in design documents

**But NOT used by any production components.**

---

#### Test Usage (Integration Tests)

**File:** `/home/deavid/git/rust/zzping/src/net/zznet-router/tests/integration_tests.rs`

**SendToPeer Usage - Line 155:**
```rust
let send_msg = SendToPeer {
    peer_id: peer_id.clone(),
    room_id: room_id.clone(),
    bytes: message_data.clone(),
};

let result = router_actor.send(send_msg).await;
assert!(result.is_ok(), "Message send should succeed");
```
**Context:** Integration test verifying `SendToPeer` functionality
**Severity:** Low - Test code only

---

**BroadcastToPeers Usage - Line 206:**
```rust
let broadcast_msg = BroadcastToPeers {
    peer_ids: peer_ids.clone(),
    room_id: RoomId::new("broadcast-room"),
    bytes: b"broadcast message".to_vec(),
};

let result = router_actor.send(broadcast_msg).await;
assert!(result.is_ok(), "Broadcast should succeed");
```
**Context:** Integration test verifying `BroadcastToPeers` functionality
**Severity:** Low - Test code only

---

### Summary: Router Runtime Routing API

**Total Production Usage:** 0 locations
**Total Test Usage:** 2 locations

**Files Affected:**
- `/home/deavid/git/rust/zzping/src/net/zznet-router/src/actor.rs` (message definitions and handlers)
- `/home/deavid/git/rust/zzping/src/net/zznet-router/tests/integration_tests.rs` (test usage only)

**Key Insight:** The runtime routing API exists and is fully implemented, but **no production components currently use it**. This is actually POSITIVE news:
- The anti-pattern exists in the code
- But it has NOT spread to actual components
- Components are likely using direct channel access (the correct pattern)

**Recommendation:** Since these messages are not used in production:
1. **Deprecate** `SendToPeer` and `BroadcastToPeers` message handlers
2. **Mark as deprecated** with comments explaining the correct pattern
3. **Keep for now** to support existing integration tests
4. **Eventually remove** after confirming no hidden dependencies

---

## Task 3: Final Synthesis Report

### Deviations from Principle #6: Trait Object Wiring

**Summary:** The `Box<dyn RoomHandle>` pattern is deeply embedded in the architecture.

**Affected Components:**

| File | Lines | Severity | Description |
|------|-------|----------|-------------|
| `src/net/zznet-router/src/peer_channels.rs` | 9, 16, 32 | **Critical** | Type alias, builder field, method parameter |
| `src/net/zznet-room/src/room_manager.rs` | 58, 65 | **Critical** | Trait return type, message type |
| `src/net/zznet-room/src/room_adapter.rs` | 14 | Low | Documentation comment |

**Total Source Code Occurrences:** 6

**Architectural Impact:**
- Prevents actor-based messaging between `PeerChannels` and `Room<T>` actors
- Enforces synchronous trait method calls instead of async actor messages
- Makes testing more difficult (mock trait objects vs. mock actors)
- Violates the "pure actor model" vision

**Migration Complexity:** High
- Requires changes to core traits (`RoomManager`)
- Affects all component implementations
- Needs new message types for room communication
- Requires `Recipient<RoomMsg>` instead of trait objects

---

### Deviations from Principle #2: Router's Runtime Role

**Summary:** Runtime routing API exists but is NOT used by production code.

**Affected Components:**

| File | Lines | Severity | Description |
|------|-------|----------|-------------|
| `src/net/zznet-router/src/actor.rs` | 194-221, 225-249 | **Medium** | Handler implementations present |
| `src/net/zznet-router/tests/integration_tests.rs` | 155, 206 | Low | Test usage only |

**Total Production Usage:** 0
**Total Test Usage:** 2

**Architectural Impact:**
- API surface implies Router should be used as a message bus
- Creates confusion about the correct way to send data
- Contradicts documentation stating Router is "lifecycle only"
- However: **No production code uses this**, so the damage is contained

**Migration Complexity:** Low
- Can be deprecated immediately with compiler warnings
- No production code to migrate
- Tests can be updated to use direct channel access
- Eventually can be removed entirely

---

## Overall Conclusion

### Primary Remaining Deviations

**1. Trait Object Wiring (HIGH PRIORITY)**
- **Status:** Pervasive in architecture
- **Impact:** Blocks pure actor model implementation
- **Remediation:** Major refactoring required

**2. Router Runtime Routing API (LOW PRIORITY)**
- **Status:** Present but unused
- **Impact:** Confusing API surface, no active harm
- **Remediation:** Deprecation and eventual removal

### Positive Findings

1. **No Production Misuse:** Components are NOT using `SendToPeer`/`BroadcastToPeers`
2. **Contained Scope:** Only 6 critical trait object locations in source code
3. **Clear Boundaries:** The deviations are well-defined and localized to specific files

### Recommended Action Plan

#### Phase 1: Deprecation (Low Effort)
1. Add `#[deprecated]` attributes to `SendToPeer` and `BroadcastToPeers`
2. Add doc comments explaining the correct pattern
3. Update integration tests to demonstrate direct channel usage

#### Phase 2: Trait Object Migration (High Effort)
1. Design new message types for room communication
2. Modify `RoomManager` trait to return `Recipient<RoomMsg>`
3. Update `PeerChannels` to store `HashMap<RoomId, Recipient<RoomMsg>>`
4. Migrate all component implementations
5. Remove `RoomHandle` trait

#### Phase 3: Cleanup (Low Effort)
1. Remove deprecated routing messages
2. Update all documentation
3. Remove obsolete test code

---

**Audit Completed:** October 29, 2025
**Confidence Level:** High - Comprehensive project-wide search performed
**Next Review:** After Phase 1 deprecations are merged
