# ZZPing Network Layer Architecture: Actor-Based Design

**Date**: October 1, 2025
**Status**: Active Design
**Authors**: David Martínez Martí, AI Design Partner (Claude 4.5 Sonnet)

---

## Document Metadata

### Supersedes

This document supersedes and clarifies the following previous design documents:

- **`ZZPing_ZzNet_Architecture_And_Design.md`** - Fully superseded. That document described an imperative API model (`request_channel`/`listen_for_channel`) and a facade pattern that has been identified as architecturally flawed. This document replaces it with a declarative, actor-based model.

- **`ZZPing_Component_Framework_Architecture.md`** - Partially superseded. The networking aspects are completely replaced by this document. The general component philosophy (isolation, testability, lifecycle management) remains valid, but the specific patterns (Builder → Wiring → Handle) are no longer emphasized as requirements—they were implementation patterns, not core needs.

- **`DESIGN_LOG.md`** - Extended and clarified. This document builds upon the three-crate architecture (`zznet-api`, `zznet`, `zznet-lib`) concept but reorganizes it around actors and adds critical missing pieces like per-connection session actors and explicit lifecycle management.

- **`ADR-001_ZzChorale_vs_CGP.md`** - Resolved. The decision to abandon the custom `zzchorale` framework in favor of the established `actix` actor framework for the actor model is now the foundation of this design. This document describes the architecture using `actix` as the actor runtime.

### Why a New Design Was Needed

The previous designs suffered from several fundamental issues that were uncovered through critical analysis:

1. **Confused Responsibilities**: The connection layer was conflating connection management, room routing, and application concerns. Subscriber maps were duplicated across layers, and it was unclear which component owned what responsibility.

2. **Hidden Lifecycle Complexity**: Connection lifecycle events were not explicitly propagated to application components, making stateful protocols difficult to implement correctly. Reconnections were treated ambiguously.

3. **Lack of Connection Awareness**: Application components had no clean way to maintain per-connection state, leading to patterns like `HashMap<ConnectionId, State>` with associated locking and complexity.

4. **Implementation Patterns Mistaken for Requirements**: Concepts like the three-phase lifecycle (Builder → Wiring → Handle) were described as requirements when they were actually just one possible implementation approach. This led to over-engineering and confusion about what was actually necessary.

5. **Insufficient Clarity on Core Concepts**: Terms like "Room" and "Session" were used ambiguously. The relationship between connections, rooms, and components was not clearly defined, leading to design confusion.

This document provides a clean, layered architecture with clear separation of concerns, explicit lifecycle management, and a connection-aware model that makes stateful protocols straightforward to implement.

---

## Executive Summary

This document defines the architecture for the **ZZPing network layer**, which enables components within a service to communicate with their counterparts in other services over the network.

**Core Insight**: Each network connection spawns a dedicated **session actor** within each application component. This per-connection actor pattern eliminates shared state, provides explicit lifecycle management, and makes connection-aware protocols natural to implement.

**Key Architectural Layers**:
1. **Transport Layer**: Abstract, pluggable transport (TCP/TLS, gRPC, etc.)
2. **Session Layer**: Protocol state machine (handshake, room negotiation, frame multiplexing)
3. **Routing Layer**: Routes session events to registered component handlers
4. **Application Layer**: Business logic components with main + per-connection session actors

**What This Enables**:
- Pure actor model with no shared state or locking
- Self-testable components with minimal dependencies
- Clean handling of connection lifecycle in stateful protocols
- Transport-agnostic design (swap TCP for gRPC without touching app code)
- Symmetric protocol (same code runs on client and server)

---

## Core Requirements

These are the fundamental, non-negotiable requirements. They describe **what** the system must do, not **how** it does it. Design decisions that follow must satisfy all of these requirements.

### R1: Pure Actor Model for Component Isolation
Components must be implemented as isolated actors. Communication between components occurs exclusively through message passing. No shared mutable state. No `Arc<Mutex<T>>` patterns (which are code smells indicating architectural problems).

**Rationale**: Enables independent testing, eliminates race conditions, and provides clear concurrency semantics.

### R2: Components Must Be Self-Testable with Minimal Dependencies
Each component must be testable in isolation with mock dependencies. A component's networking code must be self-contained within the same crate as its business logic.

**Rationale**: Enables rapid iteration and confident refactoring. Developers should not need to understand the entire system to test one component.

### R3: Components Must Work Without Active Connections (0 to N)
A component must function correctly whether it has zero, one, or many active network connections. Components must not assume connection availability.

**Rationale**: Enables offline operation, graceful degradation, and resilience to network failures. Components should buffer, queue, or defer work as appropriate to their domain logic.

### R4: Components Must Be Connection-Aware for Stateful Protocols
Components implementing stateful protocols must receive explicit notification of connection lifecycle events (connection established, connection terminated). Components must be able to maintain independent state per connection.

**Rationale**: Many protocols require negotiation or synchronization when connections are established. Hiding connection lifecycle creates ambiguity and makes correct implementation difficult. For example, a config sync component must re-send initial state to each new connection.

### R5: Symmetric Protocol (Same Code for Client and Server)
The same component code must work on both the client side (initiating connections) and the server side (accepting connections). The only difference should be configuration (e.g., "connect to X" vs. "listen on Y").

**Rationale**: Reduces code duplication, simplifies testing, and ensures protocol compatibility. If both sides use the same state machine, protocol bugs are caught immediately.

### R6: Transport Agnostic
The connection and application layers must not depend on a specific transport implementation. It must be possible to swap TCP/TLS for gRPC, or add a new transport, without modifying application component code.

**Rationale**: Enables experimentation with different transports, supports diverse deployment environments, and prevents vendor lock-in.

### R7: No Heavy Buffering in Network Layer
The network layer (session and routing layers) must not implement application-level buffering policies. Buffering decisions are the responsibility of application components.

**Rationale**: Different applications have different buffering needs (e.g., drop old data vs. apply backpressure). The network layer should not make these policy decisions. Small performance-oriented buffers (e.g., batching 3 messages) are acceptable.

### R8: Static Room List at Boot Time
The set of "rooms" (logical service types) that a service supports must be known at boot time and must not change during runtime. Dynamic addition or removal of room types is not required.

**Rationale**: Simplifies the design significantly. Room lists can be validated at startup. Runtime changes add complexity for no clear benefit in our use case.

### R9: Reconnection = New Connection (No Identity Persistence at Protocol Level)
When a connection is lost and re-established, the protocol layer must treat it as a completely new connection with a fresh connection ID. The protocol must not attempt to restore previous connection state.

**Rationale**: In our domain, a disconnection almost always indicates a process restart (not a transient network flap), which means state has been lost. Attempting to restore state creates a dangerous illusion of continuity. Application components can implement their own reconnection logic at a higher level if needed.

### R10: Component Networking Code Is Self-Contained
A component's networking code (message types, serialization, protocol logic) must reside in the same crate as its business logic. A developer working on a component should not need to navigate across multiple crates to understand its network behavior.

**Rationale**: Improves code locality, reduces cognitive load, and makes components truly self-contained units. If a component talks to itself across the network, all that code should be in one place.

---

## Critical Clarifications

These are answers to questions that inevitably arise when reading the requirements. They clarify common sources of confusion and provide precise definitions of ambiguous terms.

### What Is a "Room"?

A **room** is a per-connection, 1:1 logical communication channel between two specific component instances.

