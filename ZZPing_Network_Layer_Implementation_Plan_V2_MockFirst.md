# ZZPing Network Layer Implementation Plan V2 (Mock-First)

**Document Version:** 2.0 (Complete Rewrite)
**Date:** October 2, 2025
**Status:** Planning
**Philosophy:** Mock-First, Defer Real Transport
**Related Documents:**
- [ZZPing Network Layer Actor Design](./ZZPing_Network_Layer_Actor_Design_Oct2025.md)
- [ZZPing Network Layer Vision](./ZZPing_Network_Layer_Vision.md)

---

## Executive Summary

This implementation plan **inverts the traditional approach** to network layer development. Instead of building TCP/TLS first, we:

1. Define transport as abstract trait
2. Build comprehensive mock transport
3. **Develop entire architecture using ONLY mocks**
4. **Achieve ~100% test coverage without network I/O**
5. **ONLY THEN** implement real transport (TCP/TLS)

### The Critical Insight

> **"What better way to validate transport-agnostic design than just... not writing the transport part!"**

If SessionManager is truly transport-agnostic, we should be able to achieve full functionality and near-perfect test coverage using ONLY mock transport. Real transport becomes **proof** our abstraction works, not a **requirement** for development.

### Why This Approach?

**Architectural Benefits:**
- Validates abstraction early (if mock works, design is sound)
- Finds leaky abstractions immediately (before expensive TCP/TLS work)
- Real transport becomes "just another implementation"

**Development Benefits:**
- Tests run in microseconds, not milliseconds
- No flaky network timing issues
- No certificates, ports, or network configuration
- Develop anywhere (airplane, no network needed)
- Parallel development easier (no port conflicts)

**Risk Mitigation:**
- Architecture proven before TCP/TLS commitment
- If TCP/TLS is hard to add, abstraction leaked (we know immediately)
- Changes to session layer don't require network testing

---

## Table of Contents

