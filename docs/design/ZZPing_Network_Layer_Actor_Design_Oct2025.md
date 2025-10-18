# ZZPing Network Layer Architecture: Actor-Based Design

**Date**: October 1, 2025 (Updated: October 2, 2025)
**Status**: Active Design - Aligned with Vision Document
**Authors**: David Martínez Martí, AI Design Partner (Claude 4.5 Sonnet)

**Reference**: This document has been aligned with `ZZPing_Network_Layer_Vision.md`, which serves as the authoritative reference for essential architectural principles.

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

**Core Insight**: The **SessionManager** is the heart of the network layer—a completely transport-agnostic component that manages all peer connections and routes typed messages between components. Components communicate using only typed messages, completely independent of how those messages are transported.

**Key Architectural Layers**:
1. **Transport Layer**: Abstract, pluggable transport (TCP/TLS, mock, gRPC, etc.) - handles bytes only
2. **HELLO Handler**: Peer identity and transport-level handshake (operates on bytes)
3. **SessionManager**: Transport-agnostic core managing PeerSessions (operates on typed messages only)
4. **Room Router**: Routes typed messages to registered component handlers
5. **Application Components**: Business logic with same code on both sides (e.g., MemDB ↔ MemDB)

**Two Distinct Protocols**:
- **Protocol A (HELLO)**: Peer identity exchange using bytes - handled before SessionManager involvement
- **Protocol B (Room Communication)**: Typed message exchange between components - managed entirely by SessionManager

**What This Enables**:
- SessionManager completely testable without any network I/O (mock-first testing)
- Pure actor model with no shared state or locking
- Self-testable components with minimal dependencies
- Transport-agnostic design (swap TCP for mock without touching SessionManager)
- Symmetric protocol (same component code runs on both sides with different config)

---

## Core Requirements

These are the fundamental, non-negotiable requirements. They describe **what** the system must do, not **how** it does it. Design decisions that follow must satisfy all of these requirements.

### R1: Pure Actor Model for Component Isolation
Components must be implemented as isolated actors. Communication between components occurs exclusively through message passing. No shared mutable state. No `Arc<Mutex<T>>` patterns (which are code smells indicating architectural problems).

**Rationale**: Enables independent testing, eliminates race conditions, and provides clear concurrency semantics.

---

## System Context and Operational Constraints

Before diving into the architecture, it's critical to understand the operational context in which this network layer operates. These constraints justify many architectural decisions and explain why "fail fast, fail loud" is an appropriate strategy.

### Service-Level Resilience Design

**Network Partition Tolerance**: All zzping services are designed to survive network isolation for **at least 1 hour** with graceful degradation:
- **Collectors** continue pinging targets and buffer results in local memory (memdb component)
- **Database** continues serving queries with existing data
- **GUI/CLI** freeze and work with cached local data

**Fail-Static Behavior**: Services maintain their last-known-good state during network outages rather than failing open or closed unpredictably.

**Operational Independence**: Each service is designed to not be a strict dependency of any other service. A service failure should not cascade to dependent services beyond the expected loss of that service's functionality.

**Automatic Recovery**: Services are expected to be managed by process supervisors (systemd, Docker, Kubernetes) that automatically restart them on failure. Restart is fast and clean, with minimal operational impact.

**Architectural Consequence**: This system-level resilience means the network layer can afford to "fail fast, fail loud" at the connection level. If a connection encounters an unrecoverable error, tearing it down is acceptable because:
1. The remote service is designed to handle connection loss
2. Reconnection will happen automatically
3. Service-level state is preserved independently of connection state

### Component Lifecycle Model

**Static Wiring**: Components are wired together at boot time, and this wiring never changes during the service lifetime. There is no dynamic component discovery, no runtime registration changes, no readiness probes, no health checks beyond basic "is the actor alive?"

**No Startup Sequencing**: Components do not have complex startup dependencies. Each component is self-sufficient enough to start in any order. If a component needs data from another component, it naturally waits (via message passing) until that data arrives.

**Pre-Online State**: As a consequence of the boot sequence (`Actors → Router → SessionManager → Transport`), the architecture explicitly supports a "pre-online" state where:
- All internal actors are running and communicating
- All room handlers are registered and ready
- The transport layer is NOT yet attached (no network connections accepted/initiated)

This pre-online state is operationally valuable for controlled rollouts, maintenance windows, or testing scenarios where you want the service logic running but network-isolated.

**Single Point of Activation**: Attaching the transport layer is the final step that makes the service "live" on the network. This is a clear, auditable boundary between "service is prepared" and "service is online."

### Transport Layer Resilience Requirement

**Exception to "Fail Fast"**: While most components can panic on unexpected errors (triggering service restart), the **transport layer must be resilient** because it performs real I/O operations with external systems.

**Required Error Handling**: The transport layer must use `Result<T, E>` for all I/O operations and handle errors gracefully:
- TCP connection failures → log and retry or report to session layer
- Socket read/write errors → close connection cleanly, notify session layer
- TLS handshake failures → log and reject connection

**Rationale**: The transport layer is the boundary between our controlled actor environment and the chaotic external network. It must absorb and translate external failures into clean internal events (e.g., `TransportTerminated`), not panic.

**Contrast with Application Layer**: Application business logic is expected to use `Result<T, E>` for expected errors (e.g., validation failures), but unexpected errors (logic bugs) should panic. The transport layer has no "unexpected errors" when it comes to I/O—all I/O failures are expected and must be handled.

### Failure Isolation Philosophy

**Component Failure = Process Failure**: If a main application actor (e.g., `IntentConfigActor`) panics due to a logic bug, the entire service should crash and restart. There is no partial recovery.

**Connection Failure = Local Failure**: If a per-connection session actor panics, only that connection's vertical slice should tear down (session actor + protocol actor + transport connection). Other connections continue normally.

**Why This Works**: The system-level design (automatic restarts, 1-hour partition tolerance, fail-static behavior) makes service restarts cheap and safe. It's better to crash and restart in a clean state than to continue in a corrupted state.

### Invariants Under This Model

Given the operational context above, the following invariants are critical:

1. **"Fail Atomically"**: If any part of a connection's vertical slice fails (application session actor, protocol session actor, transport), the entire slice must be torn down atomically. Partial failures must not leave dangling state.

2. **"Static Wiring"**: Room registrations never change after boot. The set of rooms a service can handle is fixed for the lifetime of the process.

3. **"No Orphan Messages"**: A room message must never be sent to a transport connection without a corresponding registered application subscriber. This is enforced by static registration and boot-time validation.

4. **"Session-to-Connection 1:1"**: There is exactly one protocol session actor per active transport connection. No sharing, no multiplexing at the session level.

5. **"Transport Errors Are Expected"**: The transport layer must never panic due to I/O errors. All I/O operations must be wrapped in `Result<T, E>` and handled explicitly.

---

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

## Architectural Guarantees (Not Conventions)

The following are not best practices or recommendations—they are **guarantees** enforced by the architecture itself, making certain classes of bugs impossible.

### Guarantee 1: SessionHandle Enforces "Fail Atomically"

**The Invariant**: If any part of a connection's vertical slice fails (application session actor, protocol session actor, or transport), the entire slice must be torn down atomically. Partial failures must not leave dangling state.

**How It's Enforced**: The `SessionHandle` given to application session actors uses Rust's **RAII (Resource Acquisition Is Initialization)** pattern via the `Drop` trait:

```rust
pub struct SessionHandle {
    inner: Option<SessionHandleInner>,
}

impl Drop for SessionHandleInner {
    fn drop(&mut self) {
        // Automatically tear down connection when handle is dropped
        self.protocol_actor.do_send(TeardownConnection {
            connection_id: self.connection_id,
        });
    }
}
```

**What This Means**:
- When an application session actor stops (clean shutdown or panic), its `SessionHandle` is automatically dropped
- Dropping the handle triggers `TeardownConnection` to the protocol actor
- The protocol actor stops and closes the transport
- The entire vertical slice is torn down atomically

**Why This Is Better Than Convention**:
- Developers cannot forget to tear down the connection—Rust's type system guarantees it
- Works correctly even during panics (Drop always runs, even during unwinding)
- No special error handling code needed in application actors
- The invariant is enforced at compile time, not runtime

### Guarantee 2: Peer Context Is Always Available

**The Invariant**: Application components always know WHO they're connected to (peer hostname, role, protocol version).

**How It's Enforced**: The `SessionEvent::Active` variant includes peer context from the `Hello` frame:

```rust
pub enum SessionEvent {
    Active {
        connection_id: u64,
        rooms: Vec<String>,
        handle: SessionHandle,

        // Guaranteed to be present - came from Hello frame
        peer_hostname: String,
        peer_role: AuthRole,
        peer_version: String,
    },
    // ...
}
```

**What This Enables**:
- Authorization: "Only accept connections from Database role"
- Logging: "Collector 'collector-01' connected"
- Application logic: "Behave differently when talking to CLI vs Database"
- Debugging: Operators can see WHO is connected, not just connection IDs

**Why This Matters**: Without peer context, applications would need to implement their own ad-hoc "who are you?" protocol at the start of every session. This design makes peer identity a first-class citizen of the architecture.

### Guarantee 3: Message Ordering Within a Room

**The Invariant**: Messages sent to the same room on the same connection are delivered in FIFO (First-In-First-Out) order.

**How It's Enforced**: The entire pipeline naturally preserves FIFO ordering:
1. Application session actor sends messages through `SessionHandle` in order
2. Protocol session actor receives them in its mailbox (Actix mailboxes are FIFO)
3. Protocol actor serializes them onto the TCP stream in order
4. TCP guarantees FIFO byte delivery
5. Peer receives and processes frames in order

**What This Enables**:
- Stateful protocols can send sequences of updates (`Create`, `Update`, `Delete`) and know they'll be processed in order
- No need for explicit sequence numbers within a room
- Simplifies application logic dramatically

**What Is NOT Guaranteed**:
- ❌ **No ordering between different rooms** on the same connection (they come from independent actors)
- ❌ **No ordering across different connections** (network latency is unpredictable)

### Guarantee 4: SessionActive Arrives Before Data

**The Invariant**: For any connection, the `SessionActive` event is fully processed by application components before any `DataForRoom` events arrive for that connection.

**How It's Enforced**: The protocol session actor processes events sequentially:
1. Completes handshake and room negotiation
2. Publishes `SessionActive` events to all relevant rooms
3. Only then continues reading and dispatching data frames from the transport

Because the protocol actor is single-threaded (actor model) and processes its mailbox sequentially, and because Actix message delivery is fast (microseconds), the `SessionActive` handlers in main actors will spawn session actors before the first data frame is read from the TCP buffer.

**What This Prevents**:
- Race conditions where data arrives before the session actor exists
- Orphan data that has no handler
- Complex buffering logic in main actors

**Why This Works**: The actor model's sequential processing, combined with TCP's receive buffering, creates a natural synchronization point. The protocol actor cannot read the next frame until it has dispatched the previous event.

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

**Critical Principle**: SessionManager NEVER touches bytes. It operates entirely on typed messages.

**The Serialization Layers**:

1. **Component Layer** (typed → typed): Component sends typed message to SessionManager
   ```rust
   // Component code
   let msg = MemDBMessage::Insert { key, value };
   session_manager.send_to_peer(peer_id, "memdb", msg);  // Still typed!
   ```

2. **SessionManager Layer** (typed → typed): Routes typed message to correct PeerSession
   ```rust
   // SessionManager code - NO serialization, only routing
   impl SessionManager {
       fn send_to_peer<T: Serialize>(&self, peer_id: PeerId, room: &str, message: T) {
           let peer_session = self.peer_sessions.get(&peer_id)?;
           peer_session.send_room_message(room, message);  // Still typed!
       }
   }
   ```

3. **PeerSession Layer** (typed → bytes): Serializes message, wraps in envelope, sends to transport
   ```rust
   // PeerSession code - THIS is where serialization happens
   impl PeerSession {
       fn send_room_message<T: Serialize>(&self, room: &str, message: T) {
           // Stage 1: Serialize component message
           let message_bytes = bincode::serialize(&message)?;

           // Stage 2: Wrap in SessionMessage envelope
           let envelope = SessionMessage::RoomMessage {
               room: room.to_string(),
               data: message_bytes,
           };

           // Stage 3: Serialize envelope and send via transport
           let envelope_bytes = bincode::serialize(&envelope)?;
           self.transport_handle.send_bytes(envelope_bytes)?;
       }
   }
   ```

**Why This Layering?**
- **SessionManager is transport-agnostic**: It never sees bytes, only typed messages
- **PeerSession is the serialization boundary**: It converts between typed (above) and bytes (below)
- **Testing without network**: SessionManager can be tested with mock TransportHandles
- **Type safety**: Compiler catches type mismatches between components

**The Key Insight**:
- SessionManager doesn't know about serialization, envelopes, or bytes
- PeerSession encapsulates all byte-handling
- This is what makes SessionManager transport-agnostic and testable without network I/O

**Trade-offs**:
- ✅ SessionManager completely testable without network
- ✅ Transport truly pluggable (TCP, mock, gRPC)
- ✅ Clear layer boundaries (typed vs bytes)
- ⚠️ Multiple serialization stages (component message → envelope → wire)
- ⚠️ PeerSession must be generic over message types

**Decision**: We accept this trade-off. The ability to test SessionManager without network I/O is worth the complexity.

---

## Protocol Structure: Two Distinct Protocols

The zznet layer uses **two separate protocols** operating at different layers with different concerns. Understanding this separation is critical for comprehending the architecture.

### Protocol A: HELLO (Peer Identity & Transport Handshake)

This protocol establishes peer identity and validates the transport connection **before** the SessionManager gets involved.

**Layer**: Operates at the transport layer, **below** SessionManager

**Data Format**: Bytes (serialized frames)

**Purpose**:
- Establish basic peer identity (hostname, role)
- Verify protocol compatibility
- Complete transport-level handshake (may include TLS, authentication, etc.)

**Frame Structure** (simplified):
```rust
#[derive(Serialize, Deserialize)]
struct HelloFrame {
    protocol_family: String,        // "zznet"
    protocol_version: String,        // "1.0"
    hostname: String,
    role: AuthRole,                  // Collector, Database, CLI, etc.
}

#[derive(Serialize, Deserialize)]
struct HelloAckFrame {
    accepted: bool,
    hostname: String,
    role: AuthRole,
}
```

**Behavior**:
1. Transport connection established (TCP, TLS, etc.)
2. HELLO Handler sends `HelloFrame` with identity
3. Remote peer receives `HelloFrame`, validates it
4. Remote peer sends `HelloAckFrame` (accept/reject)
5. **If accepted**: HELLO Handler creates a PeerIdentity and hands off to SessionManager
6. **If rejected**: Connection closed with error

**Critical Points**:
- HELLO operates on **bytes** (serialized frames)
- HELLO knows nothing about rooms or typed messages
- HELLO completes **before** SessionManager creates a PeerSession
- SessionManager never sees HELLO messages - it receives only the validated PeerIdentity result

### Protocol B: Room Communication (Typed Messages Between Components)

After HELLO completes and SessionManager creates a PeerSession, components communicate using **typed messages**.

**Layer**: Operates at the SessionManager layer and above, **completely transport-agnostic**

**Data Format**: Typed Rust messages (e.g., `MemDBMessage`, `IntentConfigMessage`)

**Purpose**:
- Negotiate which rooms (communication channels) are active
- Exchange typed messages between component pairs
- Handle component-level lifecycle (room activation, message exchange)

**Room Negotiation**:
```rust
// Each side publishes their supported rooms
let local_rooms = vec!["memdb", "intent-config"];
let peer_rooms = vec!["memdb", "health"];

// SessionManager computes intersection
let active_rooms = local_rooms.intersection(&peer_rooms); // ["memdb"]

// Auto-join: SessionManager creates Room for each active room
// No dynamic join/leave - rooms are determined at connection establishment
```

**Message Exchange** (SessionManager perspective):
```rust
// Component sends typed message
component.send_to_peer(
    peer_id,
    "memdb",
    MemDBMessage::Query { id: 123 }
);

// SessionManager routes to correct PeerSession
// PeerSession serializes and sends to transport
// Remote transport receives bytes
// Remote PeerSession deserializes to MemDBMessage
// Remote SessionManager routes to MemDB component
```

**Critical Points**:
- SessionManager operates **only on typed messages** - never sees bytes
- Room negotiation happens via auto-join intersection (no dynamic subscribe/unsubscribe)
- Same component code on both sides (e.g., MemDB(Collector) ↔ MemDB(Database))
- Rooms are point-to-point, 1:1 channels (not broadcast)
- Each room is a "phone line" between two component instances

### Why Two Protocols?