**It is NOT**:
- A shared broadcast channel (like an IRC room)
- A multiplexed group communication mechanism
- A persistent entity that survives connection loss

**It IS**:
- A type-safe communication path for one specific protocol (e.g., "intent-config")
- Tied to a single, specific TCP connection
- Unique per connection (Connection A has its own "intent-config" room, Connection B has a separate one)

**Better Mental Model**: Think of a room as a "phone line" between two processes, not a "conference call". Each connection is a separate line.

**Naming Note**: The term "Room" was chosen because it's established in the codebase. A more precise term might be "ConnectionRoom" or "ServiceChannel", but changing terminology now would create more confusion than it solves. Just remember: rooms are per-connection and 1:1.

### What Does "Connection-Aware" Mean?

**Connection-aware** means that application components receive explicit events when connections are established and terminated, and they can maintain separate state per connection.

**Example**: An `IntentConfigActor` on the server side:
- Receives `SessionActive { connection_id: 42, ... }` when Collector-1 connects
- Spawns a session actor to handle connection #42
- Sends initial config state to connection #42
- Receives `SessionTerminated { connection_id: 42 }` when Collector-1 disconnects
- Cleans up any per-connection state for #42

**Why This Matters**: Without connection awareness, the component would blindly send messages without knowing if they're going to an old, dead connection or a new one. Stateful protocols require explicit knowledge of connection boundaries.

### How Does Reconnection Work?

**It doesn't.** There is no "reconnection" at the protocol layer.

When a TCP connection is lost:
1. The session actor for that connection stops
2. Application components receive `SessionTerminated { connection_id: 42 }`
3. Per-connection state is cleaned up
4. **End of story**

When a new TCP connection is established (even from the same remote host):
1. A **new** session actor is spawned with a **new**, unique `connection_id` (e.g., 87)
2. Application components receive `SessionActive { connection_id: 87, ... }`
3. The protocol negotiation happens from scratch
4. Application components re-establish any necessary state for this **new** connection

**Rationale**: In our deployment model, a disconnection almost always means the remote process restarted. The remote state is gone. Treating it as a "reconnection" and attempting to resume creates a dangerous illusion of continuity. It's safer and clearer to start fresh.

**Higher-Level Identity**: Application components **may** implement their own notion of identity (e.g., via hostname in mTLS certificate) and maintain cross-connection state at a higher level, but this is application logic, not protocol logic.

### Who Buffers Messages?

**Application components decide their own buffering policy.** The network layer does not buffer.

**Example Policies**:
- `PingerActor` might drop new pings if the send buffer is full (drop-newest policy)
- `IntentConfigActor` might keep only the latest config update (replace-old policy)
- `BatchCollectorActor` might apply backpressure and block until space is available

**Network Layer's Role**: The network layer may use small, performance-oriented buffering (e.g., batching a few messages to reduce syscalls), but it does not implement application-level queueing or buffering strategies.

**Rationale**: Buffering policy is domain-specific. A ping monitor has different needs than a file transfer. The network layer should not make these decisions.

### What About Multiple Connections from the Same "Identity"?

**They are different processes.** The protocol treats them as completely independent connections.

**Example Scenario**: Collector-1 has an active connection (#42) to the database. An operator starts a second instance of the collector process (same hostname, same config) to perform a handoff/takeover. The second instance creates connection #87.

**From zznet's perspective**: These are two separate connections, two separate sets of session actors, two independent protocol state machines. The protocol does not attempt to correlate them.

**From the application's perspective**: The `DatabaseOrchestrator` component (business logic) might recognize via hostnames that both connections claim to be "Collector-1" and implement handoff logic. But this is application-level orchestration, not protocol-level behavior.

**Rationale**: Attempting to correlate connections at the protocol level adds immense complexity for unclear benefit. Let application components make identity decisions based on their domain knowledge.

### How Is Serialization Handled?

**Two-stage serialization**:

1. **Stage 1 (Application → Room)**: Application message is serialized to `Vec<u8>`
   ```rust
   let msg = IntentConfigMessage::UpdateConfig { ... };
   let bytes = bincode::serialize(&msg)?; // Stage 1
   ```

2. **Stage 2 (Room → Transport)**: Bytes are wrapped in a protocol frame and serialized again
   ```rust
   let frame = Frame::MessageForRoom { room: "intent-config", data: bytes };
   let frame_bytes = bincode::serialize(&frame)?; // Stage 2
   ```

**Why Two Stages?**
- The session layer cannot deserialize application messages because it doesn't know their types (they're in different crates)
- The session layer needs to route based on room name before deserializing the payload
- This is a necessary consequence of the layered, type-safe design

**Trade-offs**:
- ✅ Clean separation of concerns
- ✅ Type safety at every layer
- ✅ Transport and protocol are decoupled from application types
- ⚠️ Slight overhead from double serialization (acceptable in our latency-tolerant domain)
- ⚠️ Allows different serialization formats for protocol vs. application (flexibility, but also potential confusion)

**Decision**: We accept this trade-off. The architectural clarity gained is worth the minor performance cost.

---

## Key Architectural Insights

These are the critical "aha moments" that led to this design. Understanding these insights helps understand why the architecture is structured the way it is.

### Insight 1: Duplicated State Is a Code Smell

**The Problem**: In the previous design, `ZzNetConnManager` held a `HashMap<String, RoomSubscribers>`, and every `ZzNetConnActor` received a **cloned copy** of this entire map.

**The Symptom**: This duplication felt wrong, but it was hard to articulate why.

**The Diagnosis**: Duplication of data structures almost always indicates confused responsibilities. If two components need the same data, either:
1. They should be the same component, or
2. One should be the source of truth and publish events to the other

**The Insight**: The connection actor should not "know" about subscribers. It should only know about protocol state and frame routing. The routing of messages to subscribers is a separate concern that belongs in a separate layer.

**The Fix**: Introduce a dedicated `RoomRouter` that owns subscriber information. Connection actors publish events; the router subscribes and dispatches.

### Insight 2: Per-Connection Session Actors Eliminate Shared State

**The Problem**: If a single `MainActor` has to handle multiple connections, it needs a `HashMap<u64, ConnectionState>`. Accessing this requires either:
- `Arc<Mutex<HashMap<...>>>` (lock contention, runtime overhead)
- Messaging patterns (complex, hard to get right)

**The Insight**: Each connection's state can be isolated in its own actor. When a connection becomes active, spawn a `SessionActor` for it. When the connection dies, the actor stops automatically.

**The Result**:
- No shared state between connections
- No mutexes or atomic operations
- Lifecycle is automatic (actor lifetime = connection lifetime)
- Each session actor is a simple, single-connection state machine

**Why This Works**: Actix actors are lightweight. Spawning one per connection is not expensive, and the architectural clarity gained is enormous.

### Insight 3: Connection Layer Should Be Dumb Pipes

**The Problem**: The previous design had the connection layer directly routing messages to application actors. This created tight coupling.

**The Insight**: The **session layer** should only know about:
- Handshake protocol
- Frame serialization
- Room negotiation
- Multiplexing messages by room name

