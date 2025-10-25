# Room Registration Design

**Date**: October 25, 2025
**Status**: Design Proposal - Phase 1, Task 1.1
**Owner**: Architecture Lead

---

## Executive Summary

This document defines the auto-registration API for Room<T> to integrate with SessionManager. The design enables components to automatically register their rooms without application boilerplate.

**Key Design Decision**: Room<T> will register itself with SessionManager during construction, eliminating manual wiring.

---

## API Design

### Option A: Direct SessionManager Integration (RECOMMENDED)

#### Room<T> Constructor

```rust
// New constructor with auto-registration
impl<T> Room<T>
where
    T: Message<Result = ()> + Send + Clone + Serialize + for<'de> Deserialize<'de> + 'static,
{
    /// Create a new Room that auto-registers with SessionManager
    ///
    /// This is the primary constructor for production use.
    /// The room will automatically register its channels with the SessionManager.
    pub fn new_with_session_manager(
        room_id: String,
        local_handler: Recipient<T>,
        session_manager: Arc<Mutex<SessionManager>>,
    ) -> Result<Self, RoomError> {
        // Create channels
        let (outbound_tx, outbound_rx) = mpsc::channel(100);
        let (inbound_tx, inbound_rx) = mpsc::channel(100);

        // Register with SessionManager immediately
        {
            let mut sm = session_manager.blocking_lock();
            sm.register_room_handler(
                RoomId::from(room_id.clone()),
                inbound_tx.clone(),
                outbound_rx,
            )?;
        }

        // Spawn receiver task
        let mut room = Room {
            room_id: room_id.clone(),
            outbound_tx,
            inbound_rx: Some(inbound_rx),
            local_handler: local_handler.clone(),
            receiver_task: None,
        };

        room.spawn_receiver()?;

        Ok(room)
    }

    /// Legacy constructor without SessionManager (for testing)
    ///
    /// Returns Room and channels for manual wiring.
    /// Use `new_with_session_manager()` in production.
    pub fn new(room_id: String, local_handler: Recipient<T>) -> (Self, RoomChannels) {
        // Existing implementation
        // ...
    }
}
```

**Pros**:
- ✅ Simple and direct
- ✅ Registration happens immediately
- ✅ Error handling straightforward
- ✅ Works with existing SessionManager

**Cons**:
- ⚠️ Requires SessionManager to be Arc<Mutex<>> not Addr<>
- ⚠️ Blocking lock during construction

---

### Option B: Deferred Registration via Message (Alternative)

```rust
// Room stores session manager address
pub struct Room<T> {
    room_id: String,
    session_manager: Option<Arc<Mutex<SessionManager>>>,
    // ... other fields
}

impl<T> Room<T> {
    pub fn new_with_deferred_registration(
        room_id: String,
        local_handler: Recipient<T>,
        session_manager: Arc<Mutex<SessionManager>>,
    ) -> Self {
        let (outbound_tx, outbound_rx) = mpsc::channel(100);
        let (inbound_tx, inbound_rx) = mpsc::channel(100);

        let room = Room {
            room_id: room_id.clone(),
            outbound_tx,
            inbound_rx: Some(inbound_rx),
            local_handler,
            receiver_task: None,
            session_manager: Some(session_manager),
        };

        // Registration happens on first peer connection
        // Triggered by SessionManager calling activate()

        room
    }

    /// Called by SessionManager when peer connects
    pub async fn activate_for_peer(&mut self, peer_id: PeerId) -> Result<(), RoomError> {
        if let Some(sm) = &self.session_manager {
            // Register now
            sm.lock().await.register_room_handler(...)?;
        }
        Ok(())
    }
}
```

**Pros**:
- ✅ Non-blocking construction
- ✅ Can defer until peer actually connects
- ✅ More flexible

**Cons**:
- ❌ More complex lifecycle
- ❌ Registration can fail later
- ❌ Harder to debug

---

## Design Decision: Option A

**Chosen Approach**: Direct SessionManager Integration (Option A)

**Rationale**:
1. Simpler implementation and clearer semantics
2. Fail-fast: errors discovered at construction time
3. Matches PoC pattern closely
4. SessionManager is already accessed synchronously in current code

