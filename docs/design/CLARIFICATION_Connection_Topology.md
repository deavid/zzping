# Clarification: Connection Topology

**Date**: 2025-10-06
**Purpose**: Document the simple connection topology for ZZPing architecture

---

## The Simple Truth

**Database = TCP Server (passive, accepts connections)**
**Collector = TCP Client (active, connects to database)**
**GUI/CLI = TCP Client (active, connects to database)**

---

## Connection Model

### Database (Server Side)

```
┌─────────────────────────────┐
│   zzping-database process   │
│                             │
│   Binds to: 0.0.0.0:9001    │  ← ONE listening socket
│                             │
│   Accepts connections from: │
│   - Multiple collectors     │
│   - Multiple GUI clients    │
│   - Multiple CLI clients    │
└─────────────────────────────┘
        ↑    ↑    ↑
        │    │    │
     TCP Connections (many)
```

**Key Points**:
- Database has **ONE listening TCP socket**
- Database is **passive** - it waits for incoming connections
- Database can accept **many connections** simultaneously
- Each connection is independent (different collectors, clients)

### Collector (Client Side)

```
┌─────────────────────────────┐
│  zzping-collector process   │
│                             │
│  Opens: ONE TCP connection  │  ← Single connection to database
│  Target: database:9001      │
│                             │
│  Auto-reconnect on failure  │
└─────────────────────────────┘
        │
        ↓
     TCP Connection
        │
        ↓
┌─────────────────────────────┐
│      zzping-database        │
└─────────────────────────────┘
```

**Key Points**:
- Collector has **ONE connection** to the database
- Collector is **active** - it initiates the connection
- Collector has **auto-reconnect logic** - maintains connection health
- If connection fails, collector reconnects automatically

### GUI/CLI (Client Side)

Same as collector: **ONE connection per app instance** to the database.

---

## Connection Ownership

### Who Owns the Connection?

**Answer**: The app's `main()` function (or its network setup code).

**Pattern**:
```rust
// In main() or network setup
let session_manager = ConnectionManager::<MessageType>::new(rooms).start();

// For database (server)
ServerBuilder::new()
    .bind("0.0.0.0:9001")
    .with_connection_manager(session_manager.clone())
    .start()
    .await?;

// For collector (client)
ClientBuilder::new()
    .connect_to("database:9001")
    .with_connection_manager(session_manager.clone())
    .connect()
    .await?;
```

The `ServerBuilder` or `ClientBuilder` manages the connection lifecycle.

---

## Component Visibility

### Do Components Know About Connections?

**Answer**: No. Components are **connection-agnostic**.

**What Components See**:
- Components receive a `SessionManager`
- Components call `session_manager.send_to_room(room_id, message)`
- Components receive messages via their actor mailbox
- Components have **no idea** if connection is TCP, Unix socket, mock, etc.

**What Components Don't See**:
- TCP sockets
- Connection state (connected/disconnected)
- Reconnection logic
- Transport-level errors

**Example**:
```rust
// Component code (connection-agnostic)
fn handle_config_change(&mut self, config: Config) {
    // Send to network - component doesn't know if connected!
    self.session_manager.send_to_room(
        RoomId::from("intent-config"),
        IntentConfigMessage::ConfigUpdate { ... }
    );
}
```

---

## Multiplexing: One Connection, Many Rooms

### All Rooms Over One Connection

```
Collector Process                Database Process
┌──────────────────┐            ┌──────────────────┐
│                  │            │                  │
│  MemDB ──────────┼───room1────┤────── MemDB      │
│                  │            │                  │
│  CState ─────────┼───room2────┤────── CState     │
│                  │────TCP─────│                  │
│  IntentConfig ───┼───room3────┤──── IntentConfig │
│                  │            │                  │
└──────────────────┘            └──────────────────┘
     ONE CONNECTION with 3 rooms multiplexed
```

**Key Point**: SessionManager multiplexes **all rooms over the single TCP connection**.

No separate connections per component. No separate connections per room.

---

## Connection Loss: Component Behavior

### What Happens When Connection Dies?

