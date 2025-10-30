# zznet-room

**Room abstraction for ZZNet components using the actor-based architecture**

## Vision

This crate provides `RoomActor<T>` and `RoomManager` - the **component developer's primary APIs** for network communication in the actor-based architecture. It makes building network-aware components simple by providing typed message passing through actors.

## Purpose

Be the **component-facing API** that integrates components with the network stack:

```
Component implements RoomManager ← YOU ARE HERE
    ↓
RouterActor creates RoomActor<T> instances
    ↓
Network stack
```

It provides:
- **Type-safe message passing**: `RoomActor<MyMessageType>` enforces types at compile time
- **Automatic serialization**: Components never touch serde
- **Actor-based communication**: Messages flow through Actix actors
- **Room management**: Components register room factories with the Router
- **Clean separation**: Business logic separate from network concerns

## Why This Matters

The actor-based architecture provides:
- ✅ Clear separation between business logic and network concerns
- ✅ Type safety through RoomMessageTrait
- ✅ Automatic serialization/deserialization via RoomActor
- ✅ Component lifecycle management through RoomManager
- ✅ Integration with Actix actor system

## Key Concepts

### RoomActor<T>
A typed actor that handles serialization/deserialization for a specific room:

```rust
// Router creates this automatically via RoomManager
let room_actor = RoomActor::new(
    room_id,
    outbound_channel,
    component_recipient,
);
```

### RoomManager
Component-provided factory for creating room actors per peer:

```rust
impl RoomManager for MyComponentNetworkManager {
    fn managed_rooms(&self) -> HashSet<RoomId> {
        // Return rooms this component handles
    }

    async fn create_for_peer(&self, peer_id, permission, room_id, outbound) -> ... {
        // Create RoomActor for this peer/room combination
    }
}
```

### RoomMessageTrait
Messages must implement this trait for serialization:

```rust
#[derive(Serialize, Deserialize)]
struct MyMessage { /* fields */ }

impl RoomMessageTrait for MyMessage {
    fn room_id(&self) -> RoomId { /* return room */ }
    fn serialize_inner(&self) -> Result<Vec<u8>> { /* bincode */ }
    fn deserialize_for_room(room_id, bytes) -> Result<Self> { /* bincode */ }
}
```

## Architecture Flow

1. Component implements `RoomManager` in its NetworkManager actor
2. NetworkManager registers with `RouterActor` at startup
3. When peer connects, Router calls `create_for_peer()` for each managed room
4. RoomActor instances are created to handle serialization
5. Messages flow: Component → RoomActor → Router → Network → Peer
6. Inbound: Network → Router → RoomActor → Component

## What This Crate Must NOT Do

- ❌ Implement transport (that's zznet-transport-tcp)
- ❌ Manage peer connections (that's zznet-peer-manager)
- ❌ Route messages (that's zznet-router)
- ❌ Provide high-level builders (that's zznet-builder)
- ❌ Implement authorization (that's zznet-auth)

## Design Principles

### Type Safety
- `RoomActor<T>` enforces message types at compile time
- Impossible to send wrong message type to a room
- Serialization errors caught at runtime (logged)

### Actor-Based
- All communication through Actix actors
- Clear message flow and lifecycle management
- Integration with Actix supervision and error handling

### Separation of Concerns
- Business logic in MainActor
- Network concerns in NetworkManager
- Serialization in RoomActor

### Fire-and-Forget
- Framework provides no ACKs
- No retries at framework level
- Application implements reliability if needed
- **Redesign app if you think you need ACKs**

### Component-Focused
- API designed for application developers
- Hides network complexity
- Natural fit with actor model

### Testable
- Works with mock transport
- Deterministic unit tests
- Easy to test components in isolation

## Rooms vs Channels

Think of RoomActor<T> as:
- **Like** a typed actor that serializes messages
- **But** integrated with Router for network communication
- **And** strongly typed
- **And** fire-and-forget

NOT like:
- ❌ IRC channels (not broadcast)
- ❌ Pub/sub topics (1:1, not 1:N)
- ❌ Message queues (no broker, no persistence)

## Multiple Message Types

One component can manage multiple rooms for different message types:

```
impl RoomManager for MyNetworkManager {
    fn managed_rooms(&self) -> HashSet<RoomId> {
        ["requests", "responses", "events"].into_iter().map(RoomId::from).collect()
    }
}
```

Different rooms = different message types. Same connection.

## Testing Requirements

### Must Pass
- Implement RoomManager easily
- Create RoomActor instances per peer
- Send/receive typed messages
- Work with mock transport
- Work with TCP transport
- Type errors at compile time
- Graceful connection close

### Mock-First
- All tests use mock transport first
- No test should require real network
- Deterministic behavior

## Relationship to Other Crates

- **zznet-router**: RouterActor that creates RoomActor instances
- **zznet-peer-manager**: PeerManagerActor for peer lifecycle
- **zznet-api**: Types and traits for network communication
- **zznet-builder**: High-level API that creates rooms

## Success Criteria

This crate succeeds if:
- ✅ Components use only Room<T>, never lower layers
- ✅ Creating a room is one line of code
- ✅ Sending/receiving is trivial
- ✅ Type safety prevents runtime errors
- ✅ Developers never think about serialization
- ✅ Fire-and-forget semantics clear and enforced

