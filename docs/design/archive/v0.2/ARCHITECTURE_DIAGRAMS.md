# ZZPing Architecture Diagrams

NOTE: Deprecated documentation.

**Visual guide to understanding the ZZPing architecture**

---

## Diagram 1: Overall System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        COLLECTOR PROCESS                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐     │
│  │ IntentConfig │    │   Pinger     │    │    MemDB     │     │
│  │  (Collector) │    │              │    │  (Collector) │     │
│  │              │    │              │    │              │     │
│  │  Receives    │───→│  Pings       │───→│  Buffers     │     │
│  │  config      │    │  targets     │    │  results     │     │
│  └──────────────┘    └──────────────┘    └──────────────┘     │
│         ↕                                         ↕             │
│    (via room)                                (via room)         │
│         ↕                                         ↕             │
│  ┌──────────────────────────────────────────────────────┐      │
│  │          SessionManager (Client Mode)                │      │
│  │  - Manages connection to database                   │      │
│  │  - Routes messages to/from rooms                    │      │
│  └──────────────────────────────────────────────────────┘      │
│         ↕                                         ↕             │
└─────────────────────────────────────────────────────────────────┘
          ↕                                         ↕
     (TCP/TLS connection)                      (TCP/TLS)
          ↕                                         ↕
┌─────────────────────────────────────────────────────────────────┐
│         ↕                                         ↕             │
│  ┌──────────────────────────────────────────────────────┐      │
│  │          SessionManager (Server Mode)                │      │
│  │  - Accepts connections from collectors               │      │
│  │  - Routes messages to/from rooms                     │      │
│  │  - Manages multiple peer connections                 │      │
│  └──────────────────────────────────────────────────────┘      │
│         ↕                                         ↕             │
│    (via room)                                (via room)         │
│         ↕                                         ↕             │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐     │
│  │ IntentConfig │    │ CollectorState│    │    MemDB     │     │
│  │  (Database)  │    │  (Database)   │    │  (Database)  │     │
│  │              │    │              │    │              │     │
│  │  Distributes │    │  Tracks      │    │  Stores      │     │
│  │  config      │    │  collectors  │    │  ping data   │     │
│  └──────────────┘    └──────────────┘    └──────────────┘     │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│                        DATABASE PROCESS                         │
└─────────────────────────────────────────────────────────────────┘
```

---

## Diagram 2: Component Structure (using MemDB as example)

```
┌─────────────────────────────────────────────────────────────────┐
│                    zzmem-db Component Crate                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  network_messages.rs (Messages sent over network)      │    │
│  │                                                         │    │
│  │  enum MemDBMessage {                                   │    │
│  │      SubmitBatch { results: Vec<PingResult> },         │    │
│  │      BatchAck { received_count: usize },               │    │
│  │      Query { target: String, ... },                    │    │
│  │      QueryResponse { results: Vec<...> },              │    │
│  │  }                                                      │    │
│  └────────────────────────────────────────────────────────┘    │
│                             ↕                                   │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  actor.rs (Component implementation)                   │    │
│  │                                                         │    │
│  │  struct MemDBActor<T> {                                │    │
│  │      role: MemDBRole,                                  │    │
│  │      data_store: HashMap<String, Vec<...>>,            │    │
│  │      session_manager: Option<Rc<SessionManager<...>>>, │    │
│  │      // Health metrics                                 │    │
│  │  }                                                      │    │
│  │                                                         │    │
│  │  impl Handler<MemDBMessage> for MemDBActor<T> {       │    │
│  │      // Handle incoming network messages               │    │
│  │  }                                                      │    │
│  └────────────────────────────────────────────────────────┘    │
│                             ↕                                   │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  role.rs (Role configuration)                          │    │
│  │                                                         │    │
│  │  enum MemDBRole {                                      │    │
│  │      Collector,                                        │    │
│  │      Database { max_results_per_target: usize },       │    │
│  │  }                                                      │    │
│  └────────────────────────────────────────────────────────┘    │
│                                                                 │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  builder.rs (Builder pattern for setup)                │    │
│  │                                                         │    │
│  │  impl MemDBBuilder {                                   │    │
│  │      pub fn new(role: MemDBRole) -> Self               │    │
│  │      pub fn with_session_manager(self, ...) -> Self    │    │
│  │      pub fn start(self) -> Result<Addr<...>, Error>    │    │
│  │  }                                                      │    │
│  └────────────────────────────────────────────────────────┘    │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Diagram 3: Message Flow (Config Update Example)