**Complete Layer Separation**:
- **Protocol A (HELLO)**: Transport layer concern - operates on bytes, handles peer identity
- **Protocol B (Room Communication)**: Application layer concern - operates on typed messages, handles component communication
- SessionManager sits **above** Protocol A and manages Protocol B

**Transport Agnostic Testing**:
- SessionManager can be tested with mock transport (in-memory channels)
- Two SessionManagers can communicate via mock without any network I/O
- Protocol A (HELLO) can be tested independently of SessionManager
- This is the **design validation**: if you can't test SessionManager without network I/O, the architecture is wrong

**Evolution Without Breaking Changes**:
- HELLO protocol (Protocol A) changes rarely - provides stable foundation
- Room message types (Protocol B) can evolve independently
- SessionManager never changes when transport changes (TCP → mock → gRPC)

**Critical Architectural Invariant**:
SessionManager must compile and function **without** any transport crate dependency. It receives PeerIdentity (from HELLO Handler) and sends/receives typed messages. It never sees bytes.

### Implementation Architecture

The two protocols are handled by **separate components** at different layers:

```rust
// Protocol A: HELLO Handler (transport layer, deals with bytes)
struct HelloHandler {
    transport: Box<dyn Transport>,
}

impl HelloHandler {
    fn perform_handshake(&mut self) -> Result<PeerIdentity> {
        // Send/receive HELLO frames (bytes)
        // Validate peer identity
        // Return validated PeerIdentity to SessionManager
    }
}

// Protocol B: SessionManager (transport-agnostic, deals with typed messages)
struct SessionManager {
    peers: HashMap<PeerId, PeerSession>,
    router: RoomRouter,
}

impl SessionManager {
    fn create_peer_session(&mut self, identity: PeerIdentity, transport_handle: TransportHandle) {
        // Negotiate rooms via intersection
        // Create PeerSession
        // Never sees bytes - only typed messages
    }
}
```

**Critical Separation**: HELLO Handler completes **before** SessionManager creates a PeerSession. SessionManager never participates in HELLO - it only receives the validated result.

### Room Name Registry (Implicit Contract)

While room names are strings at the protocol level, they represent **well-known protocol identifiers** in the zzping ecosystem:

| Room Name       | Purpose                          | Message Protocol          |
|-----------------|----------------------------------|---------------------------|
| `intent-config` | Intent configuration sync        | `IntentConfigMessage`     |
| `health`        | Health check / heartbeat         | `HealthMessage`           |
| `metrics`       | Metrics collection               | `MetricsMessage`          |
| `alerts`        | Alert notifications              | `AlertMessage`            |
| `ping-results`  | Ping result streaming            | `PingResultMessage`       |

**These are not arbitrary strings.** They are the equivalent of API endpoints or RPC method names in a distributed system. If a component offers "intent-config", it **must** implement the `IntentConfigMessage` protocol correctly.

**Configuration errors** (e.g., a Database expecting "metrics" but Collector only offers "ping-results") will be caught during room negotiation, resulting in an empty intersection and connection rejection.

---

## Connection Model and Operational Constraints

This section defines critical operational characteristics of the network layer that affect implementation and behavior.

### Connection Lifecycle

**Model**: Persistent, long-lived connections. No connection pooling or reuse.

**Rationale**: Each connection represents an active relationship between two services. When a service restarts, it establishes a new connection. The overhead of TCP handshake + TLS + protocol negotiation is acceptable for our connection frequency (minutes to hours between reconnects, not seconds).

**Behavior**:
- Services establish connections at startup and maintain them until shutdown
- No connection pool management needed
- Each connection is independent (no state sharing between connections)

### Keepalive and Liveness Detection

**Mechanism**: Transport-level heartbeat using **zero-sized frames**.

**Frame Format**:
```rust
// Transport frame structure
// [u32 length][payload bytes]

// Heartbeat frame
[0x00, 0x00, 0x00, 0x00]  // length = 0, no payload
```

**Behavior**:
- **Both sides** must send heartbeat frames periodically (every 1 second recommended)
- If no frames received (heartbeat or data) for configured timeout (e.g., 5 seconds), assume connection is dead
- If several consecutive heartbeat send attempts fail at TCP level, close connection
- Heartbeat is **transport layer responsibility**, not visible to protocol or application layers

**Rationale**: Detect "zombie connections" (peer crashed but TCP didn't notice) quickly. TCP keepalive can take minutes; protocol-level heartbeat detects failure in seconds.

### Message Size Limits

**Hard Limit**: 16 MiB (16,777,216 bytes) per message.

**Enforcement**:
- **Sending**: Protocol layer must reject messages exceeding 16 MiB before attempting to send. Return error to application session actor.
- **Receiving**: If frame header indicates size > 16 MiB, immediately close connection with protocol error.

**Rationale**:
1. **Security**: Prevent attackers from causing memory exhaustion by claiming terabytes of data
2. **Multiplexing**: Large messages would starve other rooms on the same connection (head-of-line blocking)
3. **Predictability**: Services can allocate bounded buffers

**Configuration**: This is a compile-time constant in the transport layer, not runtime-configurable.

**Failure Mode**: Attempting to send >16 MiB is a logic error (panic-worthy). Receiving >16 MiB is a protocol violation (close connection).

### Observability

**Requirements**: The framework must provide visibility into connection state for operators.

**Minimum Required Metrics** (to be exposed):
- Number of active connections
- List of connected peers (hostname, role)
- Active rooms per connection
- Messages sent/received counters
- Errors (backpressure, protocol violations, etc.)

**Implementation Strategy** (deferred): For v1.0, logging to console is sufficient. Future versions may expose structured metrics (Prometheus, statsd, etc.).

**Logging Frequency**: Periodic summary (e.g., every 60 seconds) showing active connections and basic stats.

---

## Explicit Non-Goals

These are design decisions about what the architecture explicitly **does not** provide. Documenting non-goals prevents scope creep and clarifies boundaries.

### NG1: Connection Pooling

**Not Provided**: The framework does not implement connection pooling, connection reuse, or connection multiplexing across application requests.

**Rationale**: Our connection model is persistent, long-lived connections. Pooling is for short-lived request/response patterns (e.g., HTTP). Not needed here.

### NG2: Multiple Connections from Same Identity

**Not Provided**: The framework does not correlate or manage multiple connections claiming the same identity (e.g., two connections from "Collector-1").

**Who Handles It**: Application business logic. The framework treats each connection as independent. If an application needs "connection displacement" (close old connection when new one arrives), it must implement that logic itself using the `peer_hostname` field in `SessionActive`.

### NG3: Resource Limits and Admission Control

**Not Provided**: No maximum connection limits, no per-peer connection limits, no admission control.

**Rationale**: Not needed for our use case. Services are deployed in controlled environments with known peer counts (e.g., 10 collectors, 1 database). If needed in the future, the transport layer can implement limits.

### NG4: Graceful Shutdown

**Not Provided**: No graceful connection drain, no "goodbye" frame, no timeout for finishing in-flight requests.

**Behavior**: On shutdown (SIGINT, SIGTERM), connections are closed immediately. From the peer's perspective, the connection just drops.

**Who Handles It**: Application business logic can implement its own shutdown logic (e.g., flush buffers before stopping actors) using signal handlers, but the network layer does not coordinate this.

### NG5: Transport Switching

**Not Provided**: No dynamic transport negotiation (e.g., starting with TCP, upgrading to QUIC).

**Rationale**: YAGNI. Transport is decided before connection establishment and never changes. If transport switching is needed, build a separate transport layer implementation.

### NG6: Error Recovery at Application Layer

**Not Provided**: All errors at the application layer (deserialization failures, logic panics) are **fatal** and tear down the connection.

**Rationale**: Errors indicate bugs or incompatible protocol versions. Better to fail fast and restart than continue in an inconsistent state.

**Exception**: The transport layer handles I/O errors gracefully (using `Result<T, E>`), but these are not exposed to applications.

### NG7: Authorization Error Responses

**Current Limitation**: If an application rejects a connection based on authorization (e.g., "this role is not allowed for this room"), there is no way to send an error message back to the peer. The connection is silently closed.

**Future Consideration**: Could add a `Frame::Rejected { room: String, reason: String }` frame, but this is not a v1.0 requirement.

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

// 5. VALIDATE wiring before going live
validate_boot_config(&router, &session_mgr)?;

// 6. Attach transport (service goes "online")
let transport = TcpTransport::new(...);
session_mgr.do_send(AttachTransport { transport });
```

**No chicken-and-egg problem because actors are created first, dependencies are established second.**

### Boot Validation

Before attaching the transport, the system validates that the wiring is correct. Invalid configurations cause the service to **panic at boot** rather than fail silently at runtime.

**Validation checks**:
```rust
fn validate_boot_config(router: &RoomRouter, session_mgr: &SessionManager) -> Result<(), BootError> {
    let offered_rooms = session_mgr.offered_rooms();
    let registered_rooms = router.registered_rooms();

    // Check 1: Every offered room must have a registered handler
    for room in offered_rooms {
        if !registered_rooms.contains(room) {
            return Err(BootError::NoHandlerForRoom {
                room: room.clone(),
                hint: "Did you forget to call RegisterRoom?",
            });
        }
    }

    // Check 2: No duplicate registrations (optional, depending on policy)
    let mut seen = HashSet::new();
    for room in registered_rooms {
        if !seen.insert(room) {
            return Err(BootError::DuplicateHandler {
                room: room.clone(),
                hint: "A room should have exactly one handler",
            });
        }
    }

    Ok(())
}
```

**Failure mode**: If validation fails, the service panics with a clear error message:
```
thread 'main' panicked at 'Boot validation failed: NoHandlerForRoom {
    room: "intent-config",
    hint: "Did you forget to call RegisterRoom?"
}'
```

**Why this is good**: Misconfiguration is caught immediately at boot, not discovered hours later when a connection finally arrives. Operators get clear, actionable error messages.

---

## Layered Architecture

The network layer is organized into distinct layers with a **critical boundary** between typed messages (above SessionManager) and bytes (below SessionManager).

```
┌──────────────────────────────────────────────────────────┐
│            APPLICATION COMPONENTS                        │
│   Same component code both sides (config differs)      │
│                                                          │
│  Collector Side:              Database Side:           │
│  ┌───────────────────┐  ┌───────────────────┐         │
│  │ MemDB(Collector)  │  │ MemDB(Database)   │         │
│  │ "memdb" room      │  │ "memdb" room      │         │
│  └───────────────────┘  └───────────────────┘         │
└──────────────────────────────────────────────────────────┘
                    ↕
              (typed: MemDBMessage)
                    ↕