It should **not** know about:
- Which application components exist
- How to deserialize application messages
- What to do with the messages (that's routing logic)

**The Result**: The session layer publishes events (connection active, data received, connection terminated). A separate **routing layer** subscribes to these events and dispatches to application components.

**Analogy**: The session layer is like a post office that routes packages by zip code. It doesn't know what's inside the packages or who the recipients are—that's the router's job.

### Insight 4: Main Actor + Session Actors = Clean Lifecycle

**The Pattern**:
- **Main Actor**: Singleton, holds global state, lives for the lifetime of the service
- **Session Actors**: One per connection, holds per-connection state, lives for the lifetime of the connection

**Why This Is Powerful**:
- Main actor doesn't need to track "active" vs "dead" connections—each session actor is either running (active) or stopped (dead)
- No cleanup logic needed—when a session actor stops, its state is automatically dropped
- No risk of accessing stale state—if you have an `Addr<SessionActor>`, it's valid

**Example**:
```rust
impl Handler<SessionActive> for IntentConfigActor {
    fn handle(&mut self, msg: SessionActive, ctx: &mut Context<Self>) {
        // Spawn a dedicated session actor for this connection
        let session = IntentConfigSessionActor::new(
            msg.connection_id,
            ctx.address(),
            msg.session_handle,
        ).start();

        // Optionally track it if we need to send it messages later
        self.sessions.insert(msg.connection_id, session);
    }
}

impl Handler<SessionTerminated> for IntentConfigActor {
    fn handle(&mut self, msg: SessionTerminated, _ctx: &mut Context<Self>) {
        // Just remove from tracking—the actor already stopped automatically
        self.sessions.remove(&msg.connection_id);
        // Any per-connection cleanup logic here
    }
}
```

### Insight 5: Registration-Based Wiring Avoids Chicken-and-Egg

**The Problem**: If you try to wire actors by passing addresses during construction, you often get circular dependencies:
- To create Actor A, you need the address of Actor B
- To create Actor B, you need the address of Actor A
- 💥 Impossible

**The Solution**: Use **registration** instead of **injection**:
1. Create all actors first (they start in a "not yet wired" state)
2. Create a central registry (e.g., `RoomRouter`)
3. Each actor registers itself with the registry
4. Start accepting connections

**Why This Works**: The registry is a simple, passive component. It has no dependencies. Actors can register in any order. No circular dependencies possible.

**Boot Sequence**:
```rust
// 1. Create all actors (no dependencies on each other yet)
let intent_config = IntentConfigActor::new(...).start();
let pinger = PingerActor::new(...).start();

// 2. Create router (no dependencies)
let router = RoomRouter::new().start();

// 3. Actors register themselves
router.do_send(RegisterRoom {
    room_name: "intent-config".to_string(),
    handler: intent_config.recipient(),
});
router.do_send(RegisterRoom {
    room_name: "pinger".to_string(),
    handler: pinger.recipient(),
});

// 4. Create session manager (depends on router)
let session_mgr = SessionManager::new(router.recipient()).start();

// 5. Attach transport
let transport = TcpTransport::new(...);
session_mgr.do_send(AttachTransport { transport });
```

**No chicken-and-egg problem because actors are created first, dependencies are established second.**

---

## Layered Architecture

The network layer is organized into four distinct layers, each with a clear responsibility. Data flows vertically through these layers. Each layer only communicates with its immediate neighbors.

```
┌──────────────────────────────────────────────────────────┐
│            APPLICATION LAYER                             │
│  ┌────────────────────┐  ┌────────────────────┐         │
│  │ IntentConfigActor  │  │ PingerActor        │         │
│  │ (main, singleton)  │  │ (main, singleton)  │         │
│  └────────────────────┘  └────────────────────┘         │
│         ↕                        ↕                        │
│  ┌────────────────────┐  ┌────────────────────┐         │
│  │ SessionActor       │  │ SessionActor       │         │
│  │ (per connection)   │  │ (per connection)   │         │
│  └────────────────────┘  └────────────────────┘         │
└──────────────────────────────────────────────────────────┘
                    ↕ (typed messages)
┌──────────────────────────────────────────────────────────┐
│             ROUTING LAYER                                │
│  ┌──────────────────────────────────────────┐            │
│  │          RoomRouter                      │            │
│  │  Routes events to registered handlers    │            │
│  └──────────────────────────────────────────┘            │
└──────────────────────────────────────────────────────────┘
                    ↕ (Vec<u8> + metadata)
┌──────────────────────────────────────────────────────────┐
│             SESSION LAYER                                │
│  ┌────────────────────┐                                  │
│  │  SessionManager    │                                  │
│  │  (singleton)       │                                  │
│  └────────────────────┘                                  │
│         ↕                                                 │
│  ┌────────────────────┐  ┌────────────────────┐         │
│  │  SessionActor      │  │  SessionActor      │         │
│  │  (per connection)  │  │  (per connection)  │         │
│  └────────────────────┘  └────────────────────┘         │
└──────────────────────────────────────────────────────────┘
                    ↕ (frames)
┌──────────────────────────────────────────────────────────┐
│             TRANSPORT LAYER                              │
│  ┌──────────────────────────────────────────┐            │
│  │   TcpTransport / GrpcTransport / etc.    │            │
│  │   (pluggable implementation)             │            │
│  └──────────────────────────────────────────┘            │
└──────────────────────────────────────────────────────────┘
```

### Layer Responsibilities

#### Application Layer
**What it does**:
- Implements business logic
- Maintains global and per-connection state
- Handles serialization/deserialization of application messages
- Spawns and manages session actors

**What it knows about**:
- Its own message types (e.g., `IntentConfigMessage`)
- Connection lifecycle (via events from routing layer)
- How to respond to protocol-specific requests

**What it does NOT know about**:
- Transport implementation (TCP vs. gRPC)
- Protocol framing or handshake details
- Other application components

#### Routing Layer
**What it does**:
- Maintains a registry of room name → handler mappings
- Routes `SessionActive`, `SessionTerminated`, and `DataForRoom` events to the appropriate application components
- Provides the "switchboard" between session layer and application layer

**What it knows about**:
- Which components are registered for which rooms
- How to dispatch events to handlers

**What it does NOT know about**:
- The content of messages (just passes `Vec<u8>`)
- Application-specific logic
- Transport or protocol details

#### Session Layer
**What it does**:
- Manages the zznet protocol state machine (handshake, room negotiation)
- Multiplexes/demultiplexes frames by room name
- Spawns a `SessionActor` per transport connection
- Publishes lifecycle events (connection active, terminated)

**What it knows about**:
- Protocol message types (`Hello`, `PublishRooms`, `MessageForRoom`)
- Frame serialization format
- Connection state (awaiting handshake, active, etc.)

**What it does NOT know about**:
- Application message types
- Business logic
- Which components are subscribed to which rooms

#### Transport Layer
**What it does**:
- Provides an abstract interface for bidirectional byte streams
- Handles physical connection establishment (TCP socket, TLS handshake, etc.)
- May provide connection pooling, reconnection, etc. (transport-specific)

**What it knows about**:
- Network I/O primitives
- Transport-specific configuration (TLS certificates, timeouts, etc.)

**What it does NOT know about**:
- Protocol framing or content
- Application logic

---

## Component Design Patterns

These patterns are used consistently across all application components in the system. By following these patterns, components gain automatic benefits (clean lifecycle, testability, etc.).

### Pattern 1: Main Actor (Singleton)

**Purpose**: Holds global state that spans all connections.

**Characteristics**:
- One instance per service
- Lives for the lifetime of the service
- Handles events from the routing layer
- Spawns session actors when connections become active

**Typical State**:
```rust
pub struct IntentConfigActor {
    // Global state (e.g., the current configuration)
    current_config: Config,

    // Track active session actors (optional, if main actor needs to send them messages)
    sessions: HashMap<u64, Addr<IntentConfigSessionActor>>,
}
```

**Typical Message Handlers**:
```rust
// From the routing layer
impl Handler<SessionActive> for IntentConfigActor { ... }
impl Handler<SessionTerminated> for IntentConfigActor { ... }

// From local actors (intra-process communication)
impl Handler<LocalRequest> for IntentConfigActor { ... }

// From session actors (reports from connections)
impl Handler<RemoteMessage> for IntentConfigActor { ... }
```

### Pattern 2: Session Actor (Per-Connection)

**Purpose**: Manages state for a single, specific network connection.

**Characteristics**:
- One instance per active connection
- Lifetime tied to connection lifetime (stops automatically when connection dies)
- Owns the `SessionHandle` (the send channel to the network)
- Handles serialization and deserialization
- Acts as a bridge between the main actor and the network

**Typical State**:
```rust
pub struct IntentConfigSessionActor {
    // Which connection this session represents
    connection_id: u64,

    // Handle to the main actor (to send reports, requests)
    main_actor: Addr<IntentConfigActor>,

    // Handle to send data back to the network
    session_handle: SessionHandle,

    // Per-connection state
    has_received_initial_sync: bool,
    pending_ack: Option<MessageId>,
}
```

**Typical Message Handlers**:
```rust
// From the routing layer (incoming data from network)
impl Handler<DataForRoom> for IntentConfigSessionActor {
    fn handle(&mut self, msg: DataForRoom, _ctx: &mut Context<Self>) {
        // Deserialize
        let typed_msg: IntentConfigMessage = bincode::deserialize(&msg.data)?;

        // Handle or forward to main actor
        match typed_msg {
            IntentConfigMessage::GetConfig => {
                // Respond directly
                self.send_to_network(IntentConfigMessage::ConfigResponse { ... });
            }
            IntentConfigMessage::UpdateConfig { config } => {
                // Forward to main actor
                self.main_actor.do_send(ConfigUpdate { config });
            }
        }
    }
}

// From the main actor (outgoing data to network)
impl Handler<SendToRemote> for IntentConfigSessionActor {
    fn handle(&mut self, msg: SendToRemote, _ctx: &mut Context<Self>) {
        // Serialize
        let bytes = bincode::serialize(&msg.data)?;

        // Send to network
        self.session_handle.send("intent-config", bytes);
    }
}
```

### Pattern 3: Lifecycle Management

**Connection Established Flow**:
```
1. Transport provides new connection
2. SessionManager spawns SessionActor (protocol layer)
3. SessionActor completes handshake and room negotiation
4. SessionActor publishes SessionActive event
5. RoomRouter routes event to registered handlers
6. IntentConfigActor receives SessionActive
7. IntentConfigActor spawns IntentConfigSessionActor
8. IntentConfigSessionActor sends initial sync to remote peer
```

**Connection Terminated Flow**:
```
1. Transport connection dies
2. SessionActor (protocol layer) detects and stops
3. Before stopping, SessionActor publishes SessionTerminated event
4. RoomRouter routes event to registered handlers
5. IntentConfigActor receives SessionTerminated
6. IntentConfigActor sends Stop message to IntentConfigSessionActor
7. IntentConfigSessionActor stops and cleans up
8. Main actor removes session from tracking
```

**Key Point**: Lifecycle is **explicit and observable**. Every transition is a message. No hidden state.

---

## Crate Organization

The system is organized into four crates, each corresponding to one of the architectural layers. This organization ensures clean dependency graphs and prevents circular dependencies.

### Crate 1: `zznet-transport` (Abstraction/Trait)

**Location**: `src/common/zznet-transport/` (or possibly just a trait in `zznet-session`)

**Purpose**: Define the abstract interface for transport connections.

**Contents**:
```rust
/// Represents a bidirectional byte stream from a transport layer.
pub trait TransportConnection: Send {
    /// Send a frame (length-prefixed or otherwise delimited bytes)
    async fn send(&mut self, data: Vec<u8>) -> Result<()>;

    /// Receive a frame. Returns None when connection is closed.
    async fn recv(&mut self) -> Result<Option<Vec<u8>>>;
}

/// A factory that provides transport connections.
pub trait Transport: Send {
    /// Get the next incoming connection (for servers)
    async fn accept(&mut self) -> Result<Box<dyn TransportConnection>>;

    /// Or, for clients:
    async fn connect(&self, addr: &str) -> Result<Box<dyn TransportConnection>>;
}
```

**Dependencies**: None (or only `tokio`, `anyhow`)

**Implementations** (separate crates or modules):
- `TcpTransport`: TCP sockets with optional TLS
- `MockTransport`: In-memory pipes for testing
- (Future) `GrpcTransport`, `QuicTransport`, etc.

### Crate 2: `zznet-session` (Protocol Engine)

**Location**: `src/components/zznet-session/`

**Purpose**: Implement the zznet handshake and room negotiation protocol. Manage protocol state machines for each connection.

**Key Types**:

```rust
/// Singleton actor that spawns SessionActor for each new transport connection
pub struct SessionManager {
    router: Recipient<SessionEvent>,
}

/// Per-connection actor that manages protocol state
pub struct SessionActor {
    transport: Box<dyn TransportConnection>,
    state: ProtocolState,
    active_rooms: Vec<String>,
    manager: Addr<SessionManager>,
}

/// Protocol state machine
enum ProtocolState {
    AwaitingHandshake,
    AwaitingRoomList,
    Active,
}

/// Protocol message types
#[derive(Serialize, Deserialize)]
pub enum Frame {
    Hello {
        version: String,
        auth_role: AuthRole,
        offered_rooms: Vec<String>,
    },
    PublishRooms {
        offered_rooms: Vec<String>,
    },
    MessageForRoom {
        room: String,
        data: Vec<u8>,
    },
}

/// Handle for sending data to a specific connection
pub struct SessionHandle {
    connection_id: u64,
    sender: mpsc::Sender<(String, Vec<u8>)>,
}

/// Events published by SessionActor
#[derive(Message)]
pub enum SessionEvent {
    Active {
        connection_id: u64,
        rooms: Vec<String>,
        handle: SessionHandle,
    },
    DataForRoom {
        connection_id: u64,
        room: String,
        data: Vec<u8>,
    },
    Terminated {
        connection_id: u64,
    },
}
```

**Dependencies**: `zznet-transport`, `actix`, `serde`, `bincode`

**Responsibilities**:
- Execute handshake protocol
- Negotiate active rooms (intersection of local and remote offered rooms)
- Multiplex outgoing messages by room name
- Demultiplex incoming messages by room name
- Publish lifecycle events to router

**What it does NOT do**:
- Deserialize application messages (only knows about `Vec<u8>`)
- Route messages to specific components (publishes events to router)
- Implement business logic

### Crate 3: `zznet-router` (Message Routing)

**Location**: `src/components/zznet-router/`

**Purpose**: Act as a switchboard that routes session events to registered application component handlers.

**Key Types**:

```rust
/// Central routing actor
pub struct RoomRouter {
    /// Maps room name -> handler address
    handlers: HashMap<String, Recipient<SessionEvent>>,
}

/// Registration message (sent during boot)
#[derive(Message)]
pub struct RegisterRoom {
    pub room_name: String,
    pub handler: Recipient<SessionEvent>,
}
```

**Dependencies**: `zznet-session` (for `SessionEvent` type), `actix`

**Logic**:
```rust
impl Handler<SessionEvent> for RoomRouter {
    fn handle(&mut self, msg: SessionEvent, _ctx: &mut Context<Self>) {
        match &msg {
            SessionEvent::Active { rooms, .. } => {
                // Notify handlers for all active rooms
                for room in rooms {
                    if let Some(handler) = self.handlers.get(room) {
                        handler.do_send(msg.clone());
                    }
                }
            }
            SessionEvent::DataForRoom { room, .. } => {
                // Route to specific room handler
                if let Some(handler) = self.handlers.get(room) {
                    handler.do_send(msg);
                }
            }
            SessionEvent::Terminated { .. } => {
                // Broadcast to all registered handlers
                for handler in self.handlers.values() {
                    handler.do_send(msg.clone());
                }
            }
        }
    }
}
```

**Why a separate crate?** Keeps the router's responsibilities explicit and testable in isolation. Also makes it easy to swap routing strategies (e.g., add filtering, priorities, etc.) without touching session or application code.

### Crate 4: Application Components (e.g., `intent-config`)

**Location**: `src/actors/intent-config/` or `src/components/intent-config/`

**Purpose**: Implement domain-specific business logic.

**Key Types**:

```rust
/// Main actor (singleton)
pub struct IntentConfigActor {
    current_config: Config,
    sessions: HashMap<u64, Addr<IntentConfigSessionActor>>,
}

/// Session actor (per connection)
pub struct IntentConfigSessionActor {
    connection_id: u64,
    main_actor: Addr<IntentConfigActor>,
    session_handle: SessionHandle,
}

/// Application-specific messages
#[derive(Serialize, Deserialize)]
pub enum IntentConfigMessage {
    GetConfig,
    UpdateConfig { config: Config },
    ConfigResponse { config: Config },
}
```

**Dependencies**: `zznet-session` (for `SessionEvent`, `SessionHandle`), `zznet-router` (for `RegisterRoom`), `actix`, `serde`

**Registration at Boot**:
```rust
pub struct IntentConfigComponent {
    main_actor: Addr<IntentConfigActor>,
}

impl IntentConfigComponent {
    pub fn new() -> Self {
        let actor = IntentConfigActor::new().start();
        Self { main_actor: actor }
    }

    pub fn register_with_router(&self, router: &Addr<RoomRouter>) {
        router.do_send(RegisterRoom {
            room_name: "intent-config".to_string(),
            handler: self.main_actor.clone().recipient(),
        });
    }
}
```

---

## Message Flows

These diagrams show the detailed flow of messages through the system for key scenarios.

### Flow 1: Boot Sequence

```
Service Main
   │
   ├─1─> Create IntentConfigActor
   │         └─> (starts, idle)
   │
   ├─2─> Create PingerActor
   │         └─> (starts, idle)
   │
   ├─3─> Create RoomRouter
   │         └─> (starts, empty registry)
   │
   ├─4─> IntentConfigActor.register_with_router(router)
   │         └─> RoomRouter receives RegisterRoom { "intent-config", handler }
   │              └─> handlers["intent-config"] = IntentConfigActor.recipient()
   │
   ├─5─> PingerActor.register_with_router(router)
   │         └─> RoomRouter receives RegisterRoom { "pinger", handler }
   │              └─> handlers["pinger"] = PingerActor.recipient()
   │
   ├─6─> Create SessionManager(router.recipient())
   │         └─> (starts, waiting for transport)
   │
   └─7─> SessionManager.attach_transport(TcpTransport)
             └─> Begins listening for connections
```

**Key Point**: All actors are created first, then wired via registration. No circular dependencies.

### Flow 2: Connection Establishment (Client Perspective)

```
TcpTransport                SessionManager          SessionActor        RoomRouter          IntentConfigActor
     │                            │                       │                  │                       │
     │─────connect()────────────>│                       │                  │                       │
     │                            │                       │                  │                       │
     │<────TransportConn──────────│                       │                  │                       │
     │                            │                       │                  │                       │
     │                            │──spawn()───────────>│                  │                       │
     │                            │                       │                  │                       │
     │                            │                       │──send(Hello)──>│                       │
     │                            │                       │                  │                       │
     │<─────────────────────recv(Hello)──────────────────│                  │                       │
     │                            │                       │                  │                       │
     │──send(PublishRooms)───────────────────────────>│                  │                       │
     │                            │                       │                  │                       │
     │<────recv(PublishRooms)────────────────────────────│                  │                       │
     │                            │                       │                  │                       │
     │                            │                       │ (negotiates active rooms)               │
     │                            │                       │                  │                       │
     │                            │                       │─SessionActive──>│                       │
     │                            │                       │                  │                       │
     │                            │                       │                  │──SessionActive──────>│
     │                            │                       │                  │                       │
     │                            │                       │                  │                       │<─spawn(SessionActor)
     │                            │                       │                  │                       │
```

**Notes**:
- Handshake is symmetric (both sides send Hello, both send PublishRooms)
- Active rooms = intersection of offered rooms from both sides
- SessionActive event carries the `SessionHandle` for sending data back

### Flow 3: Data Flow (Inbound)

```
Transport          SessionActor(protocol)     RoomRouter         IntentConfigActor    IntentConfigSessionActor
    │                      │                       │                     │                        │
    │─recv(frame)────────>│                       │                     │                        │
    │                      │                       │                     │                        │
    │                      │ (deserialize Frame)   │                     │                        │
    │                      │                       │                     │                        │
    │                      │─DataForRoom(Vec<u8>)─>│                     │                        │
    │                      │                       │                     │                        │
    │                      │                       │─DataForRoom(Vec<u8>)───>│                   │
    │                      │                       │                     │                        │
    │                      │                       │                     │──DataForRoom(Vec<u8>)─>│
    │                      │                       │                     │                        │
    │                      │                       │                     │                        │ (deserialize app msg)
    │                      │                       │                     │                        │
    │                      │                       │                     │                        │ (process business logic)
    │                      │                       │                     │                        │
    │                      │                       │                     │<────RemoteMessage──────│
    │                      │                       │                     │                        │
    │                      │                       │                     │ (main actor decides response)
```

**Key Point**: Each layer deserializes only what it needs to know. Protocol layer deserializes the frame envelope. Session actor deserializes the application message.

### Flow 4: Data Flow (Outbound)

```
IntentConfigActor    IntentConfigSessionActor    SessionActor(protocol)    Transport
       │                        │                         │                    │
       │─SendToRemote(msg)────>│                         │                    │
       │                        │                         │                    │
       │                        │ (serialize app msg)     │                    │
       │                        │                         │                    │
       │                        │─send("intent-config", Vec<u8>)──>│          │
       │                        │                         │                    │
       │                        │                         │ (wrap in Frame)    │
       │                        │                         │                    │
       │                        │                         │ (serialize Frame)  │
       │                        │                         │                    │
       │                        │                         │─send(bytes)──────>│
       │                        │                         │                    │
```

### Flow 5: Connection Termination

```
Transport    SessionActor(protocol)    RoomRouter    IntentConfigActor    IntentConfigSessionActor
    │                 │                     │                │                       │
    │─recv() = None──>│                     │                │                       │
    │                 │                     │                │                       │
    │                 │ (detect disconnect) │                │                       │
    │                 │                     │                │                       │
    │                 │─SessionTerminated──>│                │                       │
    │                 │                     │                │                       │
    │                 │                     │─SessionTerminated──>│                  │
    │                 │                     │                │                       │
    │                 │                     │                │──Stop────────────────>│
    │                 │                     │                │                       │
    │                 │──stop()             │                │                       │<──stopped()
    │                 │                     │                │                       │
    │                 │<──stopped()         │                │                       │
    │                 │                     │                │<──clean up state─────│
```

**Key Point**: Termination is cascading and explicit. Protocol session actor stops first, publishes event, then application session actor stops.

---

## Testing Strategy

A core requirement (R2) is that components must be self-testable. This section describes how the architecture enables comprehensive testing at multiple levels.

### Level 1: Unit Tests (Individual Actor Logic)

**What**: Test a single actor's message handlers in isolation.

**How**: Use mock recipients for dependencies.

```rust
#[actix::test]
async fn test_intent_config_handles_update() {
    // Create the actor under test
    let actor = IntentConfigActor::new().start();

    // Create a mock session handle
    let (tx, rx) = mpsc::channel(10);
    let mock_handle = SessionHandle::new(42, tx);

    // Simulate a session becoming active
    actor.do_send(SessionActive {
        connection_id: 42,
        rooms: vec!["intent-config".to_string()],
        handle: mock_handle,
    }).await.unwrap();

    // Verify session actor was spawned (check actor's internal state)
    // ...

    // Simulate receiving a config update
    actor.do_send(ConfigUpdate { config: new_config }).await.unwrap();

    // Verify message was sent to session actor
    let sent = rx.try_recv().unwrap();
    // assert on sent data
}
```

**Advantages**:
- Fast (no network I/O)
- Isolated (failures don't cascade)
- Easy to set up edge cases

### Level 2: Integration Tests (Full Stack with Mock Transport)

**What**: Test the complete interaction between session layer, router, and application components, using an in-memory mock transport.

**How**: Create two full stacks (client and server) and connect them with in-memory pipes.

```rust
#[actix::test]
async fn test_full_handshake_and_message_exchange() {
    // Create a mock transport (in-memory bidirectional pipe)
    let (client_conn, server_conn) = mock_transport::create_pipe();

    // Set up server stack
    let server_router = RoomRouter::new().start();
    let server_intent = IntentConfigActor::new().start();
    server_intent.register_with_router(&server_router);
    let server_session_mgr = SessionManager::new(server_router.recipient()).start();
    server_session_mgr.do_send(NewConnection { conn: server_conn });

    // Set up client stack
    let client_router = RoomRouter::new().start();
    let client_intent = IntentConfigActor::new().start();
    client_intent.register_with_router(&client_router);
    let client_session_mgr = SessionManager::new(client_router.recipient()).start();
    client_session_mgr.do_send(NewConnection { conn: client_conn });

    // Wait for handshake to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send a message from client to server
    client_intent.do_send(SendConfigUpdate { config }).await.unwrap();

    // Wait for delivery
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify server received the message
    let server_config = server_intent.send(GetCurrentConfig).await.unwrap();
    assert_eq!(server_config, config);
}
```

**Mock Transport Implementation**:
```rust
pub mod mock_transport {
    pub fn create_pipe() -> (MockConnection, MockConnection) {
        let (tx_a, rx_a) = mpsc::channel(10);
        let (tx_b, rx_b) = mpsc::channel(10);

        let conn_a = MockConnection { tx: tx_a, rx: rx_b };
        let conn_b = MockConnection { tx: tx_b, rx: rx_a };

        (conn_a, conn_b)
    }

    pub struct MockConnection {
        tx: mpsc::Sender<Vec<u8>>,
        rx: mpsc::Receiver<Vec<u8>>,
    }

    impl TransportConnection for MockConnection {
        async fn send(&mut self, data: Vec<u8>) -> Result<()> {
            self.tx.send(data).await?;
            Ok(())
        }

        async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
            Ok(self.rx.recv().await)
        }
    }
}
```

**Advantages**:
- Tests the full protocol flow
- No network dependencies
- Fast and deterministic
- Can simulate connection failures (close the pipe)

### Level 3: Component Tests (Business Logic Only)

**What**: Test application component logic without involving the network layer at all.

**How**: Components expose a message-based API. Tests send messages directly.

```rust
#[actix::test]
async fn test_intent_config_merge_logic() {
    let actor = IntentConfigActor::new().start();

    // Test the component's business logic
    actor.do_send(LocalUpdate { config: config_a }).await.unwrap();
    actor.do_send(LocalUpdate { config: config_b }).await.unwrap();

    let merged = actor.send(GetMergedConfig).await.unwrap();

    assert_eq!(merged, expected_merge);
}
```

**Advantages**:
- Tests only business logic
- No network or protocol concerns
- Fastest possible tests

### Level 4: End-to-End Tests (Real TCP)

**What**: Run the full system with real TCP sockets (on localhost).

**How**: Standard integration test with actual processes.

```rust
#[tokio::test]
async fn test_real_tcp_connection() {
    // Start server in background
    let server = tokio::spawn(async {
        run_server("127.0.0.1:9999").await
    });

    // Give server time to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start client
    let client = tokio::spawn(async {
        run_client("127.0.0.1:9999").await
    });

    // Wait for both
    let (server_result, client_result) = tokio::join!(server, client);

    assert!(server_result.is_ok());
    assert!(client_result.is_ok());
}
```

**Advantages**:
- Tests the complete, real system
- Catches platform-specific issues
- Validates TLS configuration, etc.

**Disadvantages**:
- Slow
- Flaky (port conflicts, timing issues)
- Should be minimal—rely on lower-level tests for most coverage

---

## Design Decisions and Rationale

This section documents the key design decisions made, the alternatives considered, and the trade-offs accepted. Future developers can use this to understand the "why" behind the design.

### Decision 1: Per-Connection Session Actors

**Decision**: Spawn a dedicated session actor for each active connection within each application component.

**Alternatives Considered**:
- **Single main actor with `HashMap<ConnectionId, State>`**: Requires locking or complex message patterns. Hard to ensure proper cleanup.
- **Connection pooling pattern**: Reuse session actors across connections. Complex lifecycle management, risk of state leakage between connections.

**Why This Decision**:
- Actor lifetime = connection lifetime (automatic cleanup)
- No shared state between connections
- Simple, easy to reason about
- Leverages Actix's lightweight actor model

**Trade-offs Accepted**:
- ✅ Clean lifecycle and state isolation
- ✅ Simple mental model
- ⚠️ One actor per connection (but actors are cheap)
- ⚠️ Slightly more verbose (main actor + session actor for each component)

### Decision 2: Registration-Based Wiring

**Decision**: Use a central `RoomRouter` with explicit registration instead of dependency injection during construction.

**Alternatives Considered**:
- **Constructor injection**: Pass `Addr<OtherActor>` to constructors. Causes chicken-and-egg problems.
- **Global registry (static)**: Avoids chicken-and-egg but introduces global mutable state and initialization order issues.
- **Pub/Sub with topics**: All actors publish/subscribe to topic strings. Loose coupling but harder to validate at compile time.

**Why This Decision**:
- Avoids circular dependencies
- Explicit and auditable (can see all registrations in boot code)
- Easy to validate (error if unregistered room is used)
- Supports dynamic registration if needed (though not required by R8)

**Trade-offs Accepted**:
- ✅ No chicken-and-egg problems
- ✅ Clear boot sequence
- ⚠️ One extra step at boot (registration)
- ⚠️ Registration happens at runtime (not compile-time validated)

### Decision 3: Two-Stage Serialization

**Decision**: Application messages are serialized, then wrapped in a protocol frame and serialized again.

**Alternatives Considered**:
- **Single-stage with dynamic dispatch**: Use `Box<dyn Any>` or similar. Loses type safety, makes testing harder.
- **Code generation**: Generate protocol-aware serialization code for each component. Complex build process, tight coupling.
- **No framing layer**: Send raw application messages. No way to multiplex multiple rooms over one connection.

**Why This Decision**:
- Maintains type safety at every layer
- Clean separation of concerns (protocol layer doesn't know about application types)
- Allows different serialization formats (protocol uses bincode, application could use JSON, etc.)

**Trade-offs Accepted**:
- ✅ Type safety
- ✅ Layer separation
- ✅ Transport agnostic
- ⚠️ Slight overhead from double serialization (acceptable in our domain)
- ⚠️ Allows format inconsistency (could serialize protocol with bincode, app with JSON—confusing but not harmful)

### Decision 4: No Reconnection Abstraction

**Decision**: Treat each TCP connection as a completely new, independent session. Do not attempt to provide reconnection logic at the protocol layer.

**Alternatives Considered**:
- **Transparent reconnection**: Automatically reconnect and buffer messages. Hides important lifecycle events from application components.
- **Session persistence**: Maintain session ID across connections, restore state. Complex, and in our domain, disconnection usually means process restart (state is gone anyway).

**Why This Decision**:
- In our deployment model, disconnection almost always indicates process restart
- Attempting to restore state after restart is dangerous (remote state is gone)
- Application components need to know about connection lifecycle for correct protocol implementation
- Simpler, more explicit

**Trade-offs Accepted**:
- ✅ Explicit lifecycle
- ✅ Correct handling of process restarts
- ✅ Simpler protocol layer
- ⚠️ Application components must handle connection lifecycle (but R4 requires this anyway)
- ⚠️ No automatic retry or buffering at protocol layer (application decides policy)

### Decision 5: Static Room List

**Decision**: Require the set of supported rooms to be known at boot time. Do not support dynamic addition/removal of rooms at runtime.

**Alternatives Considered**:
- **Dynamic rooms**: Allow components to register rooms at any time. Complex, must handle race conditions (what if message arrives for a room before it's registered?).

**Why This Decision**:
- Simplifies the design significantly
- Matches our actual use case (services know their component types at compile time)
- Allows validation at startup (error if client and server have no rooms in common)
- Reduces edge cases

**Trade-offs Accepted**:
- ✅ Simpler design
- ✅ Startup validation
- ⚠️ Can't add rooms dynamically (not needed in our use case)

### Decision 6: Separate Routing Layer

**Decision**: Create a dedicated `RoomRouter` component instead of having the session layer directly dispatch to application components.

**Alternatives Considered**:
- **Direct dispatch**: SessionActor holds `HashMap<String, Recipient<...>>`. Simpler but tightly couples session layer to application layer.
- **No router**: Application components subscribe directly to session manager. Complex subscription logic, harder to test.

**Why This Decision**:
- Clean separation of concerns (session layer only knows about protocol, not about applications)
- Centralized routing logic (easy to add filtering, logging, metrics)
- Easier to test (can test router in isolation)
- Supports multiple routing strategies (e.g., broadcast, round-robin) without touching session layer

**Trade-offs Accepted**:
- ✅ Clean layer separation
- ✅ Testable in isolation
- ✅ Extensible
- ⚠️ One extra actor in the message path (but overhead is negligible)

---

## Migration Path

The current `zznet-connection` crate has issues but also contains working protocol logic and tests. We will not delete it immediately. Instead:

### Phase 1: Build New Design in Parallel

1. Create `zznet-session` crate (fresh start)
2. Create `zznet-router` crate (fresh start)
3. Keep `zznet-connection` as-is (for reference and comparison)

**Advantages**:
- No risk of breaking existing code
- Can compare designs side-by-side
- Can copy-paste protocol logic from old to new (with modifications)

### Phase 2: Port One Component

1. Choose a simple component (e.g., `intent-config`)
2. Implement it using the new architecture
3. Write tests (unit, integration with mock transport)
4. Compare with old implementation

**Goal**: Validate that the new architecture works in practice.

### Phase 3: Reach Parity

1. Port all components to new architecture
2. Ensure all tests pass
3. Run end-to-end tests with real TCP

**At this point**: New architecture is feature-complete.

### Phase 4: Delete Old Code

1. Remove `zznet-connection` crate
2. Update documentation
3. Celebrate 🎉

---

## Error Handling and Supervision Strategy

A critical aspect of any actor-based system is how it handles failures. This section defines the error handling and supervision strategy for the network layer.

### Failure Categories

**1. Transport Failures (Connection Loss)**
- **Cause**: Network issues, remote process crash, TCP timeout
- **Handling**: Explicit and expected. SessionActor (protocol layer) detects `recv() = None`, publishes `SessionTerminated` event, and stops
- **Result**: Clean cascade - application session actors receive termination event and stop gracefully

**2. Protocol Errors (Malformed Frames, Handshake Failure)**
- **Cause**: Protocol violation, version mismatch, corrupted data
- **Handling**: SessionActor (protocol layer) logs error and stops, triggering the same cascade as transport failure
- **Result**: Connection is terminated, remote peer will see a clean disconnect

**3. Application Logic Errors (Panic in Session Actor)**
- **Cause**: Bug in application code (e.g., deserialization failure, logic panic)
- **Handling**: **This is the critical case.** If an `IntentConfigSessionActor` panics:
  - The actor stops immediately
  - **The entire vertical slice must tear down**: protocol session actor + transport connection
  - Rationale: A panic likely indicates corrupted per-connection state. Continuing the connection could lead to undefined behavior or data corruption

**4. Application Logic Errors (Panic in Main Actor)**
- **Cause**: Severe bug in global component logic
- **Handling**: **The entire service should crash**
- **Rationale**: The main actor holds global state. If it panics, the service is in an undefined state. Better to crash and restart (via process supervisor) than continue in an inconsistent state

### Supervision Strategy

**Actix Default Behavior**: When an actor panics, Actix stops the actor and drops all its addresses. Any messages sent to a stopped actor are silently dropped or return errors (depending on send method).

**Our Strategy**:

#### For Application Session Actors (e.g., `IntentConfigSessionActor`)
- **Supervision**: No restart. Let the actor die.
- **Cascading Teardown**: When the application session actor stops (whether cleanly or via panic), it must trigger teardown of the protocol session actor
- **Implementation**:
  ```rust
  impl Actor for IntentConfigSessionActor {
      type Context = Context<Self>;

      fn stopped(&mut self, _ctx: &mut Context<Self>) {
          // Explicitly close the session handle when this actor stops
          // This signals to the protocol layer to tear down the connection
          self.session_handle.close();

          // Notify main actor about abnormal termination if needed
          if self.is_panic {
              self.main_actor.do_send(SessionFailed {
                  connection_id: self.connection_id,
                  error: "Session actor panicked".to_string(),
              });
          }
      }
  }
  ```

#### For Protocol Session Actors (e.g., `ZzNetSessionActor`)
- **Supervision**: No restart. Let the actor die.
- **Cascading Teardown**: When stopped, close the transport connection
- **Implementation**:
  ```rust
  impl Actor for SessionActor {
      type Context = Context<Self>;

      fn stopped(&mut self, _ctx: &mut Context<Self>) {
          // Close transport connection
          self.transport.close();

          // Publish termination event (if not already published)
          if !self.termination_published {
              self.router.do_send(SessionEvent::Terminated {
                  connection_id: self.connection_id,
              });
          }
      }
  }
  ```

#### For Main Actors (e.g., `IntentConfigActor`)
- **Supervision**: No restart. Let the service crash.
- **Rationale**: Main actors hold global state. A panic indicates a severe bug that cannot be recovered from
- **Implementation**: No special handling needed. Let Actix stop the actor, which will cause the service to exit

#### For Singleton Infrastructure Actors (`SessionManager`, `RoomRouter`)
- **Supervision**: No restart. Let the service crash.
- **Rationale**: These are critical infrastructure. If they fail, the entire network layer is non-functional
- **Implementation**: No special handling. Service should exit and be restarted by a process supervisor (systemd, Docker, etc.)

### Error Propagation

**Vertical Slice Teardown** (Application → Protocol → Transport):
1. Application session actor panics or encounters fatal error
2. In `stopped()`, it closes `SessionHandle`
3. Protocol session actor detects closed handle and stops
4. In `stopped()`, protocol actor closes transport connection
5. Transport closure triggers connection termination cleanup

**Horizontal Event Propagation** (Protocol → Router → Application):
1. Protocol session actor stops (for any reason)
2. Before stopping, it publishes `SessionTerminated` event
3. Router forwards event to all registered handlers
4. Application main actors receive event and clean up tracking state

### Key Principle: Fail Fast, Fail Loud

**Do NOT**:
- Swallow errors silently
- Attempt to "recover" from panics (can't be done safely)
- Continue processing on a connection where an actor has panicked

**DO**:
- Log all errors with full context
- Tear down the entire connection on any actor panic
- Let the service crash on main actor or infrastructure actor failures
- Rely on a process supervisor for service-level restarts

### Testing Error Paths

Error handling must be tested:

```rust
#[actix::test]
async fn test_session_actor_panic_tears_down_connection() {
    // Set up a full connection
    let (client, server) = create_test_connection().await;

    // Inject a message that will cause the session actor to panic
    client.send_malformed_data().await;

    // Verify the entire connection is torn down
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(server.connection_is_closed());
    assert!(client.received_termination_event());
}
```

---

## Open Questions and Future Work

These are questions that do not need to be answered now but may become relevant as the system evolves.

### Question 1: Connection Pooling

**Context**: Currently, each connection spawns independent session actors. If a client needs to maintain multiple connections to the same server (e.g., for redundancy), how should this work?

**Options**:
- Application-level pooling (main actor tracks multiple connections)
- Protocol-level pooling (session manager provides "best available connection")
- No change needed (current design handles this naturally)

**Decision**: Deferred. Current design handles multiple connections naturally (each gets its own session actor). Application components can implement pooling logic if needed.

### Question 2: Authentication and Authorization

**Context**: The protocol has an `auth_role` field in the handshake, but authorization logic is not specified.

**Options**:
- Protocol layer checks role and refuses to activate certain rooms
- Application layer checks role and refuses to handle certain messages
- Hybrid (protocol filters, application authorizes)

**Decision**: Deferred. Application components should handle authorization (they know the business rules). Protocol layer can optionally filter rooms based on role if needed.

### Question 3: Protocol Versioning and Backwards Compatibility

**Context**: The current design uses a simple `version: "1.0"` string in the `Hello` frame. Currently, version strings must match exactly or the connection is rejected. As the protocol evolves, we may need to support backwards compatibility to enable non-disruptive upgrades.

**Key Questions to Explore Later**:

1. **Upgrade Strategy**:
   - In our deployment model, do we upgrade all services atomically (entire system goes down, upgrades, comes back up)?
   - Or do we need rolling upgrades where old and new versions coexist temporarily?
   - What's the typical upgrade window and acceptable downtime?

2. **Compatibility Surface**:
   - What changes to the protocol are "compatible" vs "breaking"?
   - Adding new room types: compatible or breaking?
   - Adding new fields to existing frames: compatible or breaking?
   - Changing serialization format: always breaking?

3. **Version Negotiation Mechanisms** (if needed):
   - Simple rejection: "versions must match exactly" (current approach)
   - Negotiation: "I support versions X, Y, Z; pick one we both support"
   - Feature flags: "I support features A, B, C; use the intersection"
   - Backwards compatibility mode: "new server can speak old protocol"

4. **Discovery and Diagnostics**:
   - When a version mismatch occurs, how does an operator discover it?
   - What information is logged? (both versions, who rejected whom, why)
   - Can we detect "mixed version" states in a cluster?

5. **Migration Path**:
   - If we later need to add versioning logic, can it be done without breaking existing deployments?
   - Can we evolve from "exact match" to "negotiated match" gracefully?

**Architectural Consideration**: The current design supports adding versioning logic later without fundamental changes:
- The `Hello` frame already has a version field
- The handshake phase is where version checking happens
- We can evolve the version field from a simple string to a structured format (e.g., `{ major: 1, minor: 2, features: [...] }`) without changing the architecture
- Compatibility logic would live in the `SessionActor` (protocol layer), not in application components

**Current Decision**: Use exact version matching ("1.0" == "1.0" or reject). This is simple, forces clarity in deployments, and doesn't preclude adding negotiation later when we have actual compatibility requirements.

### Question 4: Backpressure and Flow Control

**Context**: If a component produces data faster than the network can send it, what happens?

**Options**:
- Bounded channels (back pressure to producer)
- Unbounded channels (risk of OOM)
- Explicit flow control protocol (complex)

**Decision**: Use bounded channels for session actors. If channel is full, sender is automatically back-pressured. This is simple and effective.

### Question 5: Metrics and Observability

**Context**: How do we expose metrics (e.g., active connections, messages sent/received, errors)?

**Options**:
- Each actor publishes metrics
- Centralized metrics collector
- Logging only

**Decision**: Deferred. Can add later without changing architecture. Actors can publish metrics messages to a metrics collector actor.

---

## Conclusion

This document defines a clean, layered architecture for the ZZPing network layer based on the actor model. The key insights are:

1. **Per-connection session actors** eliminate shared state and provide explicit lifecycle management
2. **Clear layer separation** (transport, session, routing, application) makes the system understandable and testable
3. **Registration-based wiring** avoids circular dependencies
4. **Connection-aware design** makes stateful protocols straightforward to implement
5. **No hidden magic** (no transparent reconnection, no heavy buffering) keeps the system explicit and debugable

The design satisfies all core requirements (R1-R10) and provides a solid foundation for building reliable, testable, distributed components.

**Next Steps**:
1. Review this document for correctness and completeness
2. Begin implementation of `zznet-session` crate
3. Implement one component (e.g., `intent-config`) to validate the design
4. Iterate based on learnings

---

**Document End**