**Trade-off Accepted**: Blocking lock during Room construction is acceptable because:
- Construction happens once per component at startup
- Lock is held briefly (just to store channels)
- No async work done while holding lock

---

## SessionManager API Changes

### Current State

```rust
pub struct SessionManager {
    peers: HashMap<PeerId, PeerSession>,
    offered_rooms: Vec<RoomId>,
}

// SessionManager is created and used synchronously
let mut session_manager = SessionManager::new(offered_rooms);
```

### Required Changes

```rust
pub struct SessionManager {
    peers: HashMap<PeerId, PeerSession>,
    offered_rooms: Vec<RoomId>,
    // NEW: Store room handlers that can be applied to any peer
    room_handlers: HashMap<RoomId, RoomHandlerChannels>,
}

pub struct RoomHandlerChannels {
    /// Channel to send bytes TO the room (for inbound messages from network)
    inbound_tx: mpsc::Sender<Vec<u8>>,
    /// Channel to receive bytes FROM the room (for outbound messages to network)
    outbound_rx: mpsc::Receiver<Vec<u8>>,
}

impl SessionManager {
    /// Register a room handler that will be activated for all peers
    pub fn register_room_handler(
        &mut self,
        room_id: RoomId,
        inbound_tx: mpsc::Sender<Vec<u8>>,
        outbound_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<(), SessionError> {
        if self.room_handlers.contains_key(&room_id) {
            return Err(SessionError::RoomAlreadyRegistered(room_id));
        }

        self.room_handlers.insert(room_id.clone(), RoomHandlerChannels {
            inbound_tx,
            outbound_rx,
        });

        // Activate for all existing connected peers
        for peer in self.peers.values_mut() {
            if peer.is_connected() && peer.has_room(&room_id) {
                peer.activate_room(room_id.clone(), /* channels */)?;
            }
        }

        Ok(())
    }

    /// Called when a new peer connects
    pub fn peer_connected(&mut self, peer_id: PeerId, negotiated_rooms: Vec<RoomId>) -> Result<(), SessionError> {
        // For each negotiated room, activate the handler if registered
        for room_id in negotiated_rooms {
            if let Some(handler) = self.room_handlers.get(&room_id) {
                // Clone channels for this peer
                // (Note: may need to rethink channel ownership)
                peer.activate_room(room_id, handler.channels())?;
            }
        }
        Ok(())
    }
}
```

---

## Ownership Model

### Room Ownership

**Decision**: Component owns the Room<T> instance

```rust
pub struct ComponentActor {
    room: Option<Room<ComponentMessage>>,
    // ...
}
```

**Rationale**:
- Component needs to call `room.send()` to send messages
- Component's lifecycle controls room lifecycle
- Clear ownership: component responsible for room

### Channel Ownership

**Decision**: SessionManager owns the channel receivers/senders after registration

```rust
// Room keeps sender for outbound
pub struct Room<T> {
    outbound_tx: mpsc::Sender<Vec<u8>>,  // ← Room keeps this
    // ...
}

// SessionManager gets receiver for outbound
pub struct RoomHandlerChannels {
    outbound_rx: mpsc::Receiver<Vec<u8>>,  // ← SessionManager owns this
    // ...
}
```

**Challenge**: How to share channels across multiple peers?

**Solution**: Use `broadcast` or clone senders/receivers appropriately:

```rust
// For outbound (Room → Network):
// - Room keeps: mpsc::Sender<Vec<u8>> (many senders possible)
// - SessionManager gets: mpsc::Receiver<Vec<u8>> (only one receiver)
// - Per peer: need to multiplex from single receiver to multiple peers

// For inbound (Network → Room):
// - SessionManager has: multiple sources (one per peer)
// - Room needs: single mpsc::Receiver<Vec<u8>>
// - Solution: merge all peer senders into single receiver
```

**Refined Design**:

```rust
// Room side
pub struct Room<T> {
    // For sending TO network (outbound)
    outbound_tx: mpsc::Sender<Vec<u8>>,

    // For receiving FROM network (inbound)
    inbound_rx: Option<mpsc::Receiver<Vec<u8>>>,

    // Local handler for deserialized messages
    local_handler: Recipient<T>,
}

// SessionManager side (per peer)
pub struct PeerRoomChannels {
    room_id: RoomId,

    // To send TO this peer's network (receives from room's outbound)
    outbound_rx: /* need broadcast or select! */,

    // To receive FROM this peer's network (sends to room's inbound)
    inbound_tx: mpsc::Sender<Vec<u8>>,
}
```