┌──────────────────────────────────────────────────────────┐
│                   ROOM ROUTER                          │
│   Routes typed messages by room name to components     │
│  ┌──────────────────────────────────────────┐            │
│  │ room "memdb" → MemDB component       │            │
│  │ room "intent-config" → IntentConfig   │            │
│  └──────────────────────────────────────────┘            │
└──────────────────────────────────────────────────────────┘
                    ↕
              (typed messages)
                    ↕
╱──────── TYPED │ BYTES BOUNDARY ────────╲
                    ↕
┌──────────────────────────────────────────────────────────┐
│              SESSION MANAGER (CORE)                    │
│         100% TRANSPORT AGNOSTIC                        │
│   Manages PeerSessions, negotiates rooms via          │
│   intersection, routes typed messages                 │
│                                                          │
│  ┌──────────────────────────────────────────┐            │
│  │ PeerSession for Collector-1         │            │
│  │ rooms: ["memdb", "intent-config"]    │            │
│  └──────────────────────────────────────────┘            │
│  ┌──────────────────────────────────────────┐            │
│  │ PeerSession for Collector-2         │            │
│  │ rooms: ["memdb"]                    │            │
│  └──────────────────────────────────────────┘            │
└──────────────────────────────────────────────────────────┘
                    ↕
            (TransportHandle)
           send_bytes / recv_bytes
                    ↕
┌──────────────────────────────────────────────────────────┐
│                HELLO HANDLER                         │
│   Performs Protocol A (HELLO) using bytes             │
│   Validates peer identity                             │
│   Hands validated PeerIdentity to SessionManager      │
└──────────────────────────────────────────────────────────┘
                    ↕
                 (bytes)
                    ↕
┌──────────────────────────────────────────────────────────┐
│              TRANSPORT LAYER                          │
│   TCP/TLS, Mock, gRPC - completely pluggable          │
│   Trait-based, no hardcoded implementation            │
└──────────────────────────────────────────────────────────┘
```

**Critical Architectural Invariant**: SessionManager operates **entirely above the typed/bytes boundary**. It never sees serialized bytes, never knows about the transport implementation, and can be fully tested with mock TransportHandles.

### Layer Responsibilities

#### Application Components
**What they do**:
- Implement business logic for specific protocols (e.g., MemDB, IntentConfig)
- Same code runs on both sides, configured differently
- Send/receive typed messages via SessionManager
- Handle component-specific lifecycle

**What they know about**:
- Their own message types (e.g., `MemDBMessage`)
- Which rooms they publish (e.g., `["memdb"]`)
- Their business logic

**What they do NOT know about**:
- Transport implementation (TCP vs. mock)
- Serialization format (bincode, JSON, etc.)
- Network layer existence (fully decoupled)
- Other application components

#### Room Router
**What it does**:
- Maintains registry: room name → component handler
- Routes typed messages from SessionManager to components
- Routes typed messages from components to SessionManager

**What it knows about**:
- Room name → component mappings
- Typed message routing (generic over message type)

**What it does NOT know about**:
- Message content or semantics
- Transport or serialization details

#### SessionManager (THE CORE)
**What it does**:
- Manages all PeerSessions (one per peer connection)
- Negotiates rooms via PublishRooms intersection
- Routes typed messages between components and PeerSessions
- Completely transport-agnostic

**What it knows about**:
- PeerIdentity (from HELLO Handler)
- Which rooms are active per peer
- Typed message routing

**What it does NOT know about**:
- How messages are serialized (that's PeerSession's job)
- Transport implementation (receives TransportHandle abstraction)
- HELLO protocol details (receives validated PeerIdentity)

**Critical Constraint**: SessionManager must compile without any transport crate dependency

#### HELLO Handler
**What it does**:
- Performs Protocol A (HELLO) handshake
- Validates peer identity
- Hands validated PeerIdentity + TransportHandle to SessionManager

**What it knows about**:
- HELLO frame format (bytes)
- Peer validation rules
- Transport details (for handshake)

**What it does NOT know about**:
- Rooms or typed messages
- SessionManager internals

#### Transport Layer
**What it does**:
- Provides byte stream abstraction (send_bytes/recv_bytes)
- Handles physical connection (TCP, TLS, mock channels)
- Completely pluggable via trait

**What it knows about**:
- Network I/O or mock channel I/O
- Transport-specific config (TLS certs, etc.)

**What it does NOT know about**:
- Message framing or content
- HELLO protocol
- Application logic

---

## SessionManager: The Transport-Agnostic Core

The **SessionManager** is the central component of the network layer. It is **completely transport-agnostic** and operates entirely on typed messages, never seeing bytes.

### Core Responsibilities

1. **Manage PeerSessions**: One PeerSession per active peer connection
2. **Negotiate Rooms**: Auto-join rooms via PublishRooms intersection
3. **Route Typed Messages**: Between components and PeerSessions
4. **Lifecycle Management**: Notify components of peer connect/disconnect

### Critical Design Constraints

**SessionManager MUST**:
- ✅ Operate only on typed messages (never bytes)
- ✅ Compile without transport crate dependency
- ✅ Be fully testable with mock TransportHandles
- ✅ Know nothing about serialization formats
- ✅ Know nothing about HELLO protocol details

**SessionManager MUST NOT**:
- ❌ Call any transport methods directly (only via TransportHandle abstraction)
- ❌ Serialize or deserialize messages (that's PeerSession's job)
- ❌ Participate in HELLO handshake (receives validated PeerIdentity)
- ❌ Know whether transport is TCP, mock, or something else

### Structure

```rust
pub struct SessionManager {
    // All active peer sessions
    peers: HashMap<PeerId, PeerSession>,

    // Room router for dispatching messages to components
    router: RoomRouter,

    // Rooms this SessionManager publishes
    published_rooms: HashSet<String>,
}

pub struct PeerSession {
    peer_id: PeerId,
    peer_identity: PeerIdentity,  // From HELLO Handler
    active_rooms: Vec<String>,    // Intersection of PublishRooms
    transport_handle: TransportHandle,  // Abstract send/recv
}