```
┌──────────────────────────────────────────────────────────────────┐
│ Step 1: Admin changes config on database                        │
└──────────────────────────────────────────────────────────────────┘

DATABASE PROCESS:
┌────────────────┐
│  Config File   │
│  (intent.ron)  │
└────────┬───────┘
         │ (read/write)
         ↓
┌────────────────┐
│ IntentConfig   │
│  (Database)    │──→ Persist to disk
└────────┬───────┘
         │ (send ConfigUpdate message)
         ↓
┌────────────────┐
│ SessionManager │──→ Route to "intent-config" room
└────────┬───────┘
         │ (serialize to bytes)
         ↓
┌────────────────┐
│ TCP/TLS Socket │──→ Send over network
└────────────────┘

┌──────────────────────────────────────────────────────────────────┐
│ Step 2: Message travels over network                            │
└──────────────────────────────────────────────────────────────────┘

         Network (mTLS encrypted)

┌──────────────────────────────────────────────────────────────────┐
│ Step 3: Collector receives and processes                        │
└──────────────────────────────────────────────────────────────────┘

COLLECTOR PROCESS:
┌────────────────┐
│ TCP/TLS Socket │──→ Receive bytes
└────────┬───────┘
         │ (deserialize from bytes)
         ↓
┌────────────────┐
│ SessionManager │──→ Route from "intent-config" room
└────────┬───────┘
         │ (deliver ConfigUpdate message)
         ↓
┌────────────────┐
│ IntentConfig   │──→ Update local state
│  (Collector)   │
└────────┬───────┘
         │ (send UpdateTargets message locally)
         ↓
┌────────────────┐
│     Pinger     │──→ Start pinging new targets
└────────────────┘
```

---

## Diagram 4: Room Concept (The Most Misunderstood Part)

### WRONG Mental Model ❌

```
         Database
            │
            │  (broadcasting to all)
      ┌─────┼─────┐
      │     │     │
  Collector-1  Collector-2  Collector-3

  "Room is like IRC channel - broadcast to everyone"
```

### CORRECT Mental Model ✅

```
Connection 1:
  Collector-1 ←─ "memdb" room ─→ Database
               (point-to-point)

Connection 2:
  Collector-2 ←─ "memdb" room ─→ Database
               (point-to-point)

Connection 3:
  Collector-3 ←─ "memdb" room ─→ Database
               (point-to-point)

Three SEPARATE rooms, same name, different connections
Each is a 1:1 typed channel
```

---

## Diagram 5: SessionManager Layer Boundaries

```
┌────────────────────────────────────────────────────────────┐
│  APPLICATION LAYER (Components)                            │
│  - MemDB, IntentConfig, Pinger, etc.                       │
│  - Business logic only                                     │
│  - Registers room handlers                                 │
└────────────────────────────────────────────────────────────┘
                        ↕
             TypedMessage (Rust structs)
                        ↕
┌────────────────────────────────────────────────────────────┐
│  SESSION LAYER (SessionManager)                            │
│  - 100% typed, NEVER touches bytes                         │
│  - Routes messages to/from rooms                           │
│  - Manages peer connections                                │
│  - Handles PublishRooms negotiation                        │
└────────────────────────────────────────────────────────────┘
                        ↕
             TypedMessage (Rust structs)
                        ↕
┌────────────────────────────────────────────────────────────┐
│  SERIALIZATION LAYER                                       │
│  - Converts TypedMessage ↔ bytes                           │
│  - Part of HELLO handler or separate                       │
└────────────────────────────────────────────────────────────┘
                        ↕
                   Vec<u8> (bytes)
                        ↕
┌────────────────────────────────────────────────────────────┐
│  TRANSPORT LAYER (TCP/TLS)                                 │
│  - Network I/O                                             │
│  - Completely pluggable (can use mock)                     │
└────────────────────────────────────────────────────────────┘
```