**Issue Identified**: One Room → Multiple Peers requires broadcast pattern

**Resolution**:

1. **For Outbound (Room → Peers)**: Use `tokio::sync::broadcast`
   ```rust
   // Room creates broadcast channel
   let (broadcast_tx, _) = broadcast::channel(100);

   // Each peer gets a subscriber
   let peer_rx = broadcast_tx.subscribe();
   ```

2. **For Inbound (Peers → Room)**: Each peer has own sender to room's receiver
   ```rust
   // Room has one receiver
   let (inbound_tx, inbound_rx) = mpsc::channel(100);

   // Each peer gets a clone of the sender
   let peer_tx = inbound_tx.clone();
   ```

---

## Updated API Design with Broadcast

```rust
impl<T> Room<T>
where
    T: Message<Result = ()> + Send + Clone + Serialize + for<'de> Deserialize<'de> + 'static,
{
    pub fn new_with_session_manager(
        room_id: String,
        local_handler: Recipient<T>,
        session_manager: Arc<Mutex<SessionManager>>,
    ) -> Result<Self, RoomError> {
        // Outbound: Room → Network (broadcast to all peers)
        let (outbound_broadcast_tx, _) = broadcast::channel(100);
        let outbound_tx = /* wrap broadcast in mpsc-like interface */;

        // Inbound: Network → Room (all peers send to same receiver)
        let (inbound_tx, inbound_rx) = mpsc::channel(100);

        // Register with SessionManager
        {
            let mut sm = session_manager.blocking_lock();
            sm.register_room_handler(
                RoomId::from(room_id.clone()),
                inbound_tx,  // SessionManager can clone this for each peer
                outbound_broadcast_tx,  // SessionManager subscribes for each peer
            )?;
        }

        let mut room = Room {
            room_id: room_id.clone(),
            outbound_tx,  // Wrapped broadcast sender
            inbound_rx: Some(inbound_rx),
            local_handler: local_handler.clone(),
            receiver_task: None,
        };

        room.spawn_receiver()?;

        Ok(room)
    }
}
```

---

## Error Handling

### Registration Errors

```rust
#[derive(Debug, Error)]
pub enum RoomError {
    #[error("Room {0} already registered")]
    AlreadyRegistered(RoomId),

    #[error("SessionManager unavailable")]
    SessionManagerUnavailable,

    #[error("Failed to register with SessionManager: {0}")]
    RegistrationFailed(String),

    #[error("Failed to spawn receiver task")]
    ReceiverSpawnFailed,
}
```

### Error Handling Strategy

1. **Construction Fails**: Return error, room not created
2. **Registration Fails**: Return error, channels dropped
3. **Receiver Spawn Fails**: Return error, cleanup channels

**Fail-Fast Philosophy**: Better to fail at startup than have silent failures at runtime

---

## Lifecycle Management

### Registration Timing

**Decision**: Registration happens at Room construction

```rust
// Component builder
impl ComponentBuilder {
    pub fn start(self) -> Result<Addr<ComponentActor>, Error> {
        let actor = ComponentActor::new(...);
        let actor_addr = actor.start();

        if let Some(sm) = self.session_manager {
            // Room auto-registers HERE during construction
            let room = Room::new_with_session_manager(
                "component-room",
                actor_addr.recipient(),
                sm,
            )?;

            actor_addr.do_send(SetRoom(room));
        }

        Ok(actor_addr)
    }
}
```

### Peer Connection Lifecycle

```rust
// When peer connects:
// 1. HELLO protocol negotiates rooms
// 2. SessionManager.peer_connected(peer_id, negotiated_rooms)
// 3. For each negotiated room:
//    - If room handler registered, activate channels for this peer
//    - Start forwarding tasks
// 4. Peer now active

// When peer disconnects:
// 1. SessionManager.peer_disconnected(peer_id)
// 2. Stop forwarding tasks
// 3. Keep room registration (for reconnection)
// 4. Room stays registered, ready for next peer
```

