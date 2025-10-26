# ZZNet Component Framework: Vision & Architecture

**Document Purpose**: Comprehensive architectural vision for the ZZNet component framework and how it integrates with ZZPing applications.

**Date**: October 21, 2025
**Status**: Authoritative Reference
**Supersedes**: Previous component framework documents

---

## Executive Summary

**ZZNet** is a component framework for building distributed applications using Actix actors that communicate over the network via typed Rust messages. Components remain completely transport-agnostic - they work identically whether connected via TCP/TLS or in-memory mock channels.

### Core Vision

**Components communicate using typed messages, with zero knowledge of transport, serialization, or network topology.**

```rust
// Component developer writes this:
self.session_manager.send_to_room(
    peer_id,
    RoomId::from("memdb"),
    MemDBMessage::SubmitBatch { results }
);

// Framework handles:
// - Serialization (typed message → bytes)
// - Transport (bytes → network)
// - Routing (which peer, which room)
// - Deserialization (bytes → typed message)
// - Delivery (to peer's component)
```

### Key Principles

1. **Transport-Agnostic**: Components work with TCP, mock, or future transports
2. **Mock-First Testing**: Everything testable without network I/O
3. **Pure Actor Model**: No shared state, only message passing
4. **Fire-and-Forget**: No ACKs, no retries at component level
5. **Same Code Both Sides**: Components use same implementation on client and server

---

## Table of Contents