// Abstract interface - SessionManager never knows the concrete type
pub trait TransportHandle: Send {
    fn send_bytes(&self, bytes: Vec<u8>) -> Result<()>;
    fn recv_bytes(&self) -> Result<Vec<u8>>;
}
```

### Room Negotiation Flow

```rust
// 1. HELLO Handler completes, hands off to SessionManager
session_manager.create_peer_session(peer_identity, transport_handle);

// 2. SessionManager initiates room negotiation
let local_rooms = session_manager.published_rooms.clone();
peer_session.send_publish_rooms(local_rooms);

// 3. Receive remote peer's PublishRooms
let remote_rooms = peer_session.recv_publish_rooms()?;

// 4. Compute intersection (auto-join)
let active_rooms = local_rooms
    .intersection(&remote_rooms)
    .cloned()
    .collect::<Vec<_>>();

// 5. If no common rooms, disconnect
if active_rooms.is_empty() {
    return Err("No compatible rooms");
}

// 6. Store active rooms and notify components
peer_session.active_rooms = active_rooms.clone();
for room in active_rooms {
    router.dispatch(PeerConnected {
        peer_id: peer_session.peer_id,
        room: room.clone(),
    });
}
```

### Message Routing Flow

**Outbound** (Component → Peer):
```rust
// Component sends typed message
memdb_component.send_to_peer(
    peer_id,
    "memdb",
    MemDBMessage::Insert { key, value }
);

// SessionManager receives typed message
impl SessionManager {
    fn send_to_peer<T: Serialize>(
        &self,
        peer_id: PeerId,
        room: &str,
        message: T
    ) -> Result<()> {
        // Find PeerSession
        let peer = self.peers.get(&peer_id)?;

        // Verify room is active
        if !peer.active_rooms.contains(&room.to_string()) {
            return Err("Room not active");
        }

        // Delegate to PeerSession (it handles serialization)
        peer.send_message(room, message)
    }
}

// PeerSession serializes and sends via transport
impl PeerSession {
    fn send_message<T: Serialize>(
        &self,
        room: &str,
        message: T
    ) -> Result<()> {
        // Serialize typed message to bytes
        let message_bytes = bincode::serialize(&message)?;

        // Wrap in RoomMessage envelope
        let envelope = RoomMessage {
            room: room.to_string(),
            data: message_bytes,
        };
        let envelope_bytes = bincode::serialize(&envelope)?;

        // Send via abstract transport
        self.transport_handle.send_bytes(envelope_bytes)
    }
}
```

**Inbound** (Peer → Component):
```rust
// PeerSession receives bytes from transport
impl PeerSession {
    fn handle_received_bytes(&self, bytes: Vec<u8>) -> Result<()> {
        // Deserialize envelope
        let envelope: RoomMessage = bincode::deserialize(&bytes)?;

        // Route to SessionManager with room + bytes
        session_manager.route_message(
            self.peer_id,
            envelope.room,
            envelope.data  // Still bytes - component will deserialize
        );
    }
}

// SessionManager routes to component
impl SessionManager {
    fn route_message(
        &self,
        peer_id: PeerId,
        room: String,
        data: Vec<u8>
    ) {
        // Dispatch to component registered for this room
        self.router.dispatch(MessageFromPeer {
            peer_id,
            room,
            data,  // Component deserializes to its typed message
        });
    }
}
```

### Testing Strategy

**The Design Validation**: Two SessionManagers must be able to communicate via in-memory channels without any network I/O.

```rust
#[test]
fn test_session_manager_communication() {
    // Create mock transport (in-memory channels)
    let (tx_a, rx_a) = channel();
    let (tx_b, rx_b) = channel();

    let handle_a = MockTransportHandle { tx: tx_a, rx: rx_b };
    let handle_b = MockTransportHandle { tx: tx_b, rx: rx_a };

    // Create two SessionManagers
    let mut sm_a = SessionManager::new();
    let mut sm_b = SessionManager::new();

    // Both publish "memdb" room
    sm_a.published_rooms.insert("memdb".to_string());
    sm_b.published_rooms.insert("memdb".to_string());

    // Create peer sessions (bypassing HELLO)
    let peer_id_b = PeerId::new();
    sm_a.create_peer_session(
        PeerIdentity { hostname: "database".into(), role: Role::Database },
        handle_a
    );

    let peer_id_a = PeerId::new();
    sm_b.create_peer_session(
        PeerIdentity { hostname: "collector".into(), role: Role::Collector },
        handle_b
    );

    // Room negotiation happens automatically

    // Send typed message from A to B
    sm_a.send_to_peer(
        peer_id_b,
        "memdb",
        MemDBMessage::Insert { key: "test".into(), value: 42 }
    );

    // B receives and processes
    let received = sm_b.recv_message_blocking()?;
    assert_eq!(received.room, "memdb");

    let msg: MemDBMessage = bincode::deserialize(&received.data)?;
    assert_eq!(msg, MemDBMessage::Insert { key: "test".into(), value: 42 });
}
```

**This test proves**:
- SessionManager works without any network
- Transport is truly abstracted
- Components can be tested in isolation

### Why This Design?

**Testability**: Most tests can run without network I/O (fast, reliable, no ports/firewalls)

**Modularity**: Each layer has clear boundaries and minimal dependencies

**Flexibility**:
- Swap TCP for mock without changing SessionManager
- Swap bincode for protobuf without changing SessionManager
- Add new transports without touching SessionManager

**Type Safety**:
- Components work with typed messages
- Compiler catches message type mismatches
- No casting or dynamic typing

**Simplicity**:
- SessionManager has one job: route typed messages between components and peers
- No serialization logic mixed with routing logic
- No transport logic mixed with application logic

---

## Component Design Patterns

These patterns show how components interact with the network layer. **Critical**: The same component code runs on both sides of a connection, just configured differently.

### Example: MemDB Component (Same Code Both Sides)

The MemDB component demonstrates the symmetric protocol design. The **exact same code** runs on both Collector and Database, but with different configuration.

**On Collector**:
```rust
let memdb = MemDB::new(MemDBConfig {
    role: Role::Collector,
    mode: Mode::SendUpdates,  // Push changes to Database
    persistence: None,         // No local storage
});
```

**On Database**:
```rust
let memdb = MemDB::new(MemDBConfig {
    role: Role::Database,
    mode: Mode::ReceiveUpdates,  // Receive changes from Collectors
    persistence: Some(persistence_config),  // Persist to disk
});
```

**Communication Flow**:
```
Collector MemDB                    Database MemDB
     |                                    |
     | MemDBMessage::Insert { key, val }  |
     |------------------------------------>|
     |                                    | (stores in DB)
     |                                    |
     |  MemDBMessage::Ack { key }         |
     |<------------------------------------|
```

Both sides understand `MemDBMessage` and handle it according to their configuration. Neither side knows or cares whether the peer is a Collector or Database—they just exchange typed messages.

### Pattern 1: Component Structure (Generic)

**Purpose**: Self-contained component that can communicate with its peer instances.

**Characteristics**:
- One instance per service (may spawn per-connection actors internally if needed)
- Same code on both sides, different config
- Publishes room names it supports
- Sends/receives typed messages

**Typical State**:
```rust
pub struct MemDBActor {
    // Configuration determines behavior
    config: MemDBConfig,

    // Component-specific state
    data: HashMap<String, Value>,

    // Track active peer connections (optional)
    peers: HashMap<PeerId, PeerState>,
}
```

**Typical Message Handlers**:
```rust
// From SessionManager (via RoomRouter)
impl Handler<PeerConnected> for MemDBActor { ... }
impl Handler<PeerDisconnected> for MemDBActor { ... }
impl Handler<MessageFromPeer<MemDBMessage>> for MemDBActor { ... }

// From local actors (intra-process communication)
impl Handler<LocalQuery> for MemDBActor { ... }

// Internal logic
impl MemDBActor {
    fn handle_insert(&mut self, peer: PeerId, key: String, value: Value) {
        match self.config.mode {
            Mode::ReceiveUpdates => {
                // Store and send ack
                self.data.insert(key.clone(), value);
                self.send_to_peer(peer, MemDBMessage::Ack { key });
            }
            Mode::SendUpdates => {
                // Ignore inserts from peer (shouldn't happen)
                warn!("Received insert in SendUpdates mode");
            }
        }
    }
}
```

### Pattern 2: Per-Connection State (Optional Pattern)

**Purpose**: Some components may need to track per-peer state internally.

**When to Use**: Only if the component needs different state per peer connection. Many components don't need this.

**Example**: MemDB might track sync status per peer:
```rust
pub struct MemDBActor {
    config: MemDBConfig,
    data: HashMap<String, Value>,