### Unregistration

**Decision**: Rooms are not explicitly unregistered

**Rationale**:
- Rooms are tied to component lifecycle
- When component drops, Room drops
- Channels close automatically
- SessionManager detects closed channels and cleans up

**Optional Explicit Cleanup** (if needed later):
```rust
impl<T> Drop for Room<T> {
    fn drop(&mut self) {
        // Channels close automatically
        // SessionManager will detect and cleanup
    }
}
```

---

## Backward Compatibility

### Transition Strategy

1. **Keep old `Room::new()`** for existing code and tests
2. **Add new `Room::new_with_session_manager()`** for new pattern
3. **Components migrate incrementally** during Phase 2
4. **Remove old constructor** in Phase 4 cleanup

### Testing Compatibility

```rust
// Old tests continue to work
#[test]
fn test_room_basic() {
    let (room, channels) = Room::new("test", handler);
    // Manual wiring
}

// New tests use auto-registration
#[test]
fn test_room_auto_register() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new(vec![])));
    let room = Room::new_with_session_manager("test", handler, session_manager)?;
    // Automatically registered
}
```

---

## Implementation Checklist

### Task 1.2: Implement Registration in Room<T>

- [ ] Add `tokio::sync::broadcast` dependency
- [ ] Implement `new_with_session_manager()` constructor
- [ ] Wrap broadcast sender in mpsc-like interface (or use directly)
- [ ] Handle registration errors
- [ ] Keep old `new()` for compatibility
- [ ] Add unit tests for new constructor
- [ ] Document usage patterns

### Task 1.3: Implement Registration Handler in SessionManager

- [ ] Add `room_handlers: HashMap<RoomId, RoomHandlerChannels>` field
- [ ] Implement `register_room_handler()` method
- [ ] Update `peer_connected()` to activate registered rooms
- [ ] Update `peer_disconnected()` to deactivate (but keep registration)
- [ ] Handle channel cloning/subscription for multiple peers
- [ ] Add integration tests
- [ ] Document lifecycle

---

## Examples

### Component Usage (After Implementation)

```rust
// In component builder
pub fn start(self) -> Result<Addr<MyActor>, Error> {
    let actor = MyActor::new();
    let actor_addr = actor.start();

    if let Some(sm) = self.session_manager {
        // One line - auto-registration happens here
        let room = Room::new_with_session_manager(
            "my-component-room",
            actor_addr.recipient(),
            sm,
        )?;

        actor_addr.do_send(SetRoom(room));
    }

    Ok(actor_addr)
}
```

### Application Usage (Phase 3 Target)

```rust
// Application main
async fn main() -> Result<()> {
    // Create session manager
    let session_manager = Arc::new(Mutex::new(
        SessionManager::new(vec![
            RoomId::from("intent-config"),
            RoomId::from("memdb"),
        ])
    ));

    // Create components - rooms auto-register
    let intent = IntentConfigBuilder::new(role)
        .with_session_manager(session_manager.clone())
        .start()?;

    let memdb = MemDBBuilder::new(role)
        .with_session_manager(session_manager.clone())
        .start()?;

    // Done - no factories, no handlers, no manual wiring!

    Ok(())
}
```

---

## Review Checkpoint

```
□ API design reviewed and approved
□ Ownership model clear
□ Error handling strategy defined
□ Lifecycle semantics documented
□ Backward compatibility considered
□ Broadcast pattern acceptable for multi-peer
□ SessionManager changes understood
```

**Status**: Ready for review and approval before implementation

---

## Open Questions

1. **Q**: Should we use `broadcast::channel` or implement custom multiplexing?
   **A**: Start with broadcast, measure performance, optimize if needed

2. **Q**: What buffer sizes for channels?
   **A**: Start with 100 (same as current), make configurable later

3. **Q**: Should Room implement Drop for explicit cleanup?
   **A**: Not needed initially, channels auto-close. Add if issues arise.

4. **Q**: How to handle room re-registration after component restart?
   **A**: Component restart creates new Room, old channels close, new registration succeeds

---

**Next Step**: Review and approve this design, then proceed to Task 1.2 (Implementation)