1. [Overview](#overview)
2. [Phase 0: Foundation](#phase-0-foundation-1-2-days)
3. [Phase 1: Mock Transport](#phase-1-mock-transport-2-3-days)
4. [Phase 2: Session Layer](#phase-2-session-layer-3-4-days)
5. [Phase 3: HELLO Handler](#phase-3-hello-handler-2-3-days)
6. [Phase 4: Router Layer](#phase-4-router-layer-3-4-days)
7. [Phase 5: Application Integration](#phase-5-application-integration-3-4-days)
8. [MILESTONE: Mock-Complete](#milestone-mock-complete-architecture)
9. [Phase 6: Real Transport (Deferred)](#phase-6-real-transport-deferred-4-5-days)
10. [Phase 7: Hardening](#phase-7-hardening-3-5-days)
11. [Phase 8: Documentation](#phase-8-documentation-2-3-days)
12. [Timeline & Dependencies](#timeline--dependencies)

---

## Overview

### Crate Structure

Five independent crates with strict dependency control:

```
zznet-api          (transport trait only, no implementations)
    ├── transport.rs      (trait definitions)
    ├── error.rs          (transport errors)
    ├── mock.rs           (mock impl - Phase 1)
    └── tcp_tls.rs        (real impl - Phase 6, DEFERRED)

zznet-hello        (Protocol A: HELLO handshake, transport boundary)
    ├── messages.rs       (HELLO protocol messages)
    ├── server.rs         (server-side HELLO)
    ├── client.rs         (client-side HELLO)
    └── peer_session.rs   (serialization bridge)
    depends on: zznet-api

zznet-session      (Protocol B: SessionManager, 100% transport-agnostic)
    ├── messages.rs       (typed AppMessages)
    ├── session.rs        (SessionManager actor)
    ├── handle.rs         (SessionHandle with RAII)
    ├── events.rs         (SessionEvent for router)
    └── factory.rs        (creates sessions)
    depends on: NOTHING (no zznet-api dependency!)

zznet-router       (Room-based message routing)
    ├── router.rs         (Router actor)
    └── types.rs          (routing types)
    depends on: zznet-session

zznet              (High-level integration API)
    ├── server.rs         (ServerBuilder)
    ├── client.rs         (ClientBuilder)
    └── acceptor.rs       (TransportAcceptor)
    depends on: all above
```

### Architecture Boundaries

**Key Separation:**
```
Transport Layer (bytes)
    ↓
[ HELLO HANDLER ] ← Transport Boundary
    ↓
Session Layer (typed messages)
    ↓
Router Layer (rooms)
    ↓
Application Layer
```

**Critical Constraints:**
- SessionManager NEVER touches transport (not even the trait)
- HELLO handler is the ONLY component that knows about bytes
- Router only knows about typed messages
- Application only knows about rooms

### Phase Overview

| Phase | Component | Days | Transport | Coverage |
|-------|-----------|------|-----------|----------|
| 0 | Foundation | 1-2 | None | N/A |
| 1 | Mock Transport | 2-3 | **Mock** | >90% |
| 2 | Session Layer | 3-4 | **None!** | >90% |
| 3 | HELLO Handler | 2-3 | **Mock** | >85% |
| 4 | Router Layer | 3-4 | **Mock** | >85% |
| 5 | App Integration | 3-4 | **Mock** | >85% |
| **MILESTONE** | **Mock-Complete** | **14-20** | **Mock Only** | **>95%** |
| 6 | TCP/TLS Transport | 4-5 | **Real** | >80% |
| 7 | Hardening | 3-5 | Both | >90% |
| 8 | Documentation | 2-3 | Both | N/A |
| **TOTAL** | | **23-35** | | **>90%** |

---

## Phase 0: Foundation (1-2 Days)

### Goal
Create crate structure and define transport trait (NO implementations yet).

### Critical Constraints
- ❌ NO mock implementation yet (Phase 1)
- ❌ NO TCP/TLS implementation (Phase 6)
- ✅ ONLY trait definitions

### Tasks

#### 0.1: Create Crate Structure
- [ ] Create `src/common/zznet-api/Cargo.toml`
  - Dependencies: `tokio`, `bytes`, `async-trait`, `thiserror`
  - Purpose: Transport trait definition ONLY

- [ ] Create `src/actors/zznet-hello/Cargo.toml`
  - Dependencies: `actix`, `tokio`, `bytes`, `serde`, `bincode`, `tracing`
  - Dev-dependency: `zznet-api`
  - Purpose: HELLO handler (transport boundary)

- [ ] Create `src/components/zznet-session/Cargo.toml`
  - Dependencies: `actix`, `tokio`, `serde`, `tracing`
  - **NO zznet-api dependency!**
  - Purpose: SessionManager (100% transport-agnostic)

- [ ] Create `src/components/zznet-router/Cargo.toml`
  - Dependencies: `actix`, `zznet-session`, `tracing`
  - Purpose: Room-based routing

- [ ] Create `src/components/zznet/Cargo.toml`
  - Dependencies: All above crates
  - Purpose: High-level integration API

#### 0.2: Define Shared Types
- [ ] Create `zznet-api/src/types.rs`:
  ```rust
  pub struct PeerId(String);
  pub struct RoomId(String);
  pub struct MessagePayload(Bytes);
  pub enum ProtocolVersion { V1 }
  pub struct PeerContext {
      pub peer_id: PeerId,
      pub remote_addr: String,
      pub connected_at: Instant,
      pub protocol_version: ProtocolVersion,
  }
  ```

#### 0.3: Define Transport Trait
- [ ] Create `zznet-api/src/transport.rs`:
  ```rust
  /// Abstract transport connection (bytes in/out)
  #[async_trait]
  pub trait TransportConnection: Send {
      /// Send framed message (impl handles framing)
      async fn send(&mut self, frame: Bytes) -> Result<(), TransportError>;

      /// Receive framed message (None = graceful close)
      async fn recv(&mut self) -> Result<Option<Bytes>, TransportError>;

      /// Peer address (for logging/metrics)
      fn peer_addr(&self) -> Option<String>;
  }

  #[async_trait]
  pub trait TransportServer: Send {
      async fn accept(&mut self) -> Result<Box<dyn TransportConnection>, TransportError>;
  }

  #[async_trait]
  pub trait TransportClient: Send {
      async fn connect(&self) -> Result<Box<dyn TransportConnection>, TransportError>;
  }
  ```

- [ ] Create `zznet-api/src/error.rs`:
  ```rust
  pub enum TransportError {
      IoError(String),
      ConnectionClosed,
      Timeout,
      FrameTooLarge { size: usize, limit: usize },
  }
  ```

- [ ] Document trait contracts:
  - Frame format: `[u32 BE length][payload]`
  - Frame size limit: 16 MiB
  - Heartbeat: zero-sized frames
  - Error handling: ConnectionClosed for graceful shutdown

#### 0.4: Stub Mock Module
- [ ] Create empty `zznet-api/src/mock.rs`
  - Comment: "Mock implementation - see Phase 1"
  - Will be filled in Phase 1

### Deliverables
- ✅ Five crate directories with `Cargo.toml`
- ✅ Transport trait definition (NO implementations)
- ✅ Shared type definitions
- ✅ Clear crate dependencies

### Success Criteria
- [ ] All crates compile independently
- [ ] No circular dependencies
- [ ] `zznet-session` does NOT depend on `zznet-api`
- [ ] All public types documented
- [ ] Transport trait contracts documented

### Architectural Validation
- ✅ Transport is pure abstraction
- ✅ Session layer structurally cannot depend on transport
- ✅ Foundation ready for mock-first development

---

## Phase 1: Mock Transport (2-3 Days)

### Goal
Create production-quality mock transport that will be the ONLY transport through Phase 5.

### Critical Constraints
- ❌ NO TCP/TLS implementation
- ✅ ONLY mock transport
- ✅ Mock MUST be comprehensive enough for full architecture development

### Philosophy
Mock transport is **not a placeholder** - it's a **first-class implementation** that must be:
- Feature-complete (framing, heartbeat, errors)
- Production-quality (well-tested, documented)
- Flexible (error injection, timing control)
- Fast (microsecond latency)

### Tasks

#### 1.1: Mock Connection Pair
- [ ] Implement `MockConnection`:
  ```rust
  pub struct MockConnection {
      tx: mpsc::Sender<MockFrame>,
      rx: mpsc::Receiver<MockFrame>,
      peer_id: String,
      inject_error: Arc<Mutex<Option<TransportError>>>,
  }

  enum MockFrame {
      Data(Bytes),
      Heartbeat,
      Close,
  }
  ```
- [ ] Implement `create_mock_pair() -> (MockConnection, MockConnection)`
- [ ] Bidirectional communication via tokio channels
- [ ] Test: Send/receive between pair

#### 1.2: Frame Protocol
- [ ] Implement frame encoding: `[u32 BE length][payload]`
- [ ] Implement frame decoding with partial frame handling
- [ ] Enforce 16 MiB size limit (fail with FrameTooLarge)
- [ ] Handle zero-sized frames (heartbeat)
- [ ] Test: Encode/decode round-trip
- [ ] Test: Reject frames >16 MiB

#### 1.3: TransportConnection Trait
- [ ] Implement `TransportConnection` for `MockConnection`:
  ```rust
  async fn send(&mut self, frame: Bytes) -> Result<(), TransportError> {
      // Check injected error
      // Encode frame
      // Send via channel
  }

  async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
      // Receive from channel (None = close)
      // Decode frame
      // Filter heartbeats
  }
  ```
- [ ] Test: Send/receive via trait
- [ ] Test: Connection close detected

#### 1.4: Mock Server/Client
- [ ] Implement `MockTransportServer`:
  ```rust
  pub struct MockTransportServer {
      accept_queue: mpsc::Receiver<Box<dyn TransportConnection>>,
  }
  ```
- [ ] Implement `MockTransportClient`:
  ```rust
  pub struct MockTransportClient {
      connection_tx: mpsc::Sender<Box<dyn TransportConnection>>,
      counter: Arc<AtomicU64>,
  }
  ```
- [ ] Function `create_mock_transport() -> (MockTransportServer, MockTransportClient)`
- [ ] Test: Client connects, server accepts
- [ ] Test: Multiple concurrent connections

#### 1.5: Heartbeat Simulation
- [ ] Add automatic heartbeat sender (spawn tokio task)
  - Send zero-sized frame every 100ms (fast for tests)
  - Cancel on connection close
- [ ] Add heartbeat timeout detection
  - Fail recv() if >500ms without frames
- [ ] Make timing configurable:
  ```rust
  pub struct MockTransportConfig {
      pub heartbeat_interval: Duration,  // Default: 100ms
      pub heartbeat_timeout: Duration,   // Default: 500ms
  }
  ```
- [ ] Test: Heartbeat keeps connection alive
- [ ] Test: Missing heartbeat triggers timeout

#### 1.6: Error Injection
- [ ] Add error injection:
  ```rust
  impl MockConnection {
      pub fn inject_error(&self, error: TransportError);
      pub fn close(&mut self);
  }
  ```
- [ ] Test: Injected errors returned correctly
- [ ] Test: Can simulate all error types

#### 1.7: Comprehensive Tests
- [ ] Basic send/receive
- [ ] Bidirectional communication
- [ ] Multiple messages in flight
- [ ] Connection close detection
- [ ] Heartbeat mechanism
- [ ] Frame size enforcement
- [ ] Error injection
- [ ] Multiple concurrent connections
- [ ] Property-based tests (proptest):
  - Frame encoding is bijective
  - Connection close always detected

### Deliverables
- ✅ Production-quality mock transport
- ✅ Complete frame protocol (16 MiB limit)
- ✅ Heartbeat mechanism (fast for tests)
- ✅ Error injection capabilities
- ✅ Mock server/client
- ✅ Test coverage >90%

### Success Criteria
- [ ] Mock implements all trait methods
- [ ] Can simulate all transport behaviors
- [ ] Tests run in microseconds
- [ ] Error injection works for all error types
- [ ] Test coverage >90%

### Architectural Validation
**Critical Proof:** If mock can implement all required behaviors, trait is well-designed. Gaps discovered here are cheap to fix (no TCP/TLS written yet).

---

## Phase 2: Session Layer (3-4 Days)

### Goal
Implement SessionManager that handles typed messages, completely independent of transport.

### Critical Constraints
- ❌ NO dependency on `zznet-api` (not even the trait!)
- ❌ NO knowledge of bytes, frames, or serialization
- ❌ NO network, sockets, or I/O
- ✅ ONLY typed `AppMessage` via channels
- ✅ 100% testable with mock channels (no transport needed)

### Philosophy
SessionManager is the **core of transport-agnostic design**:
- Lives entirely in typed message space
- No awareness of transport boundary
- HELLO handler (Phase 3) will bridge transport → session
- Fully testable without any transport code

### Tasks

#### 2.1: Application Messages (Typed, No Bytes!)
- [ ] Create `zznet-session/src/messages.rs`:
  ```rust
  /// Application-level messages (Protocol B)
  /// SessionManager NEVER serializes these - that's HELLO handler's job!
  #[derive(Debug, Clone)]
  pub enum AppMessage {
      JoinRoom { room_id: RoomId },
      LeaveRoom { room_id: RoomId },
      RoomMessage { room_id: RoomId, payload: MessagePayload },
      DirectMessage { to_peer: PeerId, payload: MessagePayload },
  }
  ```
- [ ] **No serialization here!** SessionManager only works with typed messages
- [ ] Document: "SessionManager never touches bytes"

#### 2.2: Session Handle (RAII)
- [ ] Create `zznet-session/src/handle.rs`:
  ```rust
  pub struct SessionHandle {
      peer_id: PeerId,
      peer_context: PeerContext,
      outbound_tx: mpsc::Sender<AppMessage>,
      session_addr: Addr<SessionActor>,
  }

  impl Drop for SessionHandle {
      fn drop(&mut self) {
          // Send SessionClosed to router
          // GUARANTEE: Always called, even on panic
      }
  }

  impl SessionHandle {
      pub fn send_message(&self, msg: AppMessage) -> Result<()> {
          // Send via channel, handle backpressure
      }
  }
  ```
- [ ] Test: Drop always sends SessionClosed
- [ ] Test: Panic in actor still calls Drop

#### 2.3: Session Actor
- [ ] Create `zznet-session/src/session.rs`:
  ```rust
  pub struct SessionActor {
      peer_id: PeerId,
      peer_context: PeerContext,
      outbound_rx: mpsc::Receiver<AppMessage>,
      inbound_tx: mpsc::Sender<AppMessage>,
      router: Recipient<SessionEvent>,
  }
  ```
- [ ] **No transport reference!** All I/O via channels
- [ ] Forward outbound messages from channel
- [ ] Forward inbound messages to router
- [ ] Test: Send/receive via channels

#### 2.4: Session Events
- [ ] Create `zznet-session/src/events.rs`:
  ```rust
  #[derive(Message)]
  #[rtype(result = "()")]
  pub enum SessionEvent {
      PeerConnected {
          peer_id: PeerId,
          context: PeerContext,
          handle: SessionHandle,
      },
      PeerDisconnected {
          peer_id: PeerId,
      },
      MessageReceived {
          from_peer: PeerId,
          message: AppMessage,
      },
  }
  ```
- [ ] Sent to router
- [ ] Document ordering guarantees

#### 2.5: Session Factory
- [ ] Create `zznet-session/src/factory.rs`:
  ```rust
  pub struct SessionFactory {
      router: Recipient<SessionEvent>,
  }

  pub struct SessionChannels {
      pub inbound_tx: mpsc::Sender<AppMessage>,
      pub outbound_rx: mpsc::Receiver<AppMessage>,
  }

  impl SessionFactory {
      pub fn create_session(
          &self,
          peer_id: PeerId,
          peer_context: PeerContext,
      ) -> (SessionActor, SessionHandle, SessionChannels) {
          // Create actor, handle, channels
          // Return channels for HELLO handler to wire
      }
  }
  ```
- [ ] Factory creates bounded channels (capacity: 100)
- [ ] Test: Factory creates working sessions

#### 2.6: Unit Tests (No Transport!)
- [ ] Test: Create two sessions connected via channels
  - **Not using transport - using channels!**
- [ ] Test: Session A sends AppMessage to Session B
- [ ] Test: PeerConnected event sent to router
- [ ] Test: PeerDisconnected event sent on Drop
- [ ] Test: Backpressure when channel full
- [ ] Test: Panic still triggers Drop
- [ ] **Critical:** Zero transport dependency

### Deliverables
- ✅ Transport-agnostic SessionManager
- ✅ RAII SessionHandle with guaranteed cleanup
- ✅ Typed messages (no serialization!)
- ✅ Session factory
- ✅ Test coverage >90%

### Success Criteria
- [ ] Sessions work with channels only
- [ ] `zznet-session` does NOT depend on `zznet-api`
- [ ] SessionHandle Drop always triggers cleanup
- [ ] Test coverage >90%
- [ ] Tests run in microseconds

### Architectural Validation
**Critical Proof:** SessionManager is truly transport-agnostic:
- ✅ No bytes, frames, or network
- ✅ Only typed messages
- ✅ Fully testable without transport
- ✅ HELLO handler (next) will bridge transport → session

---

## Phase 3: HELLO Handler (2-3 Days)

### Goal
Implement HELLO handshake that bridges transport boundary to session layer.

### Critical Constraints
- ❌ ONLY use mock transport (from Phase 1)
- ❌ NO TCP/TLS until Phase 6
- ✅ ONLY mock transport tests

### Philosophy
HELLO handler is the **transport boundary layer**:
- **Below:** Bytes, frames, serialization (Protocol A)
- **Above:** Typed messages, SessionManager (Protocol B)

Three jobs:
1. HELLO handshake - exchange peer IDs, validate version
2. Serialization bridge - convert bytes ↔ typed messages
3. Lifecycle management - wire transport to session

### Tasks

#### 3.1: HELLO Protocol Messages (Protocol A)
- [ ] Create `zznet-hello/src/messages.rs`:
  ```rust
  #[derive(Serialize, Deserialize, Debug)]
  pub enum HelloMessage {
      Hello {
          version: ProtocolVersion,
          peer_id: PeerId,
      },
      HelloAck {
          version: ProtocolVersion,
      },
      HelloError {
          reason: String,
      },
  }

  impl HelloMessage {
      pub fn to_bytes(&self) -> Result<Bytes, HelloError> {
          bincode::serialize(self)
              .map(Bytes::from)
              .map_err(|e| HelloError::Serialization(e.to_string()))
      }

      pub fn from_bytes(bytes: &[u8]) -> Result<Self, HelloError> {
          bincode::deserialize(bytes)
              .map_err(|e| HelloError::Serialization(e.to_string()))
      }
  }
  ```
- [ ] Test: Serialize/deserialize HELLO messages
- [ ] Document: "Only HELLO touches bytes - AppMessages never do!"

#### 3.2: PeerSession (Serialization Bridge)
- [ ] Create `zznet-hello/src/peer_session.rs`:
  ```rust
  /// PeerSession bridges transport (bytes) ↔ session (typed messages)
  pub struct PeerSession {
      peer_id: PeerId,
      transport: Box<dyn TransportConnection>,
      session_channels: SessionChannels,
  }

  impl PeerSession {
      pub fn spawn(self) -> JoinHandle<()> {
          tokio::spawn(async move {
              // Spawn two tasks:
              // 1. transport → session (recv bytes, deserialize, send to inbound_tx)
              // 2. session → transport (recv from outbound_rx, serialize, send bytes)
              let send_handle = self.spawn_send_loop();
              let recv_handle = self.spawn_recv_loop();

              tokio::select! {
                  _ = send_handle => {},
                  _ = recv_handle => {},
              }
          })
      }

      async fn spawn_recv_loop(&self) {
          loop {
              // Read bytes from transport
              let bytes = self.transport.recv().await?;
              // Deserialize to AppMessage
              let msg: AppMessage = bincode::deserialize(&bytes)?;
              // Send to session inbound
              self.session_channels.inbound_tx.send(msg).await?;
          }
      }

      async fn spawn_send_loop(&self) {
          while let Some(msg) = self.session_channels.outbound_rx.recv().await {
              // Read AppMessage from session outbound
              // Serialize to bytes
              let bytes = bincode::serialize(&msg)?;
              // Send to transport
              self.transport.send(Bytes::from(bytes)).await?;
          }
      }
  }
  ```
- [ ] PeerSession handles ALL serialization
- [ ] SessionManager never sees bytes
- [ ] Test: Messages flow through PeerSession (mock transport)
- [ ] Test: Serialization errors close connection

#### 3.3: HELLO Actor (Server)
- [ ] Create `zznet-hello/src/server.rs`:
  ```rust
  pub struct HelloServerActor {
      transport: Box<dyn TransportConnection>,
      session_factory: Arc<SessionFactory>,
      state: HelloState,
      timeout: Duration,
  }

  enum HelloState {
      AwaitingHello,
      Completed,
      Failed,
  }
  ```
- [ ] On start: Set 30-second timeout
- [ ] Receive bytes from transport
- [ ] Deserialize to HelloMessage::Hello
- [ ] Validate protocol version
- [ ] Extract PeerId
- [ ] Send HelloAck
- [ ] Create SessionActor via factory
- [ ] Spawn PeerSession to bridge transport → session
- [ ] Stop (HELLO complete, PeerSession takes over)
- [ ] Test: Server HELLO with mock transport
- [ ] Test: Timeout if no HELLO received
- [ ] Test: Version mismatch sends HelloError

#### 3.4: HELLO Actor (Client)
- [ ] Create `zznet-hello/src/client.rs`:
  ```rust
  pub struct HelloClientActor {
      transport: Box<dyn TransportConnection>,
      peer_id: PeerId,
      session_factory: Arc<SessionFactory>,
      state: HelloState,
      timeout: Duration,
  }
  ```
- [ ] On start: Send Hello immediately
- [ ] Wait for HelloAck
- [ ] Handle HelloError
- [ ] Create SessionActor via factory
- [ ] Spawn PeerSession
- [ ] Stop (HELLO complete)
- [ ] Test: Client HELLO with mock transport
- [ ] Test: Timeout if no HelloAck
- [ ] Test: HelloError handled

#### 3.5: Integration Tests (Mock Only)
- [ ] Test: Full HELLO handshake (client ↔ server, mock transport)
- [ ] Test: Messages flow after HELLO
- [ ] Test: HELLO timeout
- [ ] Test: Version mismatch
- [ ] Test: Transport error during HELLO
- [ ] Test: Multiple concurrent HELLO handshakes
- [ ] **All tests use mock transport - NO network I/O**

### Deliverables
- ✅ HELLO handshake protocol
- ✅ PeerSession serialization bridge
- ✅ Server and client HELLO actors
- ✅ Test coverage >85%

### Success Criteria
- [ ] HELLO completes successfully with mock transport
- [ ] PeerConnected event sent after HELLO
- [ ] Messages flow through PeerSession
- [ ] Timeout handled correctly
- [ ] Version mismatch detected
- [ ] Test coverage >85%
- [ ] All tests use mock transport

### Architectural Validation
**Critical Proof:** Transport boundary properly abstracted:
- ✅ HELLO handler is the ONLY component touching bytes
- ✅ SessionManager remains 100% agnostic
- ✅ PeerSession bridges transport → session
- ✅ All works with mock transport

---

## Phase 4: Router Layer (3-4 Days)

### Goal
Implement room-based message routing with connection tracking.

### Critical Constraints
- ❌ ONLY mock transport through HELLO → session
- ✅ Router only knows typed messages

### Tasks

#### 4.1: Router State
- [ ] Create `zznet-router/src/router.rs`:
  ```rust
  pub struct Router {
      rooms: HashMap<RoomId, Room>,
      sessions: HashMap<PeerId, SessionHandle>,
      app_handler: Box<dyn AppHandler>,
  }

  struct Room {
      room_id: RoomId,
      members: HashSet<PeerId>,
  }
  ```
- [ ] Router stores SessionHandle (RAII)

#### 4.2: Session Registration
- [ ] Implement `SessionEvent::PeerConnected` handler:
  ```rust
  SessionEvent::PeerConnected { peer_id, context, handle } => {
      self.sessions.insert(peer_id.clone(), handle);
      self.app_handler.on_peer_connected(context);
  }
  ```
- [ ] Test: Session registered after HELLO

#### 4.3: Session Unregistration
- [ ] Implement `SessionEvent::PeerDisconnected` handler:
  ```rust
  SessionEvent::PeerDisconnected { peer_id } => {
      self.sessions.remove(&peer_id);
      // Remove from all rooms
      for room in self.rooms.values_mut() {
          room.members.remove(&peer_id);
      }
      self.app_handler.on_peer_disconnected(peer_id);
  }
  ```
- [ ] Test: Session cleanup on close

#### 4.4: Room Management
- [ ] Implement JoinRoom message:
  ```rust
  AppMessage::JoinRoom { room_id } => {
      let room = self.rooms.entry(room_id.clone())
          .or_insert_with(|| Room::new(room_id));
      room.members.insert(from_peer);
  }
  ```
- [ ] Implement LeaveRoom message
- [ ] Delete empty rooms
- [ ] Test: Join/leave rooms

#### 4.5: Message Routing
- [ ] Implement RoomMessage routing:
  ```rust
  AppMessage::RoomMessage { room_id, payload } => {
      if let Some(room) = self.rooms.get(&room_id) {
          if !room.members.contains(&from_peer) {
              return; // Not authorized
          }
          for peer_id in &room.members {
              if peer_id != &from_peer {
                  if let Some(handle) = self.sessions.get(peer_id) {
                      let _ = handle.send_message(msg.clone());
                  }
              }
          }
      }
  }
  ```
- [ ] Verify sender in room (authorization)
- [ ] Route to all members except sender
- [ ] Test: Messages routed to room members
- [ ] Test: Non-member cannot send

#### 4.6: Direct Messaging
- [ ] Implement DirectMessage routing:
  ```rust
  AppMessage::DirectMessage { to_peer, payload } => {
      if let Some(handle) = self.sessions.get(&to_peer) {
          let _ = handle.send_message(msg);
      }
  }
  ```
- [ ] Test: Direct message delivered
- [ ] Test: Unknown peer handled

#### 4.7: Application Handler Trait
- [ ] Define `AppHandler`:
  ```rust
  pub trait AppHandler: Send + 'static {
      fn on_peer_connected(&mut self, ctx: PeerContext);
      fn on_peer_disconnected(&mut self, peer_id: PeerId);
      fn on_message(&mut self, from: PeerId, room: RoomId, payload: MessagePayload);
      fn on_direct_message(&mut self, from: PeerId, to: PeerId, payload: MessagePayload);
  }
  ```
- [ ] Document trait contract

#### 4.8: Integration Tests (Mock Only)
- [ ] Test: Two sessions join same room (via mock)
- [ ] Test: Messages routed between room members
- [ ] Test: Authorization enforced
- [ ] Test: Session cleanup removes from rooms
- [ ] **All tests use mock transport**

### Deliverables
- ✅ Router with room management
- ✅ Message routing
- ✅ Authorization checks
- ✅ AppHandler trait
- ✅ Test coverage >85%

### Success Criteria
- [ ] Rooms tracked accurately
- [ ] Messages routed correctly
- [ ] Authorization enforced
- [ ] Test coverage >85%
- [ ] All tests use mock transport

### Architectural Validation
**Critical Proof:** Router works with typed messages only:
- ✅ No knowledge of transport
- ✅ No knowledge of serialization
- ✅ Only typed AppMessages
- ✅ All works with mock transport

---

## Phase 5: Application Integration (3-4 Days)

### Goal
Create high-level API and integrate with zzping-collector/database.

### Critical Constraints
- ❌ ONLY mock transport
- ✅ Full functionality achieved without real transport

### Tasks

#### 5.1: Server Builder
- [ ] Create `zznet/src/server.rs`:
  ```rust
  pub struct ServerBuilder {
      transport_config: TransportConfig,
      app_handler: Option<Box<dyn AppHandler>>,
  }

  impl ServerBuilder {
      pub fn with_mock_transport(bind_id: String) -> Self;
      // TCP/TLS methods added in Phase 6
      pub fn with_handler<H: AppHandler>(self, handler: H) -> Self;
      pub fn build(self) -> Result<Server, ConfigError>;
  }

  pub struct Server {
      router: Addr<Router>,
      acceptor: Addr<TransportAcceptor>,
  }

  impl Server {
      pub async fn run(self) -> Result<(), ServerError>;
  }
  ```
- [ ] Validate configuration
- [ ] Wire components together
- [ ] Test: Server with mock transport

#### 5.2: Client Builder
- [ ] Create `zznet/src/client.rs`:
  ```rust
  pub struct ClientBuilder {
      transport_config: TransportConfig,
      peer_id: PeerId,
  }

  impl ClientBuilder {
      pub fn with_mock_transport(peer_id: PeerId, server_id: String) -> Self;
      // TCP/TLS methods added in Phase 6
      pub async fn connect(self) -> Result<ClientHandle, ClientError>;
  }

  pub struct ClientHandle {
      session_handle: SessionHandle,
  }

  impl ClientHandle {
      pub fn join_room(&self, room_id: RoomId) -> Result<()>;
      pub fn leave_room(&self, room_id: RoomId) -> Result<()>;
      pub fn send_to_room(&self, room_id: RoomId, payload: MessagePayload) -> Result<()>;
      pub fn send_direct(&self, to_peer: PeerId, payload: MessagePayload) -> Result<()>;
  }
  ```
- [ ] Test: Client with mock transport

#### 5.3: Transport Acceptor
- [ ] Create `zznet/src/acceptor.rs`:
  ```rust
  pub struct TransportAcceptor {
      transport_server: Box<dyn TransportServer>,
      session_factory: Arc<SessionFactory>,
  }
  ```
- [ ] Accept connections loop
- [ ] Spawn HELLO handler for each connection
- [ ] Test: Accept multiple connections (mock)

#### 5.4: Integration with zzping-collector
- [ ] Create AppHandler adapter for collector
- [ ] Handle ping result messages
- [ ] Route to collector actors
- [ ] Test: Collector with mock transport

#### 5.5: Integration with zzping-database
- [ ] Create AppHandler adapter for database
- [ ] Handle query messages
- [ ] Send responses
- [ ] Test: Database with mock transport

#### 5.6: End-to-End Tests (Mock Only)
- [ ] Test: Collector connects to database (mock)
- [ ] Test: Collector sends ping results
- [ ] Test: Database queries collector
- [ ] Test: Multiple collectors
- [ ] Test: Connection recovery
- [ ] **All tests use mock transport - NO network I/O**

### Deliverables
- ✅ Server and client builders
- ✅ Transport acceptor
- ✅ Collector/database integration
- ✅ End-to-end tests with mock
- ✅ Test coverage >85%

### Success Criteria
- [ ] Server starts with mock transport
- [ ] Client connects with mock transport
- [ ] Collector/database integration works
- [ ] End-to-end tests pass
- [ ] Test coverage >85%
- [ ] **All tests use mock transport**

### Architectural Validation
**Critical Proof:** Full application functionality without real transport:
- ✅ Complete integration
- ✅ Collector and database working
- ✅ Zero network I/O
- ✅ All tests in microseconds

---

## MILESTONE: Mock-Complete Architecture

### Deliverables
After Phase 5, we have:
- ✅ Complete network layer implementation
- ✅ Full application integration (collector/database)
- ✅ Near-perfect test coverage (>95%)
- ✅ **Zero network I/O in entire test suite**

### Success Criteria
- [ ] Test coverage >95%
- [ ] All tests run in seconds (not minutes)
- [ ] Zero network I/O in any test
- [ ] Collector ↔ database communication works (via mock)
- [ ] All architectural invariants validated
- [ ] Can run entire test suite on airplane

### What This Proves

**The Architecture is Correct:**
- If full functionality works with mock, abstraction is sound
- If tests achieve >95% coverage, code is well-exercised
- If tests run in seconds, development is efficient

**We Can Now Add Real Transport:**
- Real transport is "just another implementation"
- If TCP/TLS is hard to add, abstraction leaked (but we know immediately)
- If TCP/TLS works immediately, abstraction was perfect

**This is the Critical Validation:**
> **"What better way to validate transport-agnostic design than just... not writing the transport part!"**

The fact that we've reached this point WITH ONLY MOCK TRANSPORT proves the architecture is fundamentally sound.

---

## Phase 6: Real Transport (Deferred) (4-5 Days)

### Goal
Implement TCP/TLS transport as validation that abstraction works.

### Philosophy
This phase is **validation**, not **development**:
- Architecture already proven (Phases 0-5)
- TCP/TLS should be straightforward (just implement trait)
- If it's hard, abstraction leaked (we fix it)
- If it's easy, abstraction was perfect

### Tasks

#### 6.1: TCP Connection Wrapper
- [ ] Add dependencies to `zznet-api/Cargo.toml`:
  - `rustls = "0.23"`
  - `tokio-rustls = "0.26"`
  - `rustls-pemfile = "2"`

- [ ] Create `zznet-api/src/tcp_tls/connection.rs`:
  ```rust
  pub struct TcpTlsConnection {
      stream: TlsStream<TcpStream>,
      read_buf: BytesMut,
      peer_addr: SocketAddr,
  }
  ```
- [ ] Implement `TransportConnection` trait
- [ ] Frame encoding: `[u32 BE length][payload]`
- [ ] Frame decoding with 16 MiB limit
- [ ] Handle partial reads/writes
- [ ] Test: Send/receive over localhost

#### 6.2: TLS Server
- [ ] Create `zznet-api/src/tcp_tls/server.rs`:
  ```rust
  pub struct TcpTlsServer {
      listener: TcpListener,
      tls_acceptor: TlsAcceptor,
  }
  ```
- [ ] Implement `TransportServer` trait
- [ ] Load certificates (PEM)
- [ ] TLS handshake on accept
- [ ] Test: Accept connections
- [ ] Test: TLS handshake

#### 6.3: TLS Client
- [ ] Create `zznet-api/src/tcp_tls/client.rs`:
  ```rust
  pub struct TcpTlsClient {
      server_addr: SocketAddr,
      tls_connector: TlsConnector,
      server_name: ServerName,
  }
  ```
- [ ] Implement `TransportClient` trait
- [ ] Load client certificates
- [ ] TLS handshake on connect
- [ ] Test: Connect to server
- [ ] Test: TLS handshake

#### 6.4: Heartbeat Mechanism
- [ ] Spawn heartbeat sender task
  - Send zero-sized frame every 1 second
  - Cancel on connection close
- [ ] Implement heartbeat timeout detection
  - Close if >5 seconds without frames
- [ ] Test: Heartbeat keeps connection alive
- [ ] Test: Missing heartbeat closes connection

#### 6.5: Configuration
- [ ] Create `zznet-api/src/tcp_tls/config.rs`:
  ```rust
  pub struct TcpTlsServerConfig {
      pub bind_addr: SocketAddr,
      pub cert_path: PathBuf,
      pub key_path: PathBuf,
      pub ca_cert_path: Option<PathBuf>,
  }

  pub struct TcpTlsClientConfig {
      pub server_addr: SocketAddr,
      pub server_name: String,
      pub cert_path: PathBuf,
      pub key_path: PathBuf,
      pub ca_cert_path: PathBuf,
  }
  ```
- [ ] Implement validation (panic on invalid)
- [ ] Check certificate files exist
- [ ] Test: Config validation

#### 6.6: Integration Tests (Real Transport)
- [ ] Test: Client connects to server (TCP/TLS)
- [ ] Test: Messages sent/received
- [ ] Test: Multiple connections
- [ ] Test: Connection close detected
- [ ] Test: Heartbeat mechanism
- [ ] Test: With real TLS certificates

#### 6.7: Update Builders for TCP/TLS
- [ ] Add to `ServerBuilder`:
  ```rust
  pub fn with_tcp_tls(self, config: TcpTlsServerConfig) -> Self;
  ```
- [ ] Add to `ClientBuilder`:
  ```rust
  pub fn with_tcp_tls(self, config: TcpTlsClientConfig) -> Self;
  ```
- [ ] Test: Server/client with TCP/TLS

### Deliverables
- ✅ TCP/TLS transport implementation
- ✅ Frame protocol (16 MiB limit)
- ✅ TLS-secured connections
- ✅ Heartbeat mechanism
- ✅ Configuration with validation
- ✅ Integration tests
- ✅ Test coverage >80%

### Success Criteria
- [ ] TCP/TLS implements trait correctly
- [ ] All existing tests still pass (with mock)
- [ ] New tests pass (with TCP/TLS)
- [ ] Can establish TLS connections
- [ ] Heartbeat keeps connections alive
- [ ] Test coverage >80%
- [ ] **TCP/TLS was easy to add** (proves abstraction works)

### Architectural Validation
**Critical Proof:** If TCP/TLS was straightforward to add, abstraction was perfect. If it was difficult, we found leaky abstractions (and can fix them).

---

## Phase 7: Hardening (3-5 Days)

### Goal
Comprehensive testing, stress testing, and edge case handling.

### Tasks

#### 7.1: Unit Test Completion
- [ ] Ensure >90% coverage for all modules
- [ ] Test all edge cases
- [ ] Test all error paths
- [ ] Property-based tests (proptest)

#### 7.2: Integration Tests
- [ ] Full client-server scenarios (both mock and TCP/TLS)
- [ ] Multiple clients connecting
- [ ] Room membership
- [ ] Connection drops
- [ ] HELLO failures

#### 7.3: Stress Testing
- [ ] Load test: 100 concurrent connections
- [ ] Load test: 1000 messages/second
- [ ] Test backpressure under load
- [ ] Memory profiling (check for leaks)
- [ ] CPU profiling (check for hotspots)

#### 7.4: Failure Mode Testing
- [ ] Network failures (connection drops, TCP RST, TLS handshake failure)
- [ ] Application errors (handler panics, handler blocks)
- [ ] Resource exhaustion (channel saturation, mailbox full)

#### 7.5: Invariant Validation
- [ ] Write tests for all 20 architectural invariants
- [ ] Test RAII guarantee with panic injection
- [ ] Test ordering guarantees

#### 7.6: Security Testing
- [ ] TLS certificate validation
- [ ] Peer authentication
- [ ] Authorization checks
- [ ] Message size enforcement
- [ ] DoS vulnerabilities

### Deliverables
- ✅ Comprehensive test suite
- ✅ Stress test results
- ✅ Failure mode tests
- ✅ Security audit
- ✅ Performance benchmarks
- ✅ Test coverage >90%

### Success Criteria
- [ ] Test coverage >90%
- [ ] All invariants validated
- [ ] Stress tests pass
- [ ] Failure modes handled
- [ ] No security vulnerabilities
- [ ] Performance meets requirements

---

## Phase 8: Documentation (2-3 Days)

### Goal
Complete documentation and examples.

### Tasks

#### 8.1: API Documentation
- [ ] Complete rustdoc for all public items
- [ ] Module-level documentation
- [ ] Document invariants/guarantees
- [ ] Add examples to functions
- [ ] Run `cargo doc` and review

#### 8.2: User Guide
- [ ] Create `ZZNET_USER_GUIDE.md`:
  - Architecture overview
  - How to implement AppHandler
  - Server/client creation
  - Room management
  - Error handling
  - Performance tuning
  - Troubleshooting

#### 8.3: Example Applications
- [ ] `examples/echo_server.rs` - Simple echo server
- [ ] `examples/echo_client.rs` - Client sending messages
- [ ] `examples/chat_room.rs` - Multi-client chat
- [ ] Test all examples

#### 8.4: Migration Guide
- [ ] Create `MIGRATION_FROM_ZZNET_CONNECTION.md`:
  - Differences from old implementation
  - Migration examples
  - Breaking changes
  - Architectural improvements

#### 8.5: Architecture Diagrams
- [ ] Connection establishment flow
- [ ] HELLO handshake flow
- [ ] Message routing flow
- [ ] Connection teardown flow
- [ ] Component relationships

#### 8.6: Performance Guide
- [ ] Performance characteristics
- [ ] Tuning recommendations
- [ ] Monitoring/metrics

### Deliverables
- ✅ Complete API documentation
- ✅ User guide
- ✅ Example applications
- ✅ Migration guide
- ✅ Architecture diagrams
- ✅ Performance guide

### Success Criteria
- [ ] All public APIs documented
- [ ] Examples compile and run
- [ ] Documentation reviewed
- [ ] No broken links

---

## Timeline & Dependencies

### Dependency Graph

```
Phase 0 (Foundation)
    ↓
Phase 1 (Mock Transport)
    ↓
    ├──────────────────┬──────────────────┐
    ↓                  ↓                  ↓
Phase 2 (Session)  Phase 3 (HELLO)    Phase 4 (Router)
    └──────────────────┴──────────────────┘
                       ↓
               Phase 5 (Integration)
                       ↓
       ╔═══════════════════════════════╗
       ║  MILESTONE: MOCK-COMPLETE     ║
       ║  >95% Coverage, Zero Network  ║
       ╚═══════════════════════════════╝
                       ↓
           Phase 6 (TCP/TLS - DEFERRED)
                       ↓
           Phase 7 (Hardening)
                       ↓
           Phase 8 (Documentation)
```

### Timeline Summary

| Phase | Days | Cumulative | Critical? |
|-------|------|------------|-----------|
| 0. Foundation | 1-2 | 1-2 | ✅ |
| 1. Mock Transport | 2-3 | 3-5 | ✅ |
| 2. Session Layer | 3-4 | 6-9 | ✅ |
| 3. HELLO Handler | 2-3 | 8-12 | ✅ |
| 4. Router Layer | 3-4 | 11-16 | ✅ |
| 5. Integration | 3-4 | 14-20 | ✅ |
| **MILESTONE** | | **14-20** | |
| 6. TCP/TLS | 4-5 | 18-25 | |
| 7. Hardening | 3-5 | 21-30 | |
| 8. Documentation | 2-3 | 23-33 | |
| **TOTAL** | | **23-33** | |

### Critical Path

**Phases 0-5 (Mock-Complete):** Every phase is critical. This path proves the architecture.

**Phase 6 (TCP/TLS):** Important but not critical. Can be done independently.

**Phases 7-8:** Important but parallelizable.

### Parallelization Opportunities

**Before Mock-Complete Milestone:**
- Limited parallelization (strict dependencies)
- Focus on sequential, correct implementation

**After Mock-Complete Milestone:**
- Phase 6 (TCP/TLS) can be done by one developer
- Phase 7 (Testing) can be done by another
- Phase 8 (Docs) can be done by a third

---

## Risk Assessment

### High-Risk Items (Before Milestone)

#### 1. Mock Transport Insufficiency
- **Risk:** Mock might not cover all behaviors needed
- **Mitigation:**
  - Design mock comprehensively (Phase 1)
  - Discover gaps early (Phases 2-5)
  - Cheap to fix (no TCP/TLS written yet)
- **Impact if realized:** Delays Phases 2-5, but caught early

#### 2. Transport Abstraction Leaks
- **Risk:** Session layer might need transport knowledge
- **Mitigation:**
  - Strict crate separation (Phase 0)
  - `zznet-session` cannot depend on `zznet-api`
  - Discover leaks during Phases 2-5
- **Impact if realized:** Design fixes needed, but before TCP/TLS

#### 3. Channel-Based Design Issues
- **Risk:** Bridging transport → session via channels might have issues
- **Mitigation:**
  - Prototype in Phase 3
  - Test thoroughly with mock
  - Profile if needed
- **Impact if realized:** Redesign PeerSession, but before TCP/TLS

### Medium-Risk Items (After Milestone)

#### 4. TCP/TLS Implementation Difficulty
- **Risk:** TCP/TLS might be hard to implement
- **Mitigation:**
  - **This is actually a feature, not a bug!**
  - If TCP/TLS is hard, abstraction leaked
  - We discover issues when it matters least
- **Impact if realized:** Fix abstraction, re-run Phases 0-5

#### 5. Performance Issues
- **Risk:** Mock-based design might not perform well
- **Mitigation:**
  - Profile in Phase 7
  - Optimize hot paths
  - Zero-copy where possible
- **Impact if realized:** Performance optimization needed

### Low-Risk Items

#### 6. Documentation Lag
- **Risk:** Docs might lag implementation
- **Mitigation:**
  - Write docs alongside code
  - Phase 8 catches up
- **Impact if realized:** Delays release slightly

---

## Conclusion

This implementation plan represents a **fundamental shift** in how we approach network layer development:

### Traditional Approach (What We're NOT Doing)
1. Implement TCP/TLS first
2. Build session layer on top
3. Hope abstraction is sufficient
4. Tests depend on network I/O
5. Discover abstraction leaks late

### Mock-First Approach (What We ARE Doing)
1. Define abstraction (trait)
2. Implement comprehensive mock
3. **Build entire architecture with ONLY mock**
4. **Achieve >95% coverage without network**
5. **THEN implement real transport**

### Why This Works

**Early Validation:**
- Architecture proven before expensive work
- Leaky abstractions found early
- Changes are cheap (no TCP/TLS written)

**Development Speed:**
- Tests run in microseconds
- No network flakiness
- Develop anywhere
- Parallel work easier

**Confidence:**
- Mock-complete milestone is huge
- >95% coverage without network
- If TCP/TLS is easy to add, we succeeded
- If TCP/TLS is hard, we learn immediately

### The Critical Proof

After Phase 5, we will have:
- ✅ Complete network layer
- ✅ Full application integration
- ✅ >95% test coverage
- ✅ **Zero network I/O**

This proves:
> **The architecture is fundamentally sound. Real transport is just validation.**

### Next Steps

1. Review this plan
2. Begin Phase 0: Create crate structure
3. Implement Phase 1: Comprehensive mock transport
4. **Resist temptation to implement TCP/TLS early!**
5. Trust the process: Mock-first through Phase 5
6. Celebrate mock-complete milestone
7. Add TCP/TLS as validation (Phase 6)

**Timeline:** 23-35 days total, with critical milestone at 14-20 days.

**Success Metric:** If we reach mock-complete milestone with >95% coverage and zero network I/O, the architecture is proven correct. Everything after that is validation and polish.

---

*This plan is a testament to the power of abstraction and the importance of validating architectural decisions early. By deliberately deferring real transport, we create a forcing function that ensures our design is truly transport-agnostic.*
