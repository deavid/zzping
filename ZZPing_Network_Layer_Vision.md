# ZZPing Network Layer: Core Vision

**Document Purpose**: This is the reference document for the essential architectural vision of the ZZPing network layer. It captures the key insights, critical decisions, and fundamental principles that must guide all design and implementation work.

**Date**: October 2, 2025
**Status**: Authoritative Reference
**Use Case**: When in doubt about architecture decisions, refer to this document first.

---

## The Core Vision: Transport-Agnostic Typed Communication

The network layer enables components to communicate across processes using **typed Rust messages** without any knowledge of how those messages are transported.

### The Key Insight

Components should be able to communicate across the network **exactly as if they were communicating in-memory**. The transport (TCP, mock, future transports) is completely pluggable and invisible to the component logic.

---

## Critical Architectural Principle: Same Component, Different Config

**WRONG Mental Model**: Different components on each side
```
Collector Side: PingSubmitter
Database Side:  PingReceiver
```

**CORRECT Mental Model**: Same component, different configuration
```
Process A: MemDB(config=Collector)
Process B: MemDB(config=Database)
```

**Why This Matters**:
- All network communication code for a component lives in ONE place
- Easy to reason about - you see both sides of the protocol in the same file
- Testing is trivial - create two instances with different configs
- No need to navigate multiple crates to understand communication

**Rule**: A component's networking code must be self-contained in the component's crate. If Component X talks to Component X across the network, all that code is in Component X's implementation.

---

## What is a "Room"? (The Most Misunderstood Concept)

### What Everyone Thinks

❌ "A room is like an IRC channel where multiple peers broadcast to each other"
❌ "Rooms enable many-to-many communication"
❌ "Rooms are for broadcasting messages to multiple recipients"

### What It Actually Is

✅ **A room is a point-to-point typed channel between two specific component instances**

```
Process A                          Process B
┌─────────────┐                   ┌─────────────┐
│ MemDB       │ ←─ Room "memdb" ─→│ MemDB       │
│ (Collector) │                   │ (Database)  │
└─────────────┘                   └─────────────┘
      ONE CONNECTION, ONE BIDIRECTIONAL TYPED CHANNEL
```

### Key Properties of Rooms

1. **1:1 Communication**: Room connects exactly two endpoints
2. **Per-Connection**: Each TCP connection has its own set of rooms
3. **Typed Channel**: Messages are strongly-typed Rust structs
4. **Bidirectional**: Both sides can send and receive
5. **Multiplexed**: Multiple rooms share one TCP connection

### Better Analogy

A room is like a **phone line between two people**, not a **conference call with many people**.

If Database has 3 collectors connected:
- Connection 1 has room "memdb" (between DB and Collector-1)
- Connection 2 has room "memdb" (between DB and Collector-2)
- Connection 3 has room "memdb" (between DB and Collector-3)

**These are three separate rooms**, even though they all have the same name. They exist on different connections.

---

## The Two Protocols

The network layer has **two distinct protocols** that must not be confused:

### Protocol A: HELLO (Meta-Protocol)

**Purpose**: Establish who is connecting and how to talk

**Scope**: Handles bytes, exists at the transport boundary

**Responsibilities**:
- Peer identity exchange (hostname, role)
- Protocol version negotiation
- Basic authentication/authorization (role-based)

**Implementation**: Part of transport-adjacent layer, handles serialization

**Key Point**: HELLO is NOT part of SessionManager. It happens BEFORE SessionManager sees anything.

### Protocol B: Room Communication (Application Protocol)

**Purpose**: Exchange typed messages between components

**Scope**: Completely transport-agnostic, only typed messages

**Responsibilities**:
- Publish which rooms each side offers
- Compute intersection (auto-join all matching rooms)
- Route typed messages to/from local components

**Implementation**: Handled by SessionManager (100% typed, no serialization knowledge)

**Key Point**: Protocol B operates on `TypedMessage` structs, never on bytes.

---

## The Core Component: SessionManager

This is the heart of the architecture and the most critical piece.

### What It Is

**SessionManager**: A transport-agnostic component that manages all peer connections and routes typed messages.