1. [The Network Stack](#the-network-stack)
2. [Core Concepts](#core-concepts)
3. [Component Structure](#component-structure)
4. [Developer Experience](#developer-experience)
5. [Crate Responsibilities](#crate-responsibilities)
6. [Authentication & Authorization](#authentication--authorization)
7. [Connection Lifecycle](#connection-lifecycle)
8. [Testing Strategy](#testing-strategy)
9. [Common Patterns](#common-patterns)

---

## The Network Stack

### Layer Diagram

```
┌─────────────────────────────────────────────────────────────┐
│  Application Components (Business Logic)                   │
│  - MemDB, IntentConfig, CollectorState, Pinger, etc.       │
│  - Same code on collector and database (different config)  │
│  - Registers room handlers with SessionManager             │
└────────────────────────┬────────────────────────────────────┘
                         ↕ Typed Messages (Rust structs)
┌─────────────────────────────────────────────────────────────┐
│  Room<T> Layer                                              │
│  - Bidirectional typed channels per connection             │
│  - Handles message routing to/from SessionManager          │
│  - Auto-registration with SessionManager                   │
└────────────────────────┬────────────────────────────────────┘
                         ↕ Typed Messages with (PeerId, RoomId)
┌─────────────────────────────────────────────────────────────┐
│  SessionManager (Transport-Agnostic Core)                  │
│  - Manages multiple peer connections (PeerSessions)        │
│  - Routes typed messages between rooms                     │
│  - Handles room negotiation (auto-join via intersection)  │
│  - 100% typed, NEVER touches bytes                         │
│  - Completely testable without network                     │
└────────────────────────┬────────────────────────────────────┘
                         ↕ Typed Messages
┌─────────────────────────────────────────────────────────────┐
│  Serialization Layer (in Room<T> or HELLO)                 │
│  - Serializes typed messages → bytes (serde + bincode)     │
│  - Deserializes bytes → typed messages                     │
│  - THIS IS THE ONLY PLACE SERIALIZATION HAPPENS            │
└────────────────────────┬────────────────────────────────────┘
                         ↕ Vec<u8> (bytes)
┌─────────────────────────────────────────────────────────────┐
│  HELLO Handler (Protocol A: Identity Exchange)             │
│  - Peer identity exchange (hostname, role, rooms offered)  │
│  - Protocol version negotiation                            │
│  - Room intersection computation                           │
│  - Operates on bytes (transport boundary)                  │
└────────────────────────┬────────────────────────────────────┘
                         ↕ Framed bytes [u32 len][payload]
┌─────────────────────────────────────────────────────────────┐
│  Transport Layer (Pluggable)                               │
│  - TCP/TLS: Production (zznet-transport-tcp)               │
│  - Mock: Testing (zznet-api::mock)                         │
│  - Provides: send(bytes), recv() → bytes                   │
│  - Completely swappable                                    │
└─────────────────────────────────────────────────────────────┘
                         ↕ Network I/O or in-memory channels
```

### Critical Boundaries

**The SessionManager Boundary** (Most Important):
- **Above**: Typed messages only
- **Below**: Typed messages only
- **Never crosses into bytes** - this is the architecture's foundation

**The Serialization Boundary**:
- **Above**: Typed Rust structs
- **Below**: Vec<u8> bytes
- **One-way conversion** at send/receive points only

**The Transport Boundary**:
- **Above**: Framed bytes
- **Below**: Raw network I/O
- **Completely pluggable** via TransportConnection trait

---

## Core Concepts

### 1. Components

**A Component is an Actix actor that implements business logic and optionally communicates over the network.**

Components are:
- ✅ Self-contained actors with private state
- ✅ Configured via "roles" (Collector vs Database behavior)
- ✅ Testable without network (SessionManager is optional)
- ✅ Same code on both sides (just configured differently)

Components are NOT:
- ❌ Different implementations for client vs server
- ❌ Aware of transport details (TCP, sockets, etc.)
- ❌ Concerned with serialization
- ❌ Managing connections directly

**Example**: `MemDB` component
- On collector: buffers ping results, sends batches to database
- On database: receives batches, stores to disk
- Same actor implementation, different `MemDBRole` config

### 2. Rooms

**A Room is a 1:1 bidirectional typed channel between two component instances across a single connection.**

Critical properties:
- ✅ **Point-to-point**: Connects exactly two endpoints
- ✅ **Per-connection**: Each TCP connection has its own set of rooms
- ✅ **Typed channel**: Messages are strongly-typed Rust structs
- ✅ **Bidirectional**: Both sides can send and receive
- ✅ **Multiplexed**: Multiple rooms share one TCP connection
- ✅ **Auto-negotiated**: Joined automatically based on intersection

❌ **NOT a broadcast channel**: If database has 3 collectors connected:
```
Connection 1: Database ←─ "memdb" room ─→ Collector-1
Connection 2: Database ←─ "memdb" room ─→ Collector-2
Connection 3: Database ←─ "memdb" room ─→ Collector-3
```
These are **three separate rooms**, even with the same name "memdb".

### 3. SessionManager

**SessionManager is the transport-agnostic core that manages all peer connections and routes typed messages.**

Responsibilities:
- Manages `HashMap<PeerId, PeerSession>` (one per connection)
- Routes messages: `send_to_room(peer_id, room_id, message)`
- Handles connection lifecycle: `peer_connected()`, `peer_disconnected()`
- Computes room intersection during HELLO negotiation
- Delivers `SessionEvent` to components (Active/Inactive)

**Critical constraints**:
- ❌ **Never touches bytes** - only typed messages
- ❌ **Never performs serialization** - that's serialization layer's job
- ❌ **Never touches transport** - uses abstract channels
- ✅ **100% testable with mock transport** - no network required

### 4. Roles

**Roles configure component behavior for different deployment contexts.**

```rust
enum MemDBRole {
    Collector {
        buffer_capacity: usize,
        batch_size: usize,
    },
    Database {
        storage_path: PathBuf,
        retention_days: u32,
    },
}
```

**Same component, different role:**
- Collector role: buffers and sends data
- Database role: receives and persists data
- Implementation shares common logic

### 5. Fire-and-Forget Semantics

**Component messages have no delivery guarantees beyond TCP.**

What IS guaranteed (while connected):
- ✅ **Ordering within a room**: Messages arrive in FIFO order
- ✅ **TCP delivery**: If connection exists, TCP delivers it
- ✅ **Serialization correctness**: Typed → bytes → typed is lossless

What is NOT guaranteed:
- ❌ **No ACKs**: Sender doesn't know if message was received
- ❌ **No retries**: Framework doesn't retry failed sends
- ❌ **Connection loss = message loss**: In-flight messages are lost
- ❌ **No cross-room ordering**: Messages on different rooms may arrive in any order

**Design principle**: Components handle reliability at application level (e.g., MemDB buffering), not framework level.

---

## Component Structure

### Standard Component Layout

```
src/components/zzmem-db/
├── Cargo.toml
├── src/
│   ├── actor.rs              # Actor implementation (private)
│   ├── api.rs                # Public API wrapper (uses Addr)
│   ├── builder.rs            # Builder pattern for setup
│   ├── messages.rs           # Internal/local messages
│   ├── network_messages.rs   # Messages sent over network
│   ├── role.rs               # Role configuration (Collector/Database)
│   ├── permission_wrapper.rs # Optional: role → permission mapping
│   └── lib.rs                # Public exports
└── tests/
    ├── unit_tests.rs         # Fast unit tests (no network)
    └── integration_tests.rs  # With SessionManager (mock transport)
```

### Key Files

**`network_messages.rs`** - Messages that go over the wire:
```rust
use serde::{Deserialize, Serialize};
use actix::Message;

#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub enum MemDBMessage {
    SubmitBatch {
        results: Vec<PingResult>,
    },
    Query {
        target: IpAddr,
        time_range: (u64, u64),
    },
    QueryResponse {
        results: Vec<StoredPing>,
    },
}
```

**`role.rs`** - Component configuration:
```rust
#[derive(Debug, Clone)]
pub enum MemDBRole {
    Collector {
        buffer_capacity: usize,
        batch_size: usize,
    },
    Database {
        storage_path: PathBuf,
    },
}

impl MemDBRole {
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Collector { buffer_capacity, batch_size } => {
                if *batch_size > *buffer_capacity {
                    return Err(Error::InvalidConfig("batch_size > buffer_capacity"));
                }
                Ok(())
            }
            Self::Database { storage_path } => {
                // Validate path exists, writable, etc.
                Ok(())
            }
        }
    }
}
```

---

---

## Crate Responsibilities

### zznet-api
**Purpose**: Abstract transport trait definitions

**Provides**:
- `TransportConnection` trait (send/recv bytes)
- `TransportServer` trait (accept connections)
- `TransportClient` trait (create connections)
- Mock transport implementation for testing
- Shared types: `PeerId`, `RoomId`, `PeerIdentity`

**Dependencies**: None (foundation layer)

**Key principle**: Pure abstraction, no concrete implementations except mock

---

### zznet-transport-tcp
**Purpose**: Production TCP/TLS transport

**Provides**:
- `TcpTransport` implementing `TransportConnection`
- `TcpTransportServer` implementing `TransportServer`
- `TcpTransportClient` implementing `TransportClient`
- Frame protocol: `[u32 BE length][payload]`
- mTLS support with role-based certificates
- Certificate validation and identity extraction

**Dependencies**: `zznet-api`, `tokio`, `rustls`

**Key principle**: One concrete implementation of abstract transport

---

### zznet-hello
**Purpose**: HELLO protocol and serialization boundary

**Provides**:
- Protocol A (HELLO handshake): peer identity exchange
- Room negotiation: compute intersection of offered rooms
- Serialization/deserialization at transport boundary
- `HelloActor` managing connection lifecycle
- `ConnectionManager` for auto-reconnect

**Responsibilities**:
- Extract `PeerIdentity` from transport
- Exchange HELLO frames (version, role, hostname, rooms)
- Validate protocol compatibility
- Compute room intersection (fail if empty)
- Bridge between bytes (transport) and typed messages (SessionManager)

**Dependencies**: `zznet-api`, `zznet-session`, `serde`, `bincode`

**Key principle**: The only layer that handles both bytes AND typed messages

---

### zznet-session
**Purpose**: Transport-agnostic session management

**Provides**:
- `SessionManager`: manages all peer connections
- `PeerSession`: per-connection state
- Room routing: routes messages to/from component handlers
- Connection lifecycle events: `SessionEvent::Active/Inactive`

**Critical constraints**:
- ❌ **Never touches bytes** - only typed messages
- ❌ **No serialization** - doesn't know about serde
- ❌ **No transport dependency** - doesn't depend on zznet-api
- ✅ **100% testable with mock channels** - no I/O required

**Dependencies**: `actix`, `tokio` (but NOT zznet-api!)

**Key principle**: Pure typed message routing, completely transport-agnostic

---

### zznet-room
**Purpose**: Component-facing room abstraction

**Provides**:
- `Room<T>` typed channel wrapper
- Auto-registration with SessionManager
- Helper utilities for room management
- Room connector for tests (wire two rooms together)

**Responsibilities**:
- Provide ergonomic API for components
- Handle serialization for messages (typed → bytes for transport)
- Auto-register with SessionManager on construction
- Simplify per-connection room management

**Dependencies**: `zznet-session`, `actix`, `serde`

**Key principle**: Convenience layer making components easier to write

---

### zznet-auth
**Purpose**: Generic authentication/authorization traits

**Provides**:
- `ApplicationRole` trait (apps implement this)
- `PermissionCheck` trait (components use this)
- Generic ACL utilities

**Responsibilities**:
- Define generic role/permission model
- Provide reusable auth patterns
- Keep zznet auth-agnostic (apps provide concrete roles)

**Dependencies**: None (pure trait definitions)

**Key principle**: ZZNet doesn't know about zzping roles - provides generic traits for apps to implement

---

### zznet-builder
**Purpose**: High-level integration API (batteries included)

**Provides**:
- `ServerBuilder`: fluent API for creating servers
- `ClientBuilder`: fluent API for creating clients
- Automatic wiring: SessionManager + HELLO + Transport
- Support for both TCP/TLS and mock transports
- Connection lifecycle management

**Example**:
```rust
// Production server with TCP
ServerBuilder::new()
    .bind("0.0.0.0:9001")
    .with_tls(tls_config)
    .offer_rooms(vec!["memdb", "intent-config"])
    .with_session_manager(session_manager)
    .start()
    .await?;

// Test with mock transport
ServerBuilder::new()
    .with_mock_transport(mock_connector)
    .offer_rooms(vec!["memdb"])
    .with_session_manager(session_manager)
    .start()?;
```

**Dependencies**: All zznet crates

**Key principle**: Single entry point that wires everything together correctly

---

## Authentication & Authorization

### ZZNet's Responsibility: Identity Extraction

**ZZNet is auth-agnostic.** It only extracts cryptographic identity from mTLS certificates.

```rust
pub struct PeerIdentity {
    pub common_name: String,      // CN from certificate (e.g., "collector")
    pub san_username: String,      // First DNS SAN (e.g., "alice" or "root")
    pub peer_addr: String,         // IP:port for logging
}
```

**Identity format**:
- Services: `CN=collector`, `SAN=DNS:root` → identity = "collector"
- Users: `CN=client-admin`, `SAN=DNS:alice` → identity = "alice@client-admin"

### Application's Responsibility: Authorization

**Applications (zzping-database, zzping-collector) define and enforce authorization.**

```rust
// In zzping application code:
enum AuthRole {
    Collector,
    Database,
    ClientAdmin,
    ClientReadOnly,
}

impl ApplicationRole for AuthRole {
    fn from_cn(cn: &str) -> Result<Self, AuthError> {
        match cn {
            "collector" => Ok(AuthRole::Collector),
            "database" => Ok(AuthRole::Database),
            "client-admin" => Ok(AuthRole::ClientAdmin),
            "client-ro" => Ok(AuthRole::ClientReadOnly),
            _ => Err(AuthError::UnknownRole(cn.to_string())),
        }
    }

    fn can_connect_to(&self, target: &Self) -> bool {
        match (self, target) {
            (AuthRole::Collector, AuthRole::Database) => true,
            (AuthRole::ClientAdmin, AuthRole::Database) => true,
            _ => false,
        }
    }

    fn can_access_room(&self, room: &str) -> bool {
        match (self, room) {
            (AuthRole::Collector, "memdb") => true,
            (AuthRole::Collector, "intent-config") => true,
            (AuthRole::ClientAdmin, _) => true,  // Admin can access all rooms
            (AuthRole::ClientReadOnly, "memdb") => true,  // Read-only: only query
            _ => false,
        }
    }
}
```

### Security Model

**mTLS (Production)**:
- Certificate CN is authoritative (cryptographically verified)
- HELLO message role field is informational only (ignored)
- PeerIdentity extracted from certificate is trusted

**Raw TCP (Testing/Development)**:
- No certificate available
- HELLO message role field is trusted (peer can lie!)
- Explicitly insecure, requires opt-in config

---

## Connection Lifecycle

### Connection Topology

**Simple model**:
- Database = TCP server (binds to port, accepts connections)
- Collectors/Clients = TCP clients (connect to database)
- ONE connection per app instance (not per component or room)
- All rooms multiplexed over single TCP connection

```
┌──────────────────┐         ┌──────────────────┐
│  Collector-1     │────────→│                  │
└──────────────────┘         │                  │
                             │                  │
┌──────────────────┐         │   Database       │
│  Collector-2     │────────→│   (Server)       │
└──────────────────┘         │                  │
                             │   Port 9001      │
┌──────────────────┐         │                  │
│  GUI Client      │────────→│                  │
└──────────────────┘         └──────────────────┘

         Multiple TCP connections, all to same server
```

### Lifecycle Events

**1. Connection Established**
```
Transport connects → HELLO handshake → Room negotiation → SessionEvent::Active
```

Components receive:
```rust
SessionEvent::Active {
    peer_id: PeerId,          // e.g., "collector-01"
    role: AuthRole,            // Application-specific role
    rooms: Vec<RoomId>,        // Successfully negotiated rooms
}
```

**2. Connection Lost**
```
Transport error → SessionManager cleanup → SessionEvent::Inactive
```

Components receive:
```rust
SessionEvent::Inactive {
    peer_id: PeerId,
}
```

**3. Reconnection**
```
Auto-reconnect → New HELLO → New SessionEvent::Active
```

**Important**: Reconnection is treated as **new connection**, even if same peer. No state is preserved.

### Auto-Reconnect

**Client-side** (collectors, GUI):
```rust
// ClientBuilder handles reconnection automatically
loop {
    match client.connect().await {
        Ok(transport) => {
            // Connected, hand to SessionManager
            session_manager.handle_connection(transport);
            // Wait for disconnect
            transport.wait_for_close().await;
        }
        Err(e) => {
            log::warn!("Connection failed: {}, retrying in 5s", e);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
```

**Server-side** (database):
```rust
// ServerBuilder accepts connections in a loop
loop {
    let transport = server.accept().await?;
    // Spawn handler for this connection
    tokio::spawn(async move {
        session_manager.handle_connection(transport).await;
    });
}
```

---

## Testing Strategy

### Levels of Testing

**1. Unit Tests (No Network)**
```rust
#[actix::test]
async fn test_component_logic() {
    // Component without SessionManager
    let component = MyComponentBuilder::new(role).start()?;

    // Test business logic directly
    component.send(LocalMessage { .. }).await?;
}
```

**2. Component Integration Tests (Mock Transport)**
```rust
#[actix::test]
async fn test_component_communication() {
    // Two SessionManagers with mock connector
    let (sm_a, sm_b) = create_mock_session_managers();

    // Create components
    let comp_a = ComponentBuilder::new(RoleA)
        .with_session_manager(sm_a)
        .start()?;

    let comp_b = ComponentBuilder::new(RoleB)
        .with_session_manager(sm_b)
        .start()?;

    // Test typed message exchange (no network I/O)
}
```

**3. Integration Tests (Real TCP, Localhost)**
```rust
#[tokio::test]
async fn test_full_stack() {
    // Real TCP on localhost
    let server = ServerBuilder::new()
        .bind("127.0.0.1:0")  // Random port
        .start()
        .await?;

    let client = ClientBuilder::new()
        .connect_to(server.local_addr())
        .connect()
        .await?;

    // Test real transport
}
```

**4. Smoke Tests (Real TLS)**
```rust
#[tokio::test]
async fn test_with_tls() {
    // Test with real certificates
    let tls_config = TlsConfig::from_test_certs()?;
    // Verify mTLS handshake works
}
```

### Testing Philosophy

**Mock-first development**:
1. Write component with SessionManager abstraction
2. Test with mock transport (fast, deterministic)
3. Verify with real TCP (slow, validates integration)
4. Smoke test with TLS (slowest, validates production config)

**Goal**: 95%+ test coverage without any network I/O

---

## Common Patterns

### Pattern 1: Per-Connection State

**Use case**: Database component tracking multiple collectors

```rust
pub struct DatabaseActor<T> {
    role: DatabaseRole,
    // One entry per connected peer
    connections: HashMap<PeerId, ConnectionState>,
}

impl<T: ApplicationRole> Handler<SessionEvent> for DatabaseActor<T> {
    fn handle(&mut self, event: SessionEvent, _ctx: &mut Context<Self>) {
        match event {
            SessionEvent::Active { peer_id, .. } => {
                // Track new connection
                self.connections.insert(peer_id.clone(), ConnectionState::default());
            }
            SessionEvent::Inactive { peer_id } => {
                // Clean up connection state
                self.connections.remove(&peer_id);
            }
        }
    }
}
```

### Pattern 2: Broadcast to All Peers

**Use case**: Database broadcasting config update to all collectors

```rust
impl DatabaseActor<T> {
    fn broadcast_config(&self, config: Config) {
        if let Some(ref sm) = self.session_manager {
            for peer_id in self.connections.keys() {
                // Fire-and-forget to each peer
                sm.send_to_room(
                    peer_id.clone(),
                    RoomId::from("intent-config"),
                    IntentConfigMessage::ConfigUpdate { config: config.clone() }
                );
            }
        }
    }
}
```

### Pattern 3: Request/Response (with Correlation ID)

**Use case**: GUI querying database for data

```rust
// Note: Still fire-and-forget, but with correlation for matching responses

#[derive(Serialize, Deserialize)]
pub enum QueryMessage {
    Request {
        query_id: Uuid,  // Correlation ID
        target: IpAddr,
    },
    Response {
        query_id: Uuid,  // Match to request
        results: Vec<Data>,
    },
}

// Client side:
impl ClientActor {
    fn send_query(&mut self) {
        let query_id = Uuid::new_v4();
        self.pending_queries.insert(query_id, Instant::now());

        self.session_manager.send_to_room(
            database_peer,
            RoomId::from("memdb"),
            QueryMessage::Request { query_id, target: "192.168.1.1".parse().unwrap() }
        );
    }

    fn handle_response(&mut self, msg: QueryMessage) {
        if let QueryMessage::Response { query_id, results } = msg {
            if let Some(_sent_at) = self.pending_queries.remove(&query_id) {
                // Process response
            }
        }
    }
}
```

### Pattern 4: State Synchronization After Reconnect

**Use case**: Collector reconnects, needs to sync state with database

```rust
impl CollectorActor {
    fn handle_connection(&mut self, event: SessionEvent) {
        if let SessionEvent::Active { peer_id, .. } = event {
            // Request current state from database
            self.session_manager.send_to_room(
                peer_id,
                RoomId::from("c-state"),
                CStateMessage::RequestCurrentState
            );
        }
    }
}
```

### Pattern 5: Component Health Reporting

**Use case**: Components expose health for observability

```rust
#[derive(Debug, Clone)]
pub struct ComponentHealth {
    pub connected_peers: usize,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub last_activity: Option<Instant>,
}

#[derive(Message)]
#[rtype(result = "ComponentHealth")]
pub struct GetHealth;

impl<T> Handler<GetHealth> for MyComponentActor<T> {
    type Result = ComponentHealth;

    fn handle(&mut self, _msg: GetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        ComponentHealth {
            connected_peers: self.connections.len(),
            messages_sent: self.metrics.sent.load(Ordering::Relaxed),
            messages_received: self.metrics.received.load(Ordering::Relaxed),
            last_activity: self.last_activity,
        }
    }
}
```

---

## Summary: Key Architectural Decisions

### Decision 1: SessionManager is Transport-Agnostic
**Decision**: SessionManager operates entirely on typed messages, with zero knowledge of serialization or transport.

**Why**: Enables testing without network I/O, makes transport truly pluggable, simplifies reasoning.

**Trade-off**: Requires separate serialization layer between SessionManager and transport.

---

### Decision 2: Rooms Are Auto-Joined via Intersection
**Decision**: No explicit join/leave. Rooms are auto-joined based on intersection of offered rooms during HELLO.

**Why**: Simpler protocol, boot-time validation, fail-fast if incompatible, no race conditions.

**Trade-off**: Less flexibility, but we don't need dynamic room management.

---

### Decision 3: Same Component Code on Both Sides
**Decision**: Components use same implementation on client and server, configured via roles.

**Why**: All communication code in one place, easy to reason about, simpler testing, natural protocol symmetry.

**Trade-off**: Components must handle both client and server behaviors (acceptable with role enum).

---

### Decision 4: Fire-and-Forget, No Framework ACKs
**Decision**: No ACKs or retries at framework level. TCP provides ordering and delivery, application provides reliability.

**Why**: Simpler framework, avoids complexity, forces explicit reliability design, matches use case (1+ hour partition tolerance).

**Trade-off**: Components must implement application-level reliability if needed (e.g., MemDB buffering).

---

### Decision 5: ZZNet is Auth-Agnostic
**Decision**: ZZNet extracts identity, applications enforce authorization.

**Why**: Keeps zznet reusable for other applications, separates concerns, applications know their security requirements.

**Trade-off**: Applications must implement auth logic (but zznet-auth provides generic traits to help).

---

## Validation Checklist

Use this to verify implementations follow the vision:

### SessionManager Validation
- [ ] SessionManager compiles without zznet-api dependency
- [ ] SessionManager has zero references to `Vec<u8>`, bytes, or serialization
- [ ] SessionManager testable with in-memory mock channels
- [ ] Two SessionManagers can communicate with no network I/O

### Room Validation
- [ ] Rooms declared at boot time (static)
- [ ] Room negotiation via intersection (automatic)
- [ ] Empty intersection causes connection failure
- [ ] Rooms are 1:1 per connection (not broadcast)

### Component Validation
- [ ] Component's network code lives in component's crate
- [ ] Component works with different roles (client/server)
- [ ] Component registers room handlers with SessionManager
- [ ] Component receives only typed messages (never bytes)
- [ ] Component testable without SessionManager

### Transport Validation
- [ ] Clear separation: SessionManager (typed) vs Serialization (bytes)
- [ ] HELLO handler separate from SessionManager
- [ ] Transport is pluggable (can swap TCP for mock)
- [ ] Application components never directly touch transport

### Builder Validation
- [ ] ServerBuilder and ClientBuilder support both TCP and mock
- [ ] Builder wires SessionManager + HELLO + Transport automatically
- [ ] Apps don't have duplicated setup code
- [ ] Builder provides clear error messages for misconfiguration

---

## Conclusion

The ZZNet component framework provides a robust foundation for building distributed applications where:

- **Components are simple**: Just actors that handle typed messages
- **Testing is fast**: Everything works with mock transport
- **Transport is pluggable**: TCP/TLS for production, mock for tests
- **Security is clear**: mTLS provides identity, apps enforce authorization
- **Reliability is explicit**: Fire-and-forget at framework, retry at application

**Golden Rule**: If your design makes it impossible to test two SessionManagers communicating via mock transport, you've violated the core vision.

When in doubt, refer back to:
- Rooms are 1:1 typed channels (not broadcast)
- SessionManager never touches bytes
- Same component code on both sides
- Test with mock transport first
- HELLO is separate from SessionManager

Follow these principles, and the architecture will guide you to the right implementation.