**From Component Perspective**: Nothing changes (initially).

**Actix Message Behavior**:
```rust
// Component sends message
self.session_manager.send_to_room(room_id, message);
// This is fire-and-forget - always succeeds (returns immediately)

// The message goes into SessionManager's mailbox
// If connection is dead, SessionManager buffers or drops it
```

**Component remains alive** - it doesn't crash when connection dies.

**Auto-Reconnect Handles It**:
- `ClientBuilder` has reconnect logic (connection layer)
- When connection restored, SessionManager resumes sending
- Components don't need to know connection was lost

### Connection Lifecycle Events

**Components CAN subscribe to connection lifecycle if needed**:
```rust
// From Network Layer Actor Design
SessionEvent::Active { peer_id, role, ... }  // Connection established
SessionEvent::Inactive { peer_id }           // Connection lost
```

But most components don't need this - they just keep sending messages.

---

## Auto-Reconnect Logic

### Client-Side Reconnection

**Who Does It**: `ClientBuilder` or transport layer actor

**Behavior**:
```rust
loop {
    match try_connect("database:9001").await {
        Ok(connection) => {
            // Connected! Hand to SessionManager
            session_manager.handle_new_connection(connection);
            // Wait for connection to die
            connection.wait_for_error().await;
        }
        Err(e) => {
            log::warn!("Connection failed: {}, retrying in 5s", e);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
```

**Key Points**:
- Reconnect happens **automatically** at transport layer
- Exponential backoff recommended (5s, 10s, 30s, 60s max)
- Components don't participate in reconnection logic

---

## Database Connection Management

### Server-Side: Passive Acceptance

**Database doesn't reconnect** - it accepts connections.

```rust
// Database main loop (simplified)
let listener = TcpListener::bind("0.0.0.0:9001").await?;

loop {
    let (stream, addr) = listener.accept().await?;
    log::info!("New connection from {}", addr);

    // Spawn handler for this connection
    tokio::spawn(async move {
        handle_connection(stream, session_manager.clone()).await;
    });
}
```

**Multiple Connections**:
- Database has `Vec<PeerSession>` or `HashMap<PeerId, PeerSession>`
- Each connection is independent
- Database can distinguish them by `PeerId` (from HELLO)

---

## Connection vs. Session

### Important Distinction

**Connection** = Transport-level TCP connection
**Session** = Application-level peer relationship

```
One TCP connection → One PeerSession in SessionManager
Connection dies → PeerSession removed
Reconnect → New PeerSession created (might have same PeerId/hostname)
```

**Components work with Sessions, not Connections**.

---

## Failure Modes

### 1. Connection Fails During Send

**What Happens**:
- Transport layer detects send error
- Connection actor terminates
- SessionManager removes PeerSession
- Auto-reconnect starts

**Component Impact**: None. Component keeps sending messages (they buffer or drop).

### 2. Connection Fails During Receive

**What Happens**:
- Transport layer detects EOF or error
- Connection actor terminates
- SessionManager removes PeerSession
- Components receive `SessionEvent::Inactive` (if subscribed)

**Component Impact**: Components can react to `Inactive` event if needed, but don't have to.

### 3. Network Partition (Long-Duration)

**Collector Behavior**:
- Auto-reconnect keeps trying
- Components continue operating (MemDB buffers data)
- When connection restored, buffered data sends

**Database Behavior**:
- Notices connection is gone (TCP timeout or keepalive)
- Marks that collector as offline
- Waits for reconnect

---

## Summary: Simple Connection Model

1. ✅ **ONE connection per app** (not per component, not per room)
2. ✅ **Database is TCP server** (passive, listens on one port)
3. ✅ **Collectors/Clients are TCP clients** (active, connect to database)
4. ✅ **All rooms multiplexed** over the single connection
5. ✅ **Components are connection-agnostic** (don't know about TCP)
6. ✅ **Auto-reconnect at transport layer** (components don't participate)
7. ✅ **Connection ownership in main()** (via ServerBuilder/ClientBuilder)

**Architecture Principle**: Keep it simple. One connection, many rooms, components don't care.