    // Per-peer state (optional pattern)
    peer_state: HashMap<PeerId, PeerSyncState>,
}

struct PeerSyncState {
    last_sync_timestamp: Timestamp,
    pending_acks: HashSet<String>,
}

impl Handler<PeerConnected> for MemDBActor {
    fn handle(&mut self, msg: PeerConnected, _ctx: &mut Context<Self>) {
        // Initialize per-peer state
        self.peer_state.insert(msg.peer_id, PeerSyncState::default());

        // Send initial sync if configured to do so
        if self.config.mode == Mode::SendUpdates {
            self.send_initial_sync(msg.peer_id);
        }
    }
}

impl Handler<PeerDisconnected> for MemDBActor {
    fn handle(&mut self, msg: PeerDisconnected, _ctx: &mut Context<Self>) {
        // Clean up per-peer state
        self.peer_state.remove(&msg.peer_id);
    }
}
```

**Key Point**: Per-connection state is **internal to the component**. The network layer (SessionManager) doesn't know or care about it. The component receives `PeerConnected`/`PeerDisconnected` events and manages its own state.

### Pattern 3: Lifecycle Management

**Connection Established Flow**:
```
1. Transport provides new connection
2. HELLO Handler performs Protocol A handshake (bytes)
3. HELLO Handler validates peer identity
4. HELLO Handler hands PeerIdentity + TransportHandle to SessionManager
5. SessionManager creates PeerSession
6. PeerSession negotiates rooms via PublishRooms intersection
7. For each active room, SessionManager sends PeerConnected to component
8. Component (e.g., MemDB) initializes per-peer state
9. Component sends initial sync messages if needed
```

**Message Exchange Flow** (Collector → Database):
```
1. Collector MemDB: memdb.send_to_peer(db_peer, MemDBMessage::Insert { ... })
2. SessionManager: routes to PeerSession for db_peer
3. PeerSession: serializes MemDBMessage to bytes
4. TransportHandle: sends bytes to transport
5. Transport: sends bytes over network (TCP/TLS/mock)
6. Remote Transport: receives bytes
7. Remote PeerSession: deserializes bytes to MemDBMessage
8. Remote SessionManager: routes to MemDB component (room="memdb")
9. Database MemDB: handles Insert, stores data, sends Ack
10. (Flow reverses for Ack message)
```

**Connection Terminated Flow**:
```
1. Transport detects connection loss (or receives close)
2. SessionManager notifies all components with active rooms
3. SessionManager sends PeerDisconnected to each component
4. Components clean up per-peer state
5. SessionManager removes PeerSession
```

**Key Points**:
- Lifecycle is **explicit and observable** - every transition is a message
- Same flow for all components (MemDB, IntentConfig, etc.)
- Components are completely decoupled from transport layer
- SessionManager manages all coordination

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

### Crate 2: `zznet-hello` (HELLO Protocol Handler)

**Location**: `src/components/zznet-hello/`

**Purpose**: Implement Protocol A (HELLO handshake) - peer identity exchange and validation. Operates on bytes, sits between transport and SessionManager.

**Key Types**:

```rust
/// Per-connection handler for HELLO protocol
pub struct HelloHandler {
    transport: Box<dyn TransportConnection>,
}

/// Result of successful HELLO handshake
pub struct PeerIdentity {
    hostname: String,
    role: AuthRole,
    protocol_version: String,
}

/// HELLO protocol frames (serialized to bytes)
#[derive(Serialize, Deserialize)]
enum HelloFrame {
    Hello {
        protocol_family: String,  // "zznet"
        version: String,           // "1.0"
        hostname: String,
        role: AuthRole,
    },
    HelloAck {
        accepted: bool,
        hostname: String,
        role: AuthRole,
    },
}

impl HelloHandler {
    /// Perform HELLO handshake (sends Hello, receives HelloAck)
    pub async fn perform_handshake(&mut self) -> Result<PeerIdentity> {
        // Send Hello frame (bytes)
        // Receive HelloAck (bytes)
        // Validate peer
        // Return PeerIdentity if successful
    }
}
```

**Dependencies**: `zznet-transport`, `serde`, `bincode`

**Responsibilities**:
- Serialize/deserialize HELLO frames
- Send Hello, receive HelloAck (or vice versa)
- Validate peer identity and protocol version
- Return PeerIdentity to SessionManager

**What it does NOT do**:
- Room negotiation (that's SessionManager's job)
- Typed message handling
- Application logic

---

### Crate 3: `zznet-session` (Transport-Agnostic Session Management)

**Location**: `src/components/zznet-session/`

**Purpose**: Manage PeerSessions and route typed messages. **100% transport-agnostic** - never touches bytes or serialization.

**Key Types**:

```rust
/// Singleton actor managing all peer sessions
pub struct SessionManager {
    peer_sessions: HashMap<PeerId, PeerSession>,
    router: Recipient<RoomMessage>,
    published_rooms: HashSet<String>,
}

/// Per-connection state (managed by SessionManager)
pub struct PeerSession {
    peer_id: PeerId,
    peer_identity: PeerIdentity,  // From HELLO Handler
    active_rooms: Vec<String>,    // Intersection after negotiation
    transport_handle: TransportHandle,  // Abstract interface
}

/// Abstract transport interface (SessionManager never knows concrete type)
pub trait TransportHandle: Send {
    fn send_bytes(&self, bytes: Vec<u8>) -> Result<()>;
    fn recv_bytes(&self) -> Result<Vec<u8>>;
}

/// Messages for room negotiation (Protocol B - typed!)
#[derive(Serialize, Deserialize)]
enum SessionMessage {
    PublishRooms { rooms: Vec<String> },
    RoomMessage { room: String, data: Vec<u8> },  // data is serialized component message
}

