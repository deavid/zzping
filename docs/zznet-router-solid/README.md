# ZZNet Router SOLID Architecture

This directory contains the design and implementation documentation for the ZZNet Router SOLID refactor, which introduces an actor-first messaging architecture to replace trait objects and improve separation of concerns.

## Architecture Overview

The ZZNet Router SOLID refactor implements the following key principles:

- **Actor-first messaging**: RouterActor provides typed actor-based message routing instead of `Arc<dyn MessageRouter>`
- **SOLID principles**: Single responsibility, open/closed, Liskov substitution, interface segregation, dependency inversion
- **Data-plane separation**: Control-plane (peer lifecycle) separated from data-plane (message routing)
- **Component-provided factories**: RoomManager trait enables components to provide room factories at startup
- **Strict 1:1 mapping**: Enforced room↔component mapping with hard errors on collisions

## Core Components

### RouterActor

The RouterActor is the central actor that orchestrates peer connections, room negotiation, and message routing.

```rust
use zznet_router::actor::RouterActor;

// Create RouterActor with offered rooms
let offered_rooms = vec![RoomId::new("data-room"), RoomId::new("control-room")];
let router_actor = RouterActor::new(offered_rooms, None).start();

// Register component room managers at startup
let room_manager = MyRoomManager::new();
router_actor.do_send(RegisterManager {
    manager: Box::new(room_manager),
});
```

### RoomManager Trait

Components implement the RoomManager trait to provide room factories:

```rust
use zznet_room::room_manager::RoomManager;

pub struct MyRoomManager {
    // component state
}

impl RoomManager for MyRoomManager {
    fn managed_rooms(&self) -> Vec<RoomId> {
        vec![RoomId::new("my-component-room")]
    }

    async fn create_for_peer(
        &self,
        peer_id: PeerId,
        permission: Permission,
        room_id: &RoomId,
    ) -> Result<Option<Box<dyn RoomHandle>>, CreateError> {
        if room_id == &RoomId::new("my-component-room") {
            let room = MyRoom::new(peer_id, permission);
            Ok(Some(Box::new(room)))
        } else {
            Ok(None) // This manager doesn't handle this room
        }
    }
}
```

## Message Flow

### Peer Lifecycle

1. **Peer Connection**: PeerManagerActor connects peer to RouterActor
```rust
router_actor.send(OnPeerConnected {
    peer_id: peer_id.clone(),
    permission: create_permission(peer_id.clone()),
    outbound_tx: peer_outbound_tx,
    inbound_rx: peer_inbound_rx,
}).await?;
```

2. **Room Negotiation**: Peer publishes desired rooms, RouterActor finds intersection
```rust
let result = router_actor.send(HandlePublishRooms {
    peer_id: peer_id.clone(),
    peer_rooms: vec![RoomId::new("shared-room")],
}).await?;
```

3. **Message Routing**: Components send messages through RouterActor
```rust
router_actor.send(SendToPeer {
    peer_id: target_peer,
    room_id: RoomId::new("data-room"),
    bytes: message_data,
}).await?;
```

### Lifecycle Diagram

```
PeerManagerActor    RouterActor    RoomManager
      |                   |             |
      |--OnPeerConnected->|             |
      |                   |--create_for_peer()-->
      |                   |             |
      |<--PeerConnected---|             |
      |                   |             |
      |--HandlePublishRooms----------->|
      |                   |             |
      |<--RoomsNegotiated-|             |
      |                   |             |
      |--SendToPeer-------------------->|
      |                   |             |
```

## API Reference

### RouterActor Messages

- `OnPeerConnected`: Establish peer channels and permission
- `OnPeerDisconnected`: Clean up peer resources
- `HandlePublishRooms`: Negotiate room intersection with peer
- `SendToPeer`: Route message to specific peer/room
- `BroadcastToPeers`: Send message to multiple peers in a room
- `RegisterManager`: Register component room manager at startup

### Error Handling

- `SessionError::EmptyIntersection`: No common rooms between peers
- `SessionError::RoomNotJoined`: Attempt to send to unjoined room
- `SessionError::PeerNotFound`: Peer not connected
- `SessionError::RoomAlreadyExists`: Duplicate room registration

## Migration Guide

### Before (Trait Objects)
```rust
let message_router: Arc<dyn MessageRouter> = router.clone();
component.set_message_router(message_router);
```

### After (Actor Messaging)
```rust
let router_actor: Addr<RouterActor> = router_actor.clone();
component.set_router_actor(router_actor);
```

## Testing

Comprehensive integration tests validate RouterActor + PeerManagerActor interactions:

```rust
#[actix::test]
async fn test_router_actor_peer_lifecycle() {
    let router_actor = RouterActor::new(vec![RoomId::new("test-room")], None).start();

    // Connect peer
    let (outbound_tx, _outbound_rx) = mpsc::channel(10);
    let (_inbound_tx, inbound_rx) = mpsc::channel(10);
    let peer_id = PeerId::from("test-peer");

    let connect_msg = OnPeerConnected { /* ... */ };
    let result = router_actor.send(connect_msg).await.unwrap();
    assert!(result.is_ok());
}
```

## Quality Gates

All quality gates must pass before the refactor is considered complete:

- ✅ **Build**: `cargo build` passes for all crates
- ✅ **Lint/Typecheck**: No unused imports or broken trait implementations
- ✅ **Tests**: Unit tests + integration tests covering happy path and edge cases
- ✅ **Integration**: Full workspace tests pass without regressions

## Files Changed

### Core Implementation
- `src/net/zznet-router/src/actor.rs` - RouterActor and message types
- `src/net/zznet-router/src/lib.rs` - Router core with RoomManager registration
- `src/net/zznet-room/src/room_manager.rs` - RoomManager trait definition

### Component Migration
- `src/components/zzmem-db/src/network_manager.rs` - Migrated to RouterActor
- `src/components/zzcollector-state/src/network_manager.rs` - Migrated to RouterActor
- `src/apps/zzping-database/src/service.rs` - Updated component startup
- `src/apps/zzping-collector/src/service.rs` - Updated component startup

### Tests
- `src/net/zznet-router/tests/integration_tests.rs` - Comprehensive integration tests
- Updated unit tests across all migrated components

## Future Considerations

- Room lifecycle management (creation/destruction per peer)
- Performance optimization for high-peer-count scenarios
- Enhanced error reporting and debugging capabilities
- Potential for distributed RouterActor clusters</content>
<parameter name="filePath">/home/deavid/git/rust/zzping/docs/zznet-router-solid/README.md