```rust
struct SessionManager {
    // One PeerSession per active connection
    peer_sessions: HashMap<PeerId, PeerSession>,
}

struct PeerSession {
    peer_id: PeerId,
    // Rooms auto-joined after PublishRooms negotiation
    rooms: HashMap<RoomId, RoomChannel>,
}

struct RoomChannel {
    room_id: RoomId,
    // Where to send inbound messages for this room (local component)
    local_handler: Recipient<TypedRoomMessage>,
    // Where to send outbound messages (to serialize + transport)
    remote_sender: Sender<TypedMessage>,
}
```

### Key Properties

1. **Transport-Agnostic**: Never touches bytes, serialization, or transport
2. **Typed Only**: All messages are Rust structs
3. **Testable**: Can connect two SessionManagers via mock channels (no network I/O)
4. **Per-Process Singleton**: One SessionManager manages all connections for a process

### What It Does

**Inbound Flow** (receiving from network):
```
??? (serialization layer)
    → SessionManager.dispatch_message(from_peer, room_id, typed_message)
    → local component's RoomChannel handler
```

**Outbound Flow** (sending to network):
```
Local component
    → SessionManager.send_to_room(peer_id, room_id, typed_message)
    → ??? (serialization layer)
```

**Lifecycle Management**:
```
Connection established
    → SessionManager.peer_connected(peer_id)
    → Exchange PublishRooms messages
    → Compute intersection of offered rooms
    → Auto-join all rooms in intersection
    → Send SessionActive events to local components

Connection terminated
    → SessionManager.peer_disconnected(peer_id)
    → Send SessionTerminated events to local components
```

### What It Does NOT Do