```

**Dependencies**: `zznet-hello` (for `PeerIdentity`), `actix`, `serde` (but NOT zznet-transport!)

**Critical Constraint**: SessionManager must compile without any transport crate dependency. It only knows about the TransportHandle trait.

**What it does**:
- Create PeerSession after HELLO completes
- Negotiate rooms via PublishRooms intersection (Protocol B)
- Route typed messages between components and PeerSessions
- Notify components of peer connect/disconnect

**What it does NOT do**:
- HELLO protocol (that's HelloHandler's job)
- Direct transport access (only via TransportHandle trait)
- Serialization of component messages (PeerSession handles that)

---

### Crate 4: `zznet-router` (Message Routing)

impl SessionManager {
    /// Called after HELLO Handler completes
    pub fn create_peer_session(
        &mut self,
        peer_identity: PeerIdentity,
        transport_handle: Box<dyn TransportHandle>,
    ) -> Result<PeerId> {
        let peer_id = PeerId::new();

        // Create PeerSession
        let session = PeerSession {
            peer_id,
            peer_identity,
            active_rooms: vec![],  // Negotiated next
            transport_handle,
        };

        self.peer_sessions.insert(peer_id, session);

        // Start room negotiation (Protocol B - typed messages!)
        self.negotiate_rooms(peer_id)?;

        Ok(peer_id)
    }

    fn negotiate_rooms(&mut self, peer_id: PeerId) -> Result<()> {
        let session = self.peer_sessions.get_mut(&peer_id)?;

        // Send our published rooms
        let msg = SessionMessage::PublishRooms {
            rooms: self.published_rooms.iter().cloned().collect(),
        };
        session.send_message(msg)?;  // PeerSession handles serialization

        // Receive peer's published rooms
        let peer_msg = session.recv_message()?;  // PeerSession handles deserialization

        if let SessionMessage::PublishRooms { rooms: peer_rooms } = peer_msg {
            // Compute intersection
            let active_rooms: Vec<String> = self.published_rooms
                .intersection(&peer_rooms.into_iter().collect())
                .cloned()
                .collect();

            if active_rooms.is_empty() {
                return Err("No compatible rooms");
            }

            session.active_rooms = active_rooms.clone();

            // Notify components
            for room in active_rooms {
                self.router.send(PeerConnected {
                    peer_id,
                    room: room.clone(),
                }).await?;
            }
        }

        Ok(())
    }

    /// Component sends typed message to peer
    pub fn send_to_peer<T: Serialize>(
        &self,
        peer_id: PeerId,
        room: &str,
        message: T,
    ) -> Result<()> {
        let session = self.peer_sessions.get(&peer_id)?;

        // Verify room is active
        if !session.active_rooms.contains(&room.to_string()) {
            return Err("Room not active");
        }

        // Delegate to PeerSession (it handles serialization)
        session.send_room_message(room, message)
    }
}

/// Per-connection state handles serialization (SessionManager doesn't!)
impl PeerSession {
    fn send_message(&self, msg: SessionMessage) -> Result<()> {
        // Serialize SessionMessage to bytes
        let bytes = bincode::serialize(&msg)?;

        // Send via abstract transport
        self.transport_handle.send_bytes(bytes)
    }

    fn recv_message(&self) -> Result<SessionMessage> {
        // Receive bytes via abstract transport
        let bytes = self.transport_handle.recv_bytes()?;

        // Deserialize to SessionMessage
        bincode::deserialize(&bytes)
    }

    fn send_room_message<T: Serialize>(&self, room: &str, msg: T) -> Result<()> {
        // Serialize component message to bytes
        let data = bincode::serialize(&msg)?;

        // Wrap in RoomMessage
        let session_msg = SessionMessage::RoomMessage {
            room: room.to_string(),
            data,
        };

        // Send via abstract transport
        self.send_message(session_msg)
    }
}

/// Handle for components to send to peer (simplified for illustration)
/// CRITICAL: This handle uses RAII (Drop) to guarantee the "fail atomically" invariant.
pub struct PeerHandle {
    peer_id: PeerId,
    session_manager: Addr<SessionManager>,
}

impl PeerHandle {
    pub async fn send<T: Serialize>(&self, room: &str, message: T) -> Result<()> {
        self.session_manager.send(SendToPeer {
            peer_id: self.peer_id,
            room: room.to_string(),
            message_bytes: bincode::serialize(&message)?,
        }).await
    }
}

}

---

### Crate 4: `zznet-router` (Message Routing)

**Note**: The above shows how SessionManager and PeerSession work together. PeerSession handles serialization so SessionManager doesn't have to know about bytes.

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

**Messages from SessionManager**:
```rust
#[derive(Message)]
pub enum RoomEvent {
    PeerConnected {
        peer_id: PeerId,
        room: String,
        peer_identity: PeerIdentity,  // From HELLO
    },
    MessageFromPeer {
        peer_id: PeerId,
        room: String,
        data: Vec<u8>,  // Serialized component message
    },
    PeerDisconnected {
        peer_id: PeerId,
        room: String,
    },
}
```

**Logic**:
```rust
impl Handler<RoomEvent> for RoomRouter {
    fn handle(&mut self, msg: RoomEvent, _ctx: &mut Context<Self>) {
        match &msg {
            RoomEvent::PeerConnected { room, .. } => {
                // Route to specific room handler
                if let Some(handler) = self.handlers.get(room) {
                    handler.do_send(msg);
                }
            }
            RoomEvent::MessageFromPeer { room, .. } => {
                // Route to specific room handler
                if let Some(handler) = self.handlers.get(room) {
                    handler.do_send(msg);
                }
            }
            RoomEvent::PeerDisconnected { room, .. } => {
                // Route to specific room handler
                if let Some(handler) = self.handlers.get(room) {
                    handler.do_send(msg);
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

**2a. Empty Room Intersection**
- **Cause**: After room negotiation, the intersection of offered rooms is empty (no compatible protocols)
- **Handling**: SessionActor logs warning "No compatible rooms with peer {hostname}" and stops immediately
- **Result**: Both sides disconnect. If client, it will retry connection, repeating the warning. This indicates a configuration error that operators must fix.
- **Example**: Collector offers ["intent-config", "health"], Database offers ["metrics", "alerts"], intersection = [] → disconnect

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

## Backpressure and Flow Control

A critical aspect of any network system is how it handles the case where data is produced faster than it can be consumed. This section defines the backpressure strategy and provides guidance for application developers.

### The SessionHandle.send() Contract

When an application session actor calls `session_handle.send()`, the contract is explicit:

```rust
pub enum SendError {
    /// The connection is closed (protocol actor stopped)
    ConnectionClosed,

    /// The send channel is full (backpressure)
    ChannelFull,

    /// Failed to serialize data (should never happen with correct types)
    SerializationError(bincode::Error),
}

impl SessionHandle {
    /// Queue data for sending on this connection.
    ///
    /// **Return value**: Ok(()) means data was queued to the protocol actor's mailbox.
    /// This does NOT mean:
    /// - Data has been sent on the wire
    /// - Data has been received by the peer
    /// - Data will definitely be delivered
    ///
    /// **Errors**:
    /// - ConnectionClosed: The connection is no longer active. Stop this session actor.
    /// - ChannelFull: Backpressure - the send buffer is full. Decide how to handle.
    /// - SerializationError: Logic bug - the data couldn't be serialized.
    pub async fn send(&self, room: String, data: Vec<u8>) -> Result<(), SendError> {
        let inner = self.inner.as_ref().ok_or(SendError::ConnectionClosed)?;

        inner.sender.send((room, data))
            .await
            .map_err(|_| SendError::ChannelFull)
    }
}
```

**The critical error is `ChannelFull`**. This means the bounded channel between the application session actor and the protocol session actor is full. The protocol actor cannot keep up with the rate of messages being sent.

### Why Bounded Channels?

**We use bounded channels** (not unbounded) because:
1. **Prevents OOM**: Unbounded queues can grow without limit, consuming all memory
2. **Explicit backpressure**: The producer (application) is forced to handle the "too fast" case
3. **Failure detection**: If queues are growing unbounded, something is fundamentally wrong

**The downside**: Application developers must explicitly handle `ChannelFull` errors. This is a trade-off we accept for safety and explicitness.

### Backpressure Handling Patterns

Different application components have different requirements. Here are the recommended patterns:

#### Pattern 1: Drop Old Data (Latency-Sensitive)

**Use case**: Real-time monitoring, health checks, pings

**Strategy**: If the channel is full, drop the current message. The latest data is more valuable than stale data.

```rust
impl IntentConfigSessionActor {
    async fn send_ping(&self, ping: PingMessage) {
        let bytes = bincode::serialize(&ping).unwrap();

        match self.session_handle.send("pinger", bytes).await {
            Ok(()) => {
                // Sent successfully
            }
            Err(SendError::ChannelFull) => {
                // Drop this ping, it's already stale
                self.metrics.dropped_pings.inc();
            }
            Err(SendError::ConnectionClosed) => {
                // Connection died, stop actor
                ctx.stop();
            }
            Err(SendError::SerializationError(e)) => {
                panic!("Serialization bug: {:?}", e);
            }
        }
    }
}
```

**Pros**: Simple, never blocks, system stays responsive
**Cons**: Data loss (acceptable for latency-sensitive data)

#### Pattern 2: Replace Old Data (State Synchronization)

**Use case**: Configuration sync, state replication

**Strategy**: If the channel is full, the old queued data is obsolete anyway. Keep only the latest.

```rust
impl IntentConfigSessionActor {
    async fn send_config(&mut self, config: ConfigMessage) {
        let bytes = bincode::serialize(&config).unwrap();

        // Store as "pending" config
        self.pending_config = Some(config.clone());

        match self.session_handle.send("intent-config", bytes).await {
            Ok(()) => {
                self.pending_config = None;
            }
            Err(SendError::ChannelFull) => {
                // Old config updates in queue are obsolete
                // We'll retry with the latest config
                self.schedule_retry();
            }
            Err(SendError::ConnectionClosed) => {
                ctx.stop();
            }
            Err(SendError::SerializationError(e)) => {
                panic!("Serialization bug: {:?}", e);
            }
        }
    }

    fn schedule_retry(&mut self) {
        // Try again in 100ms with the latest pending config
        ctx.run_later(Duration::from_millis(100), |act, ctx| {
            if let Some(config) = act.pending_config.clone() {
                act.send_config(config);
            }
        });
    }
}
```

**Pros**: Never blocks, guarantees latest state is eventually sent
**Cons**: Requires state tracking, retry logic

#### Pattern 3: Apply Backpressure (Reliable Delivery)

**Use case**: Metrics collection, event logging, audit trails

**Strategy**: If the channel is full, block (or return error to caller) until space is available. Do not drop data.

```rust
impl MetricsSessionActor {
    async fn send_metric(&self, metric: MetricMessage) -> Result<(), MetricError> {
        let bytes = bincode::serialize(&metric)?;

        // Retry loop with exponential backoff
        let mut retry_delay = Duration::from_millis(10);
        loop {
            match self.session_handle.send("metrics", bytes.clone()).await {
                Ok(()) => {
                    return Ok(());
                }
                Err(SendError::ChannelFull) => {
                    // Apply backpressure: wait and retry
                    tokio::time::sleep(retry_delay).await;
                    retry_delay = std::cmp::min(retry_delay * 2, Duration::from_secs(1));
                }
                Err(SendError::ConnectionClosed) => {
                    return Err(MetricError::ConnectionLost);
                }
                Err(SendError::SerializationError(e)) => {
                    panic!("Serialization bug: {:?}", e);
                }
            }
        }
    }
}
```

**Pros**: Reliable delivery, no data loss
**Cons**: Can block the sender, may cause head-of-line blocking

#### Pattern 4: Aggregate and Compress (High-Throughput)

**Use case**: High-frequency events, telemetry streams

**Strategy**: If the channel is full, aggregate multiple events into batches, reducing message count.

```rust
impl TelemetrySessionActor {
    async fn send_events(&mut self, events: Vec<Event>) {
        // Try to send individual events
        for event in events {
            let bytes = bincode::serialize(&event).unwrap();

            match self.session_handle.send("telemetry", bytes).await {
                Ok(()) => {
                    // Sent successfully
                }
                Err(SendError::ChannelFull) => {
                    // Buffer this event for batching
                    self.buffer.push(event);

                    // If buffer is large enough, send as batch
                    if self.buffer.len() >= 100 {
                        self.flush_batch().await;
                    }
                }
                Err(SendError::ConnectionClosed) => {
                    ctx.stop();
                    return;
                }
                Err(SendError::SerializationError(e)) => {
                    panic!("Serialization bug: {:?}", e);
                }
            }
        }
    }

    async fn flush_batch(&mut self) {
        let batch = BatchMessage {
            events: std::mem::take(&mut self.buffer),
        };
        let bytes = bincode::serialize(&batch).unwrap();

        // Send batch (may still fail if channel is full)
        let _ = self.session_handle.send("telemetry", bytes).await;
    }
}
```

**Pros**: Reduces message overhead, handles bursts well
**Cons**: Adds latency, complexity in batching logic

### Channel Sizing

The bounded channel size is a tuning parameter. Recommendations:

- **Default**: 100 messages
- **Latency-sensitive** (pings, health): 10 messages (fail fast if slow)
- **High-throughput** (metrics, logs): 1000 messages (buffer bursts)

**Rule of thumb**: The channel should buffer ~1 second of typical load. If the protocol actor can't keep up for more than 1 second, something is wrong and backpressure should kick in.

### What About Receiving Data?

**Receiving is simpler**: The protocol session actor reads frames from the transport and dispatches them to the application session actor via its mailbox. Actix mailboxes are bounded (default 16), so automatic backpressure is applied.

**If an application session actor is slow**, its mailbox fills up, the protocol actor's sends block, the TCP receive buffer fills up, and the peer experiences TCP backpressure. This is the correct behavior.

**Application developers should**:
- Process messages quickly (< 1ms per message)
- If expensive work is needed, spawn a task or send to a worker pool
- Never block the actor's message handler

### Key Principle: Explicit is Better Than Implicit

**We force application developers to think about backpressure** by making `ChannelFull` an explicit error. This is intentional. Different applications have different requirements, and the framework should not make policy decisions for them.

**The alternative** (unbounded queues, silent dropping) hides problems until production, when memory exhaustion or data loss occurs mysteriously. By making backpressure explicit, we force correct-by-construction designs.

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

## Summary of Architectural Invariants

This section collects all the invariants identified throughout this document in one place. These are the properties that **must always be true** for the system to function correctly.

### Protocol-Level Invariants

1. **"Room Names Are Protocol Identifiers"**: A room name (e.g., "intent-config") must mean the same protocol (same message types, same serialization format) on both sides of a connection. Room names are part of the global contract across all zzping services.

2. **"HELLO Before Application Protocol"**: The HELLO negotiation phase (Protocol A) must complete successfully before SessionManager creates a PeerSession and begins room negotiation (Protocol B). HELLO Handler completes first, then hands validated PeerIdentity to SessionManager.

3. **"Empty Intersection = Error"**: If room negotiation produces an empty intersection (no compatible rooms), the connection must be terminated immediately with an error logged on both sides.

4. **"FIFO Within a Room"**: Messages sent to the same room on the same connection are delivered in FIFO order. This is guaranteed by the TCP transport and actor mailbox ordering.

### Lifecycle Invariants

5. **"Static Wiring"**: Room registrations never change after boot. The set of rooms a service can handle is fixed for the lifetime of the process. (R8)

6. **"Session-to-Connection 1:1"**: There is exactly one protocol session actor per active transport connection. No sharing, no multiplexing at the session level.

7. **"SessionActive Before DataForRoom"**: For any connection, the `SessionActive` event is fully processed by application components before any `DataForRoom` events arrive for that connection. This prevents orphan data and race conditions.

8. **"No Orphan Messages"**: A room message is never sent to a transport connection without a corresponding registered application subscriber. This is enforced by static registration and boot-time validation.

### Failure Invariants

9. **"Fail Atomically"**: If any part of a connection's vertical slice fails (application session actor, protocol session actor, or transport), the entire slice must be torn down atomically. Partial failures must not leave dangling state. This is **guaranteed** by the RAII pattern on `SessionHandle`.

10. **"SessionHandle Enforces Teardown"**: When a `SessionHandle` is dropped (explicitly via `.close()` or when the owning actor stops), it automatically triggers teardown of the entire connection. This is enforced by Rust's type system via the `Drop` trait.

11. **"Main Actor Failure = Service Failure"**: If a main application actor or infrastructure actor (RoomRouter, SessionManager) panics, the entire service should crash and restart. There is no partial recovery from singleton actor failures.

12. **"Transport Errors Are Expected"**: The transport layer must never panic due to I/O errors. All I/O operations must be wrapped in `Result<T, E>` and handled explicitly. This is the exception to "fail fast, fail loud."

### Message Constraints

13. **"16 MiB Maximum Message Size"**: No message may exceed 16 MiB. The protocol layer rejects messages exceeding this limit before sending. The transport layer closes the connection if it receives a frame header claiming >16 MiB. This is a hard, compile-time constant.

14. **"Heartbeat Keepalive"**: The transport layer sends zero-sized frames every ~1 second as heartbeat. Both sides must send heartbeats. If no frames (heartbeat or data) are received for >5 seconds, the connection is assumed dead and closed.

### Ordering Guarantees

13. **"FIFO Within a Room"** (restated for emphasis): Messages sent to the same room on the same connection arrive in order at the peer.

14. **"No Ordering Between Rooms"**: Messages sent to different rooms on the same connection have no ordering guarantee. They come from independent actors with independent timing.

15. **"No Ordering Across Connections"**: Messages from different connections arrive in arbitrary order. Network latency and scheduler timing are unpredictable.

### Boot-Time Guarantees

16. **"Offered Rooms Must Have Handlers"**: Every room in the "offered rooms" list must have a registered handler in the RoomRouter. This is validated at boot time before the transport is attached. Violation causes a panic with a clear error message.

17. **"Pre-Online State"**: After wiring is complete and validated, but before the transport is attached, the service is in a "pre-online" state where all internal actors are running and ready, but no network connections are accepted. This is an intentional architectural feature.

### Backpressure Guarantees

18. **"Bounded Channels"**: Communication channels between components and SessionManager (and internally within SessionManager to PeerSessions) are bounded. When full, send operations return errors, forcing explicit backpressure handling.

19. **"send() Means Queued"**: A successful send from a component to SessionManager means the message was queued for delivery. It does NOT mean data was sent on the wire or received by the peer. Components must use application-level acknowledgments if they need delivery confirmation.

### Peer Identity Guarantees

20. **"Peer Context Is Always Available"**: Application components always know WHO they're connected to (peer hostname, role, protocol version). This information is included in the `PeerConnected` event and comes from the validated PeerIdentity (which originated from the HELLO handshake performed by the HelloHandler).

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
2. Begin implementation of `zznet-hello` crate (Protocol A - HELLO handler for peer identity)
3. Begin implementation of `zznet-session` crate (Protocol B - SessionManager + PeerSession for typed messages)
4. Implement mock TransportHandle for testing
5. Validate architecture by testing two SessionManagers communicating via mock (no network I/O)
6. Implement one component (e.g., MemDB) to validate the design with same code on both sides
7. Iterate based on learnings

---

**Document End**