**Critical**: SessionManager boundary is between typed and bytes. **Everything above** SessionManager = typed.
**Everything below** SessionManager = bytes.

---

## Diagram 6: Connection Lifecycle

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. INITIAL STATE (No Connection)                                │
└─────────────────────────────────────────────────────────────────┘

Collector:                     Database:
┌─────────────┐                ┌─────────────┐
│ Components  │                │ Components  │
│ (waiting)   │                │ (listening) │
└─────────────┘                └─────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ 2. CONNECTION ESTABLISHED                                        │
└─────────────────────────────────────────────────────────────────┘

Collector:                     Database:
┌─────────────┐                ┌─────────────┐
│ TCP CONNECT │───────────────→│ TCP ACCEPT  │
└─────────────┘                └─────────────┘
       ↓                              ↓
┌─────────────┐                ┌─────────────┐
│ HELLO       │←──────────────→│ HELLO       │
│ Handshake   │  (exchange ID, │ Handshake   │
│             │   rooms, auth) │             │
└─────────────┘                └─────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ 3. ACTIVE STATE (Rooms Negotiated)                              │
└─────────────────────────────────────────────────────────────────┘

Collector:                     Database:
┌─────────────┐                ┌─────────────┐
│ PublishRooms│                │ PublishRooms│
│ ["memdb",   │                │ ["memdb",   │
│  "intent-   │                │  "intent-   │
│   config"]  │                │   config"]  │
└─────────────┘                └─────────────┘
       ↓                              ↓
   Intersection = ["memdb", "intent-config"]
       ↓                              ↓
┌─────────────┐                ┌─────────────┐
│ Auto-join   │                │ Auto-join   │
│ both rooms  │                │ both rooms  │
└─────────────┘                └─────────────┘
       ↓                              ↓
┌─────────────┐                ┌─────────────┐
│ Components  │←──room "X"────→│ Components  │
│ active and  │                │ active and  │
│ communicating│                │ communicating│
└─────────────┘                └─────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ 4. CONNECTION LOST (Network Failure)                            │
└─────────────────────────────────────────────────────────────────┘

Collector:                     Database:
┌─────────────┐                ┌─────────────┐
│ Disconnect  │    ✗ ✗ ✗ ✗    │ Disconnect  │
│ event       │                │ event       │
└─────────────┘                └─────────────┘
       ↓                              ↓
┌─────────────┐                ┌─────────────┐
│ Components  │                │ Components  │
│ notified    │                │ notified    │
│ (SessionEvent│                │ (SessionEvent│
│  ::Inactive)│                │  ::Inactive)│
└─────────────┘                └─────────────┘
       ↓                              ↓
   Buffer data                    Wait for
   locally                        reconnection

┌─────────────────────────────────────────────────────────────────┐
│ 5. RECONNECTION (Back to Step 2)                                │
└─────────────────────────────────────────────────────────────────┘

Collector:                     Database:
┌─────────────┐                ┌─────────────┐
│ Auto-       │───────────────→│ Accept new  │
│ reconnect   │                │ connection  │
└─────────────┘                └─────────────┘
       ↓                              ↓
   Repeat HELLO, PublishRooms, etc.
       ↓                              ↓
┌─────────────┐                ┌─────────────┐
│ Flush       │───────────────→│ Receive     │
│ buffered    │                │ backlog     │
│ data        │                │             │
└─────────────┘                └─────────────┘
```

**Key Point**: Reconnection is treated as NEW connection. No state carried over - must be renegotiated.

---

## Diagram 7: Testing Strategy (Mock vs Real)

### Mock Transport (Unit/Integration Tests)

```
┌─────────────────────────────────────────────────────────────────┐
│                     IN-MEMORY TEST                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Component A          Mock Channels          Component B       │
│  ┌──────────┐         (in memory)          ┌──────────┐        │
│  │          │                               │          │        │
│  │ MemDB    │←──mpsc::channel────────────→│ MemDB    │        │
│  │(Collector│                               │(Database)│        │
│  │          │                               │          │        │
│  └──────────┘                               └──────────┘        │
│                                                                 │
│  NO network I/O! Tests run in microseconds!                     │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Real Transport (System Tests)

```
┌─────────────────────────────────────────────────────────────────┐
│                     REAL NETWORK TEST                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Component A              TCP/TLS              Component B      │
│  ┌──────────┐         (real network)         ┌──────────┐      │
│  │          │                                 │          │      │
│  │ MemDB    │←──TCP Socket─────────────────→│ MemDB    │      │
│  │(Collector│    (localhost:9001)            │(Database)│      │
│  │          │                                 │          │      │
│  └──────────┘                                 └──────────┘      │
│                                                                 │
│  Uses real network! Only for smoke tests!                       │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

**Strategy**:

- 90% of tests use mock transport
- 10% use real network (smoke tests only)
- Mock validates architecture, real validates implementation

---

## Diagram 8: Data Flow (Complete Ping Cycle)

```
┌─────────────────────────────────────────────────────────────────┐
│                    COLLECTOR PROCESS                            │
└─────────────────────────────────────────────────────────────────┘

1. Config Update Received
   ┌──────────────┐
   │ IntentConfig │← (from database via "intent-config" room)
   │  (Collector) │
   └──────┬───────┘
          │ UpdateTargets(["8.8.8.8", "1.1.1.1"])
          ↓
   ┌──────────────┐
   │    Pinger    │
   └──────┬───────┘

2. Perform Pings
          │ (sends ICMP echo requests)
          ↓
   [Network Interface]
          ↓
   [Target: 8.8.8.8, 1.1.1.1, ...]
          ↓
   (collect responses + timeouts)
          ↓
   ┌──────────────┐
   │  PingResult  │
   │  queue       │
   └──────┬───────┘

3. Send Results to MemDB (local)
          │ PingResult { target: "8.8.8.8", rtt_us: 15000, ... }
          ↓
   ┌──────────────┐
   │    MemDB     │
   │  (Collector) │← Buffers results
   └──────┬───────┘

4. Send Batch to Database
          │ (periodic batch, e.g., every 1 second)
          │ SubmitBatch { results: [50 pings] }
          ↓
   ┌──────────────┐
   │ SessionMgr   │→ Route to "memdb" room
   └──────────────┘
          ↓
      [TCP/TLS]
          ↓

┌─────────────────────────────────────────────────────────────────┐
│                    DATABASE PROCESS                             │
└─────────────────────────────────────────────────────────────────┘

          ↓
      [TCP/TLS]
          ↓
   ┌──────────────┐
   │ SessionMgr   │← Route from "memdb" room
   └──────┬───────┘
          │ SubmitBatch { results: [50 pings] }
          ↓
   ┌──────────────┐
   │    MemDB     │
   │  (Database)  │← Store in memory
   │              │← Update indices
   │              │← Persist to disk (optional)
   └──────┬───────┘

5. Send Acknowledgment
          │ BatchAck { received_count: 50 }
          ↓
   ┌──────────────┐
   │ SessionMgr   │→ Route to "memdb" room
   └──────────────┘
          ↓
      [TCP/TLS]
          ↓

┌─────────────────────────────────────────────────────────────────┐
│                    COLLECTOR PROCESS                            │
└─────────────────────────────────────────────────────────────────┘

          ↓
      [TCP/TLS]
          ↓
   ┌──────────────┐
   │ SessionMgr   │← Route from "memdb" room
   └──────┬───────┘
          │ BatchAck { received_count: 50 }
          ↓
   ┌──────────────┐
   │    MemDB     │
   │  (Collector) │← Clear sent buffer
   │              │← Update health metrics
   └──────────────┘

CYCLE COMPLETE - Repeat every second
```

---

**These diagrams should help visualize the architecture!**

Key takeaways:

1. **Same component, different roles** (not separate components)
2. **Rooms are 1:1** (not broadcast channels)
3. **SessionManager never touches bytes** (transport-agnostic)
4. **Test with mocks first** (validates abstraction)
5. **Data flows through rooms** (typed messages)

For more details, see:

- `IMPLEMENTATION_PLAN_OCT2025.md` - Detailed plan
- `QUICK_START_GUIDE.md` - Getting started
- `ZZPing_Network_Layer_Vision.md` - Core vision