- ❌ Serialization (doesn't know about bytes)
- ❌ Transport (doesn't know about TCP/TLS/sockets)
- ❌ HELLO protocol (that's Protocol A, happens before SessionManager)
- ❌ Routing to multiple peers (rooms are 1:1 per connection)

---

## Room Negotiation: Auto-Join via Intersection

### The Process

1. **Boot Time**: Each process declares which rooms it offers
   ```rust
   // Collector offers:
   vec!["memdb", "health", "metrics"]

   // Database offers:
   vec!["memdb", "health", "admin"]
   ```

2. **Connection Time**: Both sides exchange PublishRooms messages (Protocol B)
   ```
   Collector → PublishRooms(["memdb", "health", "metrics"])
   Database  → PublishRooms(["memdb", "health", "admin"])
   ```

3. **Auto-Join**: Compute intersection, auto-join all matching rooms
   ```
   Intersection = ["memdb", "health"]

   Both sides automatically join "memdb" and "health" rooms
   No explicit join/subscribe mechanism needed
   ```

4. **Empty Intersection = Error**: If no common rooms, connection fails
   ```
   If intersection is empty:
       Log error: "No compatible rooms"
       Close connection
   ```

### Key Rules

- **Rooms are static**: Defined at boot time, never change during runtime
- **No dynamic subscription**: You can't join/leave rooms after negotiation
- **No negotiation UI**: If rooms don't match, connection fails immediately
- **Both sides auto-join**: No one-sided subscriptions

---

## Testing Without Transport: The Critical Validation

This is how we validate that the architecture is truly transport-agnostic.

### The Test Setup

```rust
#[test]
fn test_session_manager_communication() {
    // Create two SessionManagers (two processes in-memory)
    let manager_a = SessionManager::new();
    let manager_b = SessionManager::new();

    // Create mock connector (wires typed messages between them)
    let mock = MockConnector::new();
    mock.connect(manager_a.outbound(), manager_b.inbound());
    mock.connect(manager_b.outbound(), manager_a.inbound());

    // Simulate peer connection (no network!)
    manager_a.peer_connected("peer_b");
    manager_b.peer_connected("peer_a");

    // Publish rooms (Protocol B, typed messages)
    manager_a.handle_publish_rooms("peer_b", vec!["memdb", "health"]);
    manager_b.handle_publish_rooms("peer_a", vec!["memdb"]);

    // Intersection: ["memdb"] is now active

    // Register local handler for room "memdb" on manager_b
    let receiver = TestComponent::new();
    manager_b.register_room_handler("peer_a", "memdb", receiver.recipient());

    // Send typed message from manager_a to room "memdb"
    let msg = MemDBMessage::Query { id: 42 };
    manager_a.send_to_room("peer_b", "memdb", msg);

    // Should arrive at manager_b's receiver (no network I/O!)
    assert_eq!(receiver.received().id, 42);
}
```

### Why This Test is Critical

1. **Proves transport-agnostic design**: SessionManager works with ZERO network code
2. **Fast testing**: No TCP sockets, no waiting for timeouts
3. **Deterministic**: No race conditions from network timing
4. **Component isolation**: Can test component communication without integration tests

**If this test doesn't work, the architecture is wrong.**

---

## The Layer Boundaries

This is where things get serialized/deserialized and where transport happens.

### The Correct Layer Model

```
┌─────────────────────────────────────────────────────┐
│  Application Components (Business Logic)           │
│  - MemDB, IntentConfig, etc.                        │
│  - Same code on both sides                          │
│  - Registers with SessionManager for rooms          │
└─────────────────────────────────────────────────────┘
              ↕ (TypedMessage: Rust structs)
┌─────────────────────────────────────────────────────┐
│  SessionManager (Transport-Agnostic Core)           │
│  - Manages PeerSessions (one per connection)        │
│  - Routes typed messages to/from rooms              │
│  - Handles PublishRooms negotiation                 │
│  - 100% testable without network                    │
└─────────────────────────────────────────────────────┘
              ↕ (TypedMessage: Rust structs)
┌─────────────────────────────────────────────────────┐
│  Serialization Layer (Transport Boundary)           │
│  - Serializes TypedMessage → bytes                  │
│  - Deserializes bytes → TypedMessage                │
│  - May be part of HELLO handler or separate actor   │
└─────────────────────────────────────────────────────┘
              ↕ (Vec<u8>: bytes)
┌─────────────────────────────────────────────────────┐
│  HELLO Handler (Protocol A)                         │
│  - Handles HELLO handshake (peer identity)          │
│  - Negotiates protocol version                      │
│  - After success, passes control to SessionManager  │
└─────────────────────────────────────────────────────┘
              ↕ (Vec<u8>: bytes)
┌─────────────────────────────────────────────────────┐
│  Transport Layer (Pluggable)                        │
│  - TCP/TLS, mock, gRPC, etc.                        │
│  - Provides: send(bytes), recv() → bytes            │
│  - Completely swappable                             │
└─────────────────────────────────────────────────────┘
```

### Critical Boundaries

**The SessionManager Boundary (MOST IMPORTANT)**:
- **Above**: TypedMessage (Rust structs)
- **Below**: TypedMessage (Rust structs)
- **Key**: SessionManager NEVER crosses into bytes

**The Serialization Boundary**:
- **Above**: TypedMessage
- **Below**: Vec<u8>
- **Key**: This is where transport-agnostic becomes transport-specific

**The Transport Boundary**:
- **Above**: Vec<u8>
- **Below**: Network I/O (TCP, TLS, etc.)
- **Key**: This is completely pluggable

---

## Critical Design Decisions

### Decision 1: SessionManager is Transport-Agnostic

**Decision**: SessionManager operates entirely on typed messages, with zero knowledge of serialization or transport.

**Why**:
- Enables testing without network I/O
- Makes transport truly pluggable
- Simplifies reasoning about component communication
- Reduces dependencies dramatically

**Trade-off**: Requires a separate serialization layer between SessionManager and transport.

### Decision 2: Rooms Are Auto-Joined via Intersection

**Decision**: No explicit join/leave mechanism. Rooms are auto-joined based on intersection of offered rooms.

**Why**:
- Simpler protocol (no join/leave messages to handle)
- Boot-time validation of room compatibility
- Fail-fast if peers are incompatible
- No race conditions from dynamic join/leave

**Trade-off**: Less flexibility, but we don't need dynamic room management.

### Decision 3: Same Component Code on Both Sides

**Decision**: A component that communicates across the network uses the same code on both ends, just configured differently.

**Why**:
- All communication code in one place
- Easy to reason about both sides of protocol
- Natural symmetry in protocol design
- Simpler testing (instantiate component twice)

**Trade-off**: Components must be designed to work in both client and server roles.

### Decision 4: No Broadcast, Only Point-to-Point

**Decision**: Rooms are 1:1 channels. No broadcasting to multiple peers.

**Why**:
- Our use case doesn't need broadcast
- Simpler state management (no subscriber lists)
- Clear ownership of connections
- Application can implement multi-peer logic if needed

**Trade-off**: Can't easily send same message to multiple peers (but this is a feature, not a bug—forces explicit intent).

### Decision 5: Protocol A (HELLO) is Separate from Protocol B (Rooms)

**Decision**: HELLO handshake happens first (bytes), then control passes to SessionManager (typed).

**Why**:
- HELLO is transport-adjacent, needs to handle bytes
- SessionManager should never deal with serialization
- Clear separation of concerns
- HELLO is stable, Protocol B can evolve

**Trade-off**: Two distinct protocol layers to understand, but they have very different concerns.

---

## Common Misconceptions to Avoid

### ❌ Misconception 1: "Rooms are for broadcasting"

**Reality**: Rooms are 1:1 typed channels. If you need to send to multiple peers, you explicitly send to each one's room.

### ❌ Misconception 2: "SessionManager handles serialization"

**Reality**: SessionManager is 100% typed. Serialization happens in a separate layer below it.

### ❌ Misconception 3: "HELLO is part of SessionManager"

**Reality**: HELLO happens BEFORE SessionManager. It's part of the transport boundary.

### ❌ Misconception 4: "Different components on each side"

**Reality**: Same component code on both sides, just configured differently.

### ❌ Misconception 5: "You need transport to test SessionManager"

**Reality**: SessionManager should work perfectly with mock transport (no network I/O).

### ❌ Misconception 6: "Rooms can be joined/left dynamically"

**Reality**: Rooms are auto-joined at connection time based on intersection. Static afterwards.

### ❌ Misconception 7: "The router routes between peers"

**Reality**: The router dispatches messages to local components. Rooms are already peer-specific.

---

## Implementation Validation Checklist

Use this checklist to validate that an implementation follows the vision:

### SessionManager Validation

- [ ] SessionManager compiles without any transport crate dependency
- [ ] SessionManager has zero references to `Vec<u8>`, `bytes`, or serialization
- [ ] SessionManager can be instantiated with mock transport
- [ ] Two SessionManagers can communicate via in-memory channels (no network)
- [ ] All SessionManager methods accept/return typed messages only

### Room Validation

- [ ] Rooms are declared at boot time
- [ ] Room negotiation happens via PublishRooms message (Protocol B)
- [ ] Intersection of offered rooms is computed automatically
- [ ] All matching rooms are auto-joined (no explicit join message)
- [ ] Empty intersection causes connection to fail

### Component Validation

- [ ] Component's network code lives in the component's crate
- [ ] Component can be instantiated with different configs (client/server)
- [ ] Component registers its room handlers with SessionManager
- [ ] Component receives typed messages only (no bytes)

### Layer Boundary Validation

- [ ] Clear separation: SessionManager (typed) vs Serialization (bytes)
- [ ] HELLO handler is separate from SessionManager
- [ ] Transport is pluggable (can swap TCP for mock)
- [ ] Application components never directly touch transport

### Testing Validation

- [ ] Component communication can be tested without network I/O
- [ ] SessionManager can be tested with mock transport
- [ ] Protocol logic can be validated with two in-memory SessionManagers
- [ ] Integration tests with real TCP are minimal (smoke tests only)

---

## When to Refer to This Document

Use this document as the reference when:

1. **Designing a new component**: "Does my component follow the same-code-both-sides principle?"
2. **Reviewing architecture**: "Does this design respect SessionManager's transport-agnostic nature?"
3. **Implementing features**: "Am I putting serialization in the right layer?"
4. **Writing tests**: "Can I test this without network I/O?"
5. **Resolving confusion**: "What exactly is a room again?"
6. **Making trade-offs**: "What are the core principles I must not violate?"

**Golden Rule**: If your design makes it impossible to test two SessionManagers communicating via mock transport, you've violated the core vision.

---

## Conclusion

The ZZPing network layer vision is built on one core idea: **components communicate with typed messages, completely independent of how those messages are transported**.

SessionManager is the heart of this vision—a pure, typed, transport-agnostic component that can be tested without any network code. Everything else (HELLO, serialization, transport) exists to support SessionManager, not the other way around.

When in doubt, remember:
- Rooms are 1:1 typed channels
- SessionManager never touches bytes
- Same component code on both sides
- Test with mock transport first
- HELLO is separate from SessionManager

Follow these principles, and the architecture will guide you to the right implementation.
