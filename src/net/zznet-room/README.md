# zznet-room

**High-level room abstraction for ZZNet components**

## Vision

This crate provides `Room<T>` - the **component developer's primary API** for network communication. It makes building network-aware components trivial by hiding all serialization, registration, and routing complexity.

## Purpose

Be the **component-facing API** that makes networking feel like local function calls:

```
Component uses Room<T> ← YOU ARE HERE
    ↓
SessionManager (routes messages)
    ↓
Network stack
```

It provides:
- **Type-safe message passing**: `Room<MyMessageType>` enforces types at compile time
- **Automatic serialization**: Components never touch serde
- **Auto-registration**: Rooms register themselves with SessionManager
- **Fire-and-forget semantics**: No ACKs, no retries at framework level
- **Simple send/recv API**: Like channels, but over network

## Why This Matters

Without Room<T>:
- ❌ Components manually register with SessionManager
- ❌ Components handle serialization
- ❌ Type safety not enforced
- ❌ Boilerplate everywhere

With Room<T>:
- ✅ Zero boilerplate - just `Room::<MyType>::new()`
- ✅ Type safety automatic
- ✅ Serialization transparent
- ✅ Clean, simple API

## Key Requirements

### Zero-Boilerplate Rooms
Creating a typed room should be one line:
```
let room = Room::<MyMessage>::new(room_id, session_mgr, session_id);
```

Then just `room.send(msg)` and `room.recv()`. That's it.

### Fire-and-Forget Semantics
**From design docs**: Component network messages are fire-and-forget. No ACKs at framework level.

**NOTE**: If the app needs acks, redesign the whole app. We shouldn't need ACKs. ZZNet is explicitly designed this way.

`send()` returns when message is queued for sending. Does NOT wait for:
- Network transmission
- Peer reception
- Peer acknowledgment

**If application needs reliability**, implement at application level, not framework level.

### Rooms Are 1:1 Per Connection
**Critical insight from vision docs**: Rooms are NOT broadcast channels.

```
Process A                          Process B
┌─────────────┐                   ┌─────────────┐
│ Component   │ ←─ Room "data" ─→ │ Component   │
└─────────────┘                   └─────────────┘
      ONE CONNECTION, ONE BIDIRECTIONAL TYPED CHANNEL
```

If Process B has 3 connections, it has 3 separate "data" rooms (one per connection).

### Type Safety
`Room<T>` enforces message type at compile time. Cannot send wrong type to a room.

## What This Crate Must NOT Do

- ❌ Implement transport (that's zznet-transport-tcp)
- ❌ Manage sessions (that's zznet-session)
- ❌ Provide high-level builders (that's zznet-builder)
- ❌ Implement authorization (that's zznet-auth)
- ❌ Provide ACKs/retries (application responsibility)
- ❌ Implement reliable delivery (that's TCP's job)

## Design Principles

### Type Safety
- `Room<T>` enforces message types at compile time
- Impossible to send wrong message type
- Serialization errors caught early

### Zero Boilerplate
- No manual handler implementation
- No explicit serialization/deserialization
- Just send and receive typed messages

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

Think of Room<T> as:
- **Like** a Rust channel (mpsc, oneshot)
- **But** over the network
- **And** strongly typed
- **And** fire-and-forget

NOT like:
- ❌ IRC channels (not broadcast)
- ❌ Pub/sub topics (1:1, not 1:N)
- ❌ Message queues (no broker, no persistence)

## Multiple Message Types

One component can use multiple rooms for different message types:

```
struct MyComponent {
    requests: Room<RequestMessage>,
    responses: Room<ResponseMessage>,
    events: Room<EventMessage>,
}
```

Different rooms = different message types. Same connection.

## Testing Requirements

### Must Pass
- Create rooms easily
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

- **zznet-session**: SessionManager that rooms register with
- **zznet-hello**: Provides serialization for rooms
- **zznet-api**: Transport abstraction rooms build on
- **zznet-builder**: High-level API that creates rooms

## Success Criteria

This crate succeeds if:
- ✅ Components use only Room<T>, never lower layers
- ✅ Creating a room is one line of code
- ✅ Sending/receiving is trivial
- ✅ Type safety prevents runtime errors
- ✅ Developers never think about serialization
- ✅ Fire-and-forget semantics clear and enforced

