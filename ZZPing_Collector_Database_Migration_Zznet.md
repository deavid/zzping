# ZZPing: Migration plan for Apps Collector and Database into ZzNet

**Date**: October 6, 2025
**Status**: Draft, WIP, writting in progress.
**Author**: David Martinez Marti

---

## Current Status

The codebase has had way too many refactors, and after a bit of clean-up there are now two clearly separated "projects" here.

1) The old zzping refactor, that is functional to some degree:

src/apps/zzping-cli
src/apps/zzping-collector
src/apps/zzping-database
src/apps/zzping-gui
src/common/zzping-lib
src/common/zzping-proto

2) The new zznet refactor, along with a demonstration actor/component "zzintent-config":

src/actors/zzintent-config
src/actors/zznet-hello
src/common/zznet-api
src/common/zzping-auth
src/components/zznet-builder
src/components/zznet-room
src/components/zznet-session
src/transports/zznet-transport-tcp

The old one (1) is functional, it uses gRPC for communication. The new one (2) creates a framework around Actix for communicating
actors across the network.

The objective is to remove gRPC completely, and use this new architecture instead. However this is not possible: The old codebase (1)
does not use anything remotely similar to a component - it is just a massive unit of code interlinked, and components/actors in (2)
must have isolation between each other.

Attempting to replace gRPC with ZzNet is not going to work, it is naive, and it is deemed to fail because it does require a complete
rewrite of everything.

## The overall plan

We do not want to touch the old code (1) at all. What we want to do is to build our way in (2) until we reach feature parity, and
once there, verify and prove it (by a human) and once a human has verified that the new stack is good enough, we remove the
old code (1).

We could however move the old crates (1) into a subfolder, like "src/old/*", such that we are clear on what we're not touching.

And if doing this, we probably want to rethink and reorganize the folders for the new crates (2).

Mainly the actors vs components. I would like to call components only "zzintent-config" and the other ones that we will do.
So a component would be an Actix actor, plus everything else needed to work with zznet, which might include additional actors.

All zznet-* crates could just be in "src/net/". And zzping-auth would be refactored into zznet-auth.

So the expected result would be:

Old code (1):

src/old/apps/zzping-cli
src/old/apps/zzping-collector
src/old/apps/zzping-database
src/old/apps/zzping-gui
src/old/common/zzping-lib
src/old/common/zzping-proto

Zznet crates:

src/net/zznet-hello
src/net/zznet-api
src/net/zznet-auth
src/net/zznet-builder
src/net/zznet-room
src/net/zznet-session
src/net/zznet-transport-tcp

New common crates:

(*) src/common/zzping-lib (for auth specifics, and other shared code)

Components:

src/components/zzintent-config
(*) src/components/zzmem-db
(*) src/components/zzpinger
(*) src/components/zzcollector-state (cstate)
(*) src/components/zztcp-lock
(*) src/components/zzhealth
.. etc

New apps:

(*) src/new-apps/zzping-cli
(*) src/new-apps/zzping-collector
(*) src/new-apps/zzping-database
(*) src/new-apps/zzping-gui


(*): Crates marked (*) do not exist yet.


I would expect the renaming/repathing of crates to take place as a first step before moving on. I don't want the new crates to be created
until they're needed, so the new crates should not be created until the last moment.

UPDATE: The crate rename has been performed.

## Refactor of zzping-auth

This should actually be zznet-auth, 90% of the code is generic over applications, 10% of it is tied to zzping. We probably need
to see how to convert role.rs::AuthRole into a trait or something, such that each application can implement their own set of roles.

I also noticed that zzping-auth is used along several other crates only for the purpose of tests. Instead, these crates should not
require application specific roles and mock their own. Maybe the mock roles can be in zznet-auth for reuse across tests - that
would be fine.

## Components overview

The plan is to create new components, that would bring in all the functionality. However, I don't have it all figured out. Especially
when it comes to the CLI/GUI crates, I'm not clear how to use the architecture to extract the data needed. This is something we will
figure out last.

A component is basically an actor, just that it might contain additional stuff, such as additional actors for zznet comms. An application
would be made of components wired together.

An app is either a service or a program that contains several components, configures and wires them together to accomplish something.

### Apps

The database (zzping-database) is the TCP server, which is in charge of storing everything on disk and maintaining the authoritative
view of everything. It is the orchestrator for collectors, to order them around and control what they do.

The database is in turn controlled by the Client programs (CLI/GUI) in ClientAdmin mode/role. The idea being that the administrator
opens the GUI, and can configure stuff (in intent-config) visually, that gets fed into the database app, and this would then fed this
onto the collector and will control them following the orders of the administrator given via the GUI.

The client is a type of app, which has two main usages: 1) Fetch data, either past, or real-time, to observe and monitor what is happening
or what has happened - this woks in Client-RO and Client-Admin modes. 2) To configure the parameters for collectors, specify what to ping
and how frequently. There are two client apps, a CLI and a GUI.

The collector is the main objective, which is the one that pings the desired targets at the desired frequency and pushes the data
into the database.

### Components

**src/components/zzintent-config**

This component acts as a dynamic configuration source. It stores the overall ping per second, one value for all hosts, and also all the
hosts to ping.

The database app will store this on disk, and maintain it as the authoritative. Collector and clients will receive read only copies and
updates - they will subscribe to it.

The clients, with ClientAdmin role, they can modify the contents. The user is not expected to edit the files on disk on the database, and
if doing so, the database has to be stopped. The file storage here would use TOML or RON or similar - human readable. RON is probably best.

The collector, on receiving updates, will push the updates to the Pinger component to update what it's doing.

The collector, will also store a cache of this config on disk, that would use on boot, in case there's no database connection. As it is a
cache, whatever the database says, it overrides the data in cache.

Therefore this component exists on all three apps: collector, database and clients.

**src/components/zztcp-lock**

TCPLock is a collector only component. It's job is to claim mastership of the collector primary role in the machine is running by opening
a TCP port in listen mode, such that no other job can do that.

It will send updates to the CState component to inform when a lock has been grabbed. It can receive requests from CState too to try to grab
a lock on request. Otherwise, it will retry every N seconds. (usually 10 seconds, configurable).

The port used for the lock is also configurable when creating the component.

**src/components/zzcollector-state (cstate)**

CState or CollectorState is the main component that handles mastership of collectors in a machine. It does not store anything on disk.

On the collector side, this component receives updates from TCPLock which allow for a Collector to become master without database interaction.

The data is sent to the Pinger to enable pinging when it's in a mode for doing so. So, we do not disable pinging by setting ping rate to zero.
Instead, Pinger needs to expose some enabled/disabled interface to control this separately.

On the database side, this is used to negotiate between incoming collectors, to promote the new one and demote the old one safely.

On the client side, this is just read-only metrics to give admins more insights on what is happening.

**src/components/zzpinger**

Pinger is a Collector only component. It reads the instructions from IntentConfig and CState, and it just pings whatever is told.

The result of the pings, and the ongoing started pings are sent to the MemDB component.

**src/components/zzmem-db**

MemDB is a database of pings that can do a master-master replica between collector and database, and also to clients (although clients only
get read mode).

Ping component sends data here to push it to the database.

In the database app, MemDB stores to disk using a highly efficient format that chunks by minute.

In the client app, MemDB serves as an interface for querying data, and also for real time monitoring.

In the collector, MemDB caches several hours of pings in memory only, to allow for database restarts to not lose anything.

**src/components/zzhealth**

This is an optional component in case we need to push health data from other components, however it is not clear if it's truly needed.

If it's needed, it contains stats from other components in the collector, such as effective ping rates, MemDB memory limits, etc.

But it's unclear if this is needed in a separate component, or if each component could just push and share their metrics up the stack.

### Message Flow

Components can only talk to themselves in another process. This has to be very clear.

When it's intra-process (inside the same process), components talk to each other:

* Pinger -> MemDB
* IntentConfig -> Pinger
* CState -> Pinger
* TCPLock -> CState

When it's inter-process (Collector to Database, or similar), components talk to the other versions of themselves:

* MemDB (Collector) -> MemDB (Database) -> MemDB (Client)
* IntentConfig (Collector) <- IntentConfig (Database) <-> IntentConfig (Client)
* CState (Collector) <-> CState (Database) -> CState (Client)

ZZNet talks about rooms. In general, it's just a room per component that talks across process boundaries:

* "mem-db"
* "intent-config"
* "c-state"

Upon building it we can consider if a component might need multiple rooms - but this is not expected. We expect a 1:1 mapping from
components to rooms, for the components that talk across processes. Obviously for components that do not use zznet, it is a 1:0 mapping.

## Components Detail

### `zztcp-lock` Component

* Purpose: To ensure that only one primary collector process can be active on a single machine at any given time. Its existence
    prevents local "split-brain" scenarios where multiple collector instances might believe they are in charge during an upgrade
    or chaotic restart. It is key for allowing a correct mastership even with no database is reachable for negotiation.

* Key Responsibilities:
    * To exclusively acquire a resource on the local machine that can only be held by one process at a time (a conceptual
        "lock").
    * To continuously report the status of this lock (e.g., whether it is held by this process or not).

* State / Ownership: It is the sole owner and source of truth for the status of the local collector lock.

* Message Types (Conceptual):
    * Outputs: LockStatusUpdate (e.g., Acquired, Released, ContentionDetected).

* Lifecycle:
    * The zztcp-lock component itself has a static lifetime.
    * Child Tasks: This component is simple and self-contained; it does not spawn any dynamic, long-lived child tasks.

* Dependencies: It provides its status to the zzcollector-state component. It has no dependencies on other components.

### `zzcollector-state` (cstate) Component

* Purpose: To decide primary/secondary status, and allow the database to be the negotiator when a new collector spins up
  in the same host machine, such that the old collector running can safely transfer the work to the new one, without losing
  pings, and testing the new collector for correct behavior and being able to perform a rollback if the new collector is
  not behaving up to spec.

* Key Responsibilities:
    * To reconcile the desired state from the database (the "intent") with the actual state of the local machine (the lock
        status).
    * To determine and hold the collector's authoritative, final role (e.g., Primary, Standby, AwaitingLock).
    * To command the zzpinger component to either start or stop its work based on the determined role.
    * To report the collector's authoritative role back to the database for system-wide observability.

* State / Ownership: It owns the authoritative decision on the collector's current operational role.

* Message Types (Conceptual):
    * Inputs: LockStatusUpdate (from zztcp-lock), DatabaseCommand (from its peer on the database).
    * Outputs: PingerControlCommand (e.g., Activate, Deactivate, to zzpinger), RoleStatusReport (to its peer on the database).

* Lifecycle:
    * The zzcollector-state component itself has a static lifetime.
    * Child Tasks: In the database and client, It spawns one `SessionHandler` child task for each active network connection to the database.
      In the collector, there would be only 1 children.
    * Why spawn children? CState is per-collector, therefore any process that can see multiple collectors must have several. It is
      per connected collector, so if a collector disappears, the child on the other processes would be purged too. If there are two collectors
      for the same host connected, there are two CState childs one for each collector connected even if they're trying to do the same work,
      and/or they provide the same hostname/nickname.

* Dependencies: It depends on zztcp-lock for local machine status and its peer zzcollector-state component on the database for
    strategic commands. It directs the zzpinger component.

### `zzpinger` Component

* Purpose: To be the "engine" of the collector, solely responsible for executing network probes against designated targets and
    generating the raw performance data that the entire system is built to analyze.

* Key Responsibilities:
    * To execute ping operations against a list of targets at a specified rate.
    * To generate a discrete PingResult for every single attempt, explicitly capturing round-trip time for successes and
        timeout/loss information for failures.
    * To dynamically adjust its list of targets and its pinging rate in response to new configuration commands.
    * Note that not only has to send the results, but also the attempts to send ICMP, whenever that happens.

* State / Ownership: It owns the transient state of all in-flight ping operations, such as the sequence numbers and timing for
    each target. However, in-flight pings are also sent to zzmem-db as well.

* Message Types (Conceptual):
    * Inputs: PingerControlCommand (from cstate), PingConfiguration (from zzintent-config).
    * Outputs: A continuous stream of PingResult messages plus the in-flight probes (to zzmem-db).

* Lifecycle:
    * The zzpinger component itself has a static lifetime.
    * Child Tasks: It spawns one `PingerShard` child task for each unique target IP address it is configured to ping.
    * Why spawn children? This pattern solves the problem of managing concurrent ping operations to multiple, independent
        targets. Each PingerShard is responsible for the entire lifecycle of pinging a single target (sending packets, tracking
        sequence numbers, managing timeouts). This allows all targets to be pinged in parallel and isolates failures; a problem
        with one target's socket or network path will only affect its dedicated child task, not the others. The main zzpinger
        component acts as a lightweight supervisor, creating and destroying these children as the configuration changes.

* Dependencies: It depends on zzintent-config for its configuration and cstate for its activation control. It provides its
    output stream to zzmem-db.


### `zzintent-config` Component

* Purpose: To be the distributed, resilient source of the system's operational intent. It defines what should be monitored and
    how, ensuring that all parts of the system are working towards the same goal, even in the face of network interruptions.

* Key Responsibilities:
    * (When on the Database Service) To act as the single source of truth for configuration, persisting the master copy and
        publishing any changes to all subscribers.
    * (When on a Collector or Client Service) To act as a local, read-only cache of the configuration. This ensures the service
        can start up and operate with the last-known-good configuration if the database is unreachable.
    * To provide the latest valid configuration to other local components that require it.
    * Note that on Collectors, it cannot write to cache if it's not the master. This is to avoid two competing writes. Probably
      needs to read from TCPLock in the Collector to know this information.
      (To consider: do we want an InformationBase component that acts as the in-memory data storage for most of the stuff,
      so everything can just read-write to InformationBase, instead of managing specific channels towards each component, or
      is this just overcomplicating stuff?)

* State / Ownership: The instance on the database service owns the authoritative master copy of the configuration. Instances on
    collector and client services own a non-authoritative, cached copy. However the client services, with the ClientAdmin role,
    can send requests to write to the database.

* Message Types (Conceptual):
    * Inputs (on Database): ConfigurationChangeRequest (from admin clients).
    * Outputs (from Database): ConfigurationUpdate (to all subscribers).
    * Outputs (on Collector): PingConfiguration (to the local zzpinger component).

* Lifecycle:
    * The zzintent-config component itself has a static lifetime.
    * Child Tasks: None.
    * Why not spawn children? The config is the same for all collectors, all processes. There's no need to spawn childs.

* Dependencies: Subscriber instances (on collectors/clients) depend on the authoritative instance (on the database). Locally, it
    provides configuration to components like zzpinger.

### `zzmem-db` Component

* Purpose: To provide a resilient and efficient data pipeline for transporting high-volume ping results (and in-flight) from the point of
    collection to the point of storage, guaranteeing that no data is lost during transient network or service failures.

* Key Responsibilities:
    * (When on a Collector Service) To buffer a significant history of PingResult data in memory, ensuring the collector can
        survive database or network outages without data loss.
    * (When on a Collector Service) To reliably transmit its buffered data to the database, handling acknowledgments and
        re-transmissions as needed to guarantee delivery. To re-sync database data on reconnect.
    * (When on the Database Service) To receive batches of PingResult data from all connected collectors.
    * (When on the Database Service) To persist the received data to the long-term, efficient on-disk storage format.
    * (When on the Database Service) To serve data, both real-time and historical, to querying clients.
    * NOTE: It's unclear if on the database we want a separate component to handle disk reads and writes.

* State / Ownership: The instance on a collector service owns the in-memory buffer of recent, unacknowledged data. The instance
    on the database service owns the complete, persisted historical record of all data.

* Message Types (Conceptual):
    * Inputs (on Collector): A stream of PingResult messages (from zzpinger).
    * Outputs (from Collector): PingDataBatch (to its peer on the database).
    * Inputs (on Database): PingDataBatch (from collector peers), DataQuery (from clients).
    * Outputs (from Database): DataQueryResponse (to clients).

* Lifecycle:
    * The zzmem-db component itself has a static lifetime.
    * Child Tasks (on Collector): It spawns one `DataPipeline` child task for each unique source-destination ping stream.
        * Unclear if needed, or if we want just to multiplex the data.
    * Child Tasks (on Database): It spawns one `QueryHandler` child task for each data query received from a client.
        * Unclear if needed, the requirement is for serving only 1 thing at a time.

* Dependencies: On the collector, it depends on zzpinger for its data source. It communicates with its peer component on the
    database. On the database, it serves data to client applications.

* Note: Unclear how querying works exactly, and on the client side, unclear how MemDB does store these queries or how does it forward them.

---

## Architectural Paradigm Shift

The migration from gRPC to ZzNet is not just a protocol change - it represents a fundamental shift in architectural philosophy.

### From Monolithic to Pure Actor Model

**Old Architecture (gRPC-based):**
- Monolithic CollectorService owns all state
- Hierarchical supervision (CollectorService → TaskSupervisor → TargetWorker)
- Components tightly coupled through shared ownership
- Long-lived state vs ephemeral tasks separation

**New Architecture (ZzNet-based):**
- Pure actor model with complete component isolation
- No shared mutable state, only message passing
- Each component is an independent, self-contained actor
- Components are peers, not hierarchical

**Why This Matters:**
- Components must be independently testable
- No `Arc<Mutex<T>>` patterns (these are architectural code smells)
- Failures are isolated - one component's panic doesn't cascade
  - Decision: We will make the whole app fail anyway, on a component failure. This is because components have static lifetimes.
    However, for child components, these we could optionally handle them better. But out of scope for now.
    If we wanted all components to be surviving to panics, we would need a way to self-restart, self-rewire them. This is out of scope.
- Enables true mock-first development

### Transport-Agnostic Design

**Critical Principle:** Components never touch bytes or serialization directly.

## Connection Topology and Ownership

### The Simple Connection Model

**One Connection Per App** - Each application (collector, database, GUI, CLI) has a **single TCP connection** to the database. All rooms are multiplexed over this one connection.

**Database is TCP Server:**
- Database binds to one listening socket (e.g., `0.0.0.0:9001`)
- Database is **passive** - waits for incoming connections
- Database accepts connections from multiple collectors and clients
- Each connection is independent and identified by `PeerId` (hostname from HELLO)

**Collectors/Clients are TCP Clients:**
- Each collector/client opens **one connection** to database
- Clients are **active** - they initiate the connection
- Clients have **auto-reconnect logic** at transport layer
- If connection fails, reconnect happens automatically

**Connection Ownership:**
```rust
// In main() - database side (server)
let session_manager = ConnectionManager::<MessageType>::new(rooms).start();

ServerBuilder::new()
    .bind("0.0.0.0:9001")
    .with_connection_manager(session_manager.clone())
    .start()
    .await?;

// In main() - collector side (client)
ClientBuilder::new()
    .connect_to("database:9001")
    .with_connection_manager(session_manager.clone())
    .connect()
    .await?;
```

The `ServerBuilder`/`ClientBuilder` owns the connection lifecycle. Components never see TCP sockets.

### Component Connection Agnosticism

**Critical Design Principle:** Components are **completely unaware of connections**.

**What Components See:**
- A `SessionManager` reference
- `send_to_room(room_id, message)` method
- Messages arriving in their actor mailbox
- Optionally: `SessionEvent::Active`/`Inactive` events

**What Components DON'T See:**
- TCP sockets or connection state
- Serialization/deserialization
- Reconnection logic
- Transport-level errors

**Example:**
```rust
// Component code - connection-agnostic
fn handle_event(&mut self) {
    self.session_manager.send_to_room(
        RoomId::from("c-state"),
        CStateMessage::StatusReport { ... }
    );
    // Component has NO IDEA if connection is alive or dead!
}
```

### Multiplexing: All Rooms Over One Connection

```
Collector Process                Database Process
┌──────────────────┐            ┌──────────────────┐
│  MemDB ──────────┼───room1────┤────── MemDB      │
│  CState ─────────┼───room2────┤────── CState     │
│  IntentConfig ───┼───room3────┤──── IntentConfig │
└──────────────────┘            └──────────────────┘
        └────────ONE TCP CONNECTION────────┘
```

All rooms share the single TCP connection. SessionManager handles the multiplexing.

### Connection Loss Behavior

**When Connection Dies:**

1. **Component Perspective**: Nothing changes immediately
   - Components keep sending messages (fire-and-forget)
   - Messages go into SessionManager's mailbox
   - SessionManager buffers or drops based on policy

2. **Transport Layer**: Auto-reconnect starts
   - `ClientBuilder` reconnect loop (exponential backoff)
   - When reconnected, buffered messages resume sending
   - Components don't participate in reconnection

3. **Optional Lifecycle Events**: Components can subscribe if needed
   - `SessionEvent::Active { peer_id, ... }` - connection established
   - `SessionEvent::Inactive { peer_id }` - connection lost
   - Most components ignore these (they just keep operating)

**Design Philosophy:** Decouple component logic from network state. Components work whether connected or not.



## Component Lifecycle and Wiring

**The Layer Boundary:**

```
Application Components (typed Rust messages)
    ↓
SessionManager (100% typed, never touches bytes)
    ↓
Serialization Layer (typed → bytes)
    ↓
HELLO Handler (peer identity, bytes only)
    ↓
Transport (TCP/TLS/mock, bytes only)
```

**What This Enables:**
- SessionManager is completely testable without network I/O
- Two SessionManagers can communicate via in-memory channels (no TCP)
- Real transport becomes "just another implementation"
- Mock transport proves the abstraction works

**Validation Test:** If you can't test two components communicating via mock channels (no network), the architecture is wrong.

---

## Component Lifecycle and Wiring

### The Three-Phase Lifecycle Pattern

Every component in the new architecture follows this explicit pattern:

**Phase 1: Instantiation (Builder)**
- Components are created as inert builder objects
- Configuration only, no active logic, no spawned tasks
- Example: `let pinger = PingerComponent::new(config);`

**Phase 2: Wiring (Dependency Injection)**
- Builders are connected to each other
- Intra-process channels are created and shared
- Still no actors running, everything is deterministic
- Example: `pinger.connect_to_memdb(&memdb);`

**Phase 3: Activation (Handle)**
- `.start()` consumes builder and spawns actor task
- Returns a handle for controlling the component
- Actor is now running independently
- Example: `let handle = pinger.start();`

### Why This Pattern Matters

**Eliminates "Cold Start" Race Conditions:**
- In traditional systems, publishers can send before subscribers are ready
- Result: Lost initial messages, inconsistent state
- The wire-then-start pattern makes this impossible by design

**Compiler-Enforced Ordering:**
- All channels created before any message can be sent
- All subscribers guaranteed to be listening before publishers start
- Moves correctness from runtime logic to compile-time guarantees

**This is not optional** - it's fundamental to the reliability of the system.

---

## Operational Context and Resilience

### System-Level Resilience Design

**Network Partition Tolerance:**
- All services designed to survive **1+ hour** of network isolation
- Collectors continue pinging and buffer results in memory
- Database continues serving with existing data
- Clients work with cached data (Note that for clients, this is relaxed, the clients may need permanent connection)

**Fail-Static Behavior:**
- Services maintain last-known-good state during outages
- No unpredictable fail-open or fail-closed behaviors
- Configuration cache on disk enables offline operation

**Automatic Recovery:**
- Services managed by process supervisors (systemd, Docker, Kubernetes)
- Crash = restart in clean state (fast, safe, predictable)
- Service-level state preserved independently of connection state

### Why "Fail Fast" Works

**At Connection Level:**
- Connection failures are cheap - tear down and reconnect
- Remote service is designed to handle connection loss
- Reconnection happens automatically

**At Service Level:**
- Unexpected errors → panic → supervisor restart
- Better to crash and restart clean than continue in corrupted state
- State is preserved in buffers and on disk

**This operational model justifies many architectural decisions** - we can afford to be aggressive about tearing down bad connections.

---

## Message Flow and Communication Patterns

### Symmetric Protocol: Components Talk to Themselves

**Fundamental Principle:** A component on one process talks only to the same component on another process.

**Not This (Asymmetric):**
```
Collector: PingSubmitter → Database: PingReceiver (different components)
```

**This (Symmetric):**
```
Collector: MemDB → Database: MemDB (same component)
```

**Why This Matters:**
- All communication code for a component lives in ONE place
- Easy to reason about - you see both sides of protocol in same file
- Testing is trivial - instantiate component twice with different configs
- Natural protocol symmetry

### Intra-Process vs Inter-Process

**Intra-Process (Reliable):**
- Different components talk to each other
- Direct Tokio channels (mpsc, watch, oneshot)
- Examples: Pinger → MemDB, IntentConfig → Pinger, TCPLock → CState

**Inter-Process (Unreliable):**
- Same component talks to itself in another process
- ZzNet rooms over network
- Examples: MemDB ↔ MemDB, IntentConfig ↔ IntentConfig, CState ↔ CState

**Room Mapping:**
- 1:1 mapping from component to room name
- "mem-db" room, "intent-config" room, "c-state" room
- Components that don't cross process boundaries don't need rooms

---

## Request-Response in a Message World

### The Challenge

gRPC provides native request-response:
```rust
let response = client.send_batch(request).await?;
```

ZzNet is message-based - how do we implement request-response patterns?

We don't. All components should be designed in a way such that they communicate in a fire and forget pattern.

If this is a problem, we would need to go back to the drawing board in zznet to see how to allow a better request-response pattern.

---

## Data Integrity and DESYNC Recovery

### The DESYNC Protocol (Conceptual)

**Problem:** After collector or database restart, how to ensure no data gaps or duplicates?

**gRPC Solution:**
```
1. Collector tracks: last_acked_cursor
2. SendBatchRequest includes: collector_believes_cursor
3. Database checks: does my cursor match collector's belief?
4. If match: Accept data, advance cursor, ACK
5. If mismatch: Reject with DESYNC, send authoritative cursor
6. Collector rewinds buffer to database cursor, resends
```

**ZzNet Translation (Conceptual):**
```
Messages over "mem-db" room:
- PingDataBatch { cursor, data }
- BatchAck { cursor }
- DesyncNotification { authoritative_cursor }
- ResyncRequest { from_cursor }
- ResyncResponse { cursor, data }
```

**Key Properties:**
- Cursor must be monotonic (use received_nanos, not sent_nanos)
- Buffer must support efficient seek by cursor (B-Tree keyed by cursor)
- Recovery is stateful - requires message correlation

**This is complex** - needs careful design for message-based implementation.

NOTE: This might be completely wrong, needs human revision, and better explanation. Do not trust this. A discussion needs to happen first.

---

## Handoff Orchestration Vision

### The Zero-Downtime Handoff Protocol

**Goal:** Upgrade collector binary without losing pings.

**High-Level Flow:**
1. Old collector (C1) is PRIMARY with TCP lock
2. New collector (C2) starts, becomes STANDBY (no TCP lock, no pinging)
3. Database detects two collectors with same hostname (two connections, same identity)
4. Database orchestrates via "c-state" room messages:
   - Sends C1: PrepareToSwap { role: SUPERVISING, swap_time }
   - Sends C2: PrepareToSwap { role: PRIMARY, swap_time }
5. At swap_time, both collectors atomically change roles
6. C1 stops pinging but continues reporting (supervision mode)
7. C2 starts pinging
8. Database monitors C2 health
9. Success: Database commands C1 to shutdown
10. Failure: Database commands C1 to resume PRIMARY, C2 to exit

**Critical Components:**
- Pre-scheduled swap time (avoids polling delay)
- C2 calls GetRecentData to seed buffer from database
- C1 in SUPERVISING mode provides verification
- Database is orchestrator (single source of authority)

**ZzNet Implications:**
- Complex state machine over "c-state" room messages
- Requires precise timing coordination
- Needs request-response pattern for GetRecentData

**This is probably the hardest feature to implement** in the new architecture.

This needs discussion.

---

## Testing Philosophy

### Mock-First Development

**Core Principle:** Components must be testable without any network I/O.

**Testing Hierarchy:**
1. **Unit tests:** Component with mock dependencies (in-memory channels)
2. **Integration tests:** Two components via mock transport (no TCP)
3. **E2E tests:** Full apps via mock transport (no TCP)
4. **Smoke tests:** Real TCP/TLS (minimal, just validation)

**Validation Test:**
```rust
#[test]
fn test_memdb_replication_no_network() {
    // Create two MemDB components
    let collector_memdb = MemDB::new(MemDBConfig::Collector);
    let database_memdb = MemDB::new(MemDBConfig::Database);

    // Connect via mock channels (no network!)
    let mock = MockConnector::new();
    mock.connect(&collector_memdb, &database_memdb);

    // Collector receives ping data
    collector_memdb.receive_ping(ping_result);

    // Should arrive at database (no network I/O!)
    assert_eq!(database_memdb.query_latest(), ping_result);
}
```

**If this test doesn't work, the architecture is wrong.**

### Transport-Agnostic Validation

**The Critical Property:**
- SessionManager never touches bytes
- Components communicate via typed messages only
- Transport is completely pluggable

**Proof:** If you can't swap TCP for mock without changing component code, the abstraction leaked.

---

## Open Design Questions

### Questions Requiring Decisions

1. **Request-Response Pattern:** Which pattern for queries? Message correlation? Ephemeral channels? Session actor state?

   * Should be fire-and-forget only. We need to ensure that all the design works thisway.

2. **DESYNC State Machine:** How to implement multi-message stateful recovery in message-passing world?

   * We need to discuss this.

3. **MemDB Component Boundaries:**
   - Should database spawn separate disk I/O component?
   - Should collector spawn per-target DataPipeline children?
   - Should database spawn per-query children or serialize?

4. **Query Multiplexing:** Can clients query concurrently or must queries be serial?

   * Not needed. Serial is enough.

5. **Handoff Precision:** How to implement pre-scheduled swap time coordination over messages?

   * We need to discuss this.

6. **Health Reporting:** Separate zzhealth component or built into each component?

   * I'm not sure it is needed. We can defer for later.

7. **InformationBase Pattern:** Central in-memory data store for sharing state between components, or keep point-to-point channels?

   * This is something to analyze, would reduce the dependency tree.

### Questions Deferred to Implementation

- Exact message type definitions for each room
- Serialization format (bincode, MessagePack, Cap'n Proto?)
   * I'm favoring MessagePack.
- Connection multiplexing strategy
- Backpressure handling
- Buffer pruning policies
- Metrics and observability

---

## Next Steps (Planning)

NOTE: This is all tentative, unreviewed, do not trust this.

### What Needs to Be Written Next

1. **Room Message Definitions:** Concrete message types for each room with direction and semantics
2. **Sequence Diagrams:** Message flows for key scenarios (startup, data submission, DESYNC recovery, handoff)
3. **App Composition:** How components are wired together in each app's main.rs
4. **Migration Strategy:** Phase-by-phase implementation plan with testing checkpoints

### Likely Implementation Order

1. **Phase 0:** Reorganize crates (move old to src/old/, move zznet to src/net/)
2. **Phase 1:** Implement simple component (zztcp-lock or enhance zzintent-config)
3. **Phase 2:** Implement core data flow (zzpinger, zzmem-db on collector)
4. **Phase 3:** Implement database side (zzmem-db on database, disk I/O)
5. **Phase 4:** Implement orchestration (zzcollector-state, handoff protocol)
6. **Phase 5:** Implement clients (CLI/GUI, queries, real-time monitoring)
7. **Phase 6:** Feature parity validation, cutover, remove old code

**Each phase must be testable via mock transport before proceeding.**

---

## Success Criteria

The new stack is ready to replace the old when:

1. ✅ All components testable without network I/O
2. ✅ Mock-transport integration tests passing
3. ✅ Data submission with DESYNC recovery working
4. ✅ Handoff protocol demonstrated (manual test)
5. ✅ Historical queries working
6. ✅ Real-time monitoring working
7. ✅ 24+ hour stability test passed
8. ✅ Human verification: "This is better than the old stack"

Only then do we delete src/old/.

---

# FAQ

Q: In what order components must be created and wired?

A: Component bootstrap ordering and wiring IS showcased in the codebase, see:

* src/actors/zzintent-config/src/builder.rs
* src/actors/zzintent-config/src/test_integration.rs

---

Q: What happens if a collector offers rooms the database doesn't support?

A: **Partial match succeeds.** If intersection is non-empty, connection establishes with available rooms. Components without matched rooms simply have no child actors spawned - from their perspective, there's no connection. This is not an error.

Example:
- Collector offers: `["mem-db", "c-state", "intent-config"]`
- Database offers: `["mem-db", "c-state"]`
- Result: Connection succeeds with `["mem-db", "c-state"]`
- IntentConfig on both sides: no child spawned, no connection

For critical components that need all rooms, use `require_all_rooms(true)` flag in `ClientBuilder`.

See: `CLARIFICATION_Room_Negotiation.md` for details.

---

Q: How do components handle connection loss and reconnection?

A: Components use **per-connection child actors**. Each connection spawns a child actor that owns the `SessionHandle`. When connection dies, child is destroyed and all its state is cleared. On reconnect, a fresh child is spawned and state must be renegotiated from scratch.

**Parent component** (e.g., CState):
- Spawns child actors per connection
- Maintains `HashMap<PeerId, ChildActor>` (doubles as connection list)
- Sends messages to children, children forward to network

**Child actor**:
- Owns SessionHandle for one connection
- Only entity that can send/receive on that connection
- Dies with connection, no state preservation

**Fire-and-forget guarantees:**
- ✅ Ordering within room (while connected)
- ✅ TCP delivery (if connection exists)
- ❌ No ACKs, no retries
- ❌ Connection loss = message loss (acceptable)

See: `CLARIFICATION_Per_Connection_Actor_Pattern.md` for details.

---

Q: Are messages ordered across different rooms?

A: **No ordering guarantees across rooms.** Messages within a single room are FIFO-ordered (while connected), but messages sent to different rooms may arrive in any order.

**Why this is not a problem:**
- Components are designed to be **self-sufficient**
- Each component operates independently on its own room
- No component should have ordering dependencies on another component's messages

Example:
- Message A sent to "mem-db" room
- Message B sent to "c-state" room
- B might arrive before A (non-deterministic)
- Both components handle their messages independently
- No cross-component coordination needed

**Design principle:** If you need ordering between two message types, they belong in the same room (same component).

See: `CLARIFICATION_Cross_Room_Message_Ordering.md` for details.

---

Q: How does backpressure work in the system?

A: Backpressure propagates naturally through bounded mailboxes and TCP buffers.

**Component Level:**
- All component mailboxes have **bounded capacity** (typically 3 messages)
- When mailbox is full, sender is slowed down (blocked/backpressure)
- MemDB buffer is **limited by message count** - old messages are pruned when limit reached

**Network Level:**
- TCP send buffers fill when receiver is slow
- Full TCP buffer creates backpressure on SessionManager
- SessionManager backpressure affects **all rooms** on that connection (per-connection, not per-room)

**End-to-End Flow:**
```
Pinger (fast) → MemDB mailbox full → Pinger slows down
MemDB (fast) → TCP buffer full → MemDB slows down
Database (slow) → TCP receive buffer full → Collector TCP send buffer fills
```

**Design Principle:** Backpressure propagates to the source, slowing down producers when consumers can't keep up. This is handled naturally by Actix mailbox bounds and TCP flow control - no custom backpressure protocol needed.

**When buffers are full:**
- ✅ Pinger slows down (desired behavior)
- ✅ Old memdb data is pruned (acceptable loss)
- ✅ TCP buffers provide short-term buffering
- ❌ No unbounded growth (prevents OOM)

---

Q: How will message versioning and protocol evolution work?

A: **Two complementary strategies** for forward/backward compatibility:

**Strategy 1: Self-Describing Serialization (MessagePack + Serde)**
- MessagePack is self-describing (includes type information)
- Serde supports field aliasing and optional fields
- New fields can be added without breaking old receivers
- Old fields can be deprecated gracefully

Example:
```rust
// v1 message
struct PingDataBatch {
    cursor: u64,
    data: Vec<PingResult>,
}

// v2 message (backward compatible)
struct PingDataBatch {
    cursor: u64,
    data: Vec<PingResult>,
    #[serde(default)]  // Old senders won't include this
    compression: Option<CompressionType>,
}
```

**Strategy 2: Multiple Rooms Per Component**
- One component can handle multiple room versions
- Different rooms = different protocol versions
- Example: `"mem-db-v1"` and `"mem-db-v2"` rooms
- Component converts between versions internally

Example:
```rust
// Component offers both rooms
component.rooms(&["mem-db-v1", "mem-db-v2"]);

// Receives from v1 room, converts to v2 format
fn handle_v1(msg: PingDataBatchV1) {
    let v2 = msg.into();  // Convert to internal format
    self.process(v2);
}
```

**Why both strategies?**
- Strategy 1: Simple field additions (most common case)
- Strategy 2: Breaking changes (rare but necessary)
- Similar to how ZzNet separates HELLO protocol from Room protocol

**Status:** Open for analysis - exact patterns will emerge during implementation. MessagePack provides flexibility for evolution without premature specification.

---

Q: How do components get their configuration parameters?

A: **Mix of config file, source code constants, and runtime generation** - chosen based on what makes sense for each parameter.

**Static Config File** (same file with hostname and database address):
- TcpLock port number
- MemDB max buffer size
- Database connection details
- App main() parses and passes to component builders

Example:
```rust
// collector.ron
CollectorConfig {
    hostname: "collector-01",
    database_address: "database:9001",
    tcp_lock_port: 9100,
    memdb_max_buffers: 10000,
}

// In main()
let config = load_config("collector.ron")?;
let tcp_lock = TcpLockComponent::new(config.tcp_lock_port);
let memdb = MemDBComponent::new(config.memdb_max_buffers);
```

**Source Code Constants**:
- Default timeouts
- Retry intervals
- Magic numbers
- Anything that rarely/never changes

**Runtime Generation**:
- Per-connection identifiers (e.g., CState connection tracking)
- Options: process ID, random ID, TCP source port, sequence number
- Specific choice intentionally left undefined (implementation detail)

**Design Principle:** Configuration comes from the layer that makes sense - don't force everything into one mechanism. App main() acts as the composition root, reading config and wiring components.

---

Q: What happens to component state when a connection dies?

A: **Already documented** - see the per-connection child actor pattern.

**Quick summary:**
- Parent component spawns **child actor per connection**
- Child owns SessionHandle and all connection-specific state
- Connection dies → child actor destroyed → state cleared
- Reconnection → fresh child spawned → state renegotiated from scratch

**Distinction:**
- **Parent component** = Long-lived, survives disconnects (equivalent to old TaskSupervisor)
- **Child actor** = Ephemeral, dies with connection (equivalent to old SessionHandler)

See: `CLARIFICATION_Per_Connection_Actor_Pattern.md` for complete details on lifecycle, state management, and message passing.

---

Q: How do GUI clients get real-time monitoring data?

A: **Clients subscribe to "mem-db" room and receive ALL data** - no filtering, no sampling.

**What clients receive:**
- ✅ All ping results from all collectors
- ✅ All in-flight pings (probes sent, not yet returned)
- ✅ All hosts being monitored
- ✅ Real-time stream with no delay

**Subscription model:**
- Subscribe = Get everything
- Don't subscribe = Get nothing
- No partial filtering, no "show me only host X"

**For historical/aggregated queries:**
- Different mechanism (queries, not subscriptions)
- Aggregation done on database side
- Returns summarized data for past time ranges
- Details to be covered in query design

**Why no filtering?** Volume analysis shows several orders of magnitude of margin even in worst-case scenarios. The data rate is not overwhelming - simpler to send everything than to build a complex filtering system.

**Design principle:** Real-time monitoring = full firehose. Filtering happens in the GUI for display purposes only.

---

Q: How does disk I/O work with the actor model?

A: **Disk I/O is batched, infrequent, and can block** - this is acceptable given the write pattern.

**Write Pattern:**
- Writes happen on **wall-clock minute-aligned chunks**
- Data is **heavily compressed** using custom algorithm (from old gRPC version)
- Ping results are **delayed 1-2 minutes** before writing (for efficient batching)
- Writes are infrequent enough that blocking is not a problem

**Actor Integration:**
- Use Actix **fire-and-forget** sends to disk writer
- If mailbox is full, **block and log** (indicates serious problem)
- Actix can spawn actors on **separate threads** - disk I/O runs on its own thread
- No need for async I/O (blocking is fine given low frequency)

**State Tracking:**
- MemDB tracks "what's persisted" as **timestamp per collector*host**
- This timestamp is slightly behind what's actually on disk (conservative)
- MemDB focuses on **real-time and recent data** (hours in memory)

**If Disk Fails:**
- Everything backs up (acceptable - disk full/failure is catastrophic)
- System should fail-fast rather than continue with data loss

**Open Question:** Should disk I/O be a separate component? This is an interesting architectural question to explore during implementation. Current thinking: possibly yes, but defer decision.

**Historical Queries:** Reading from disk for queries might be handled by separate component or MemDB child actor - to be determined.

---

Q: How are message send errors handled?

A: **Bounded mailboxes with backpressure** for intra-process, **kick connection** for inter-process failures.

**Intra-Process (Component to Component):**
- All component mailboxes are **bounded and small** (typically 3 messages)
- When mailbox is full, sender **blocks** (backpressure propagates naturally)
- Components must **slow down** when they can't send
- No retries, no dropping - sender waits until receiver catches up

**Inter-Process (Over ZzNet):**
- TCP provides delivery guarantees (messages reach destination)
- **Parse/deserialization failures** = fatal error
- Failed parse → **kick connection immediately** (close socket)
- Connection close → child actor destroyed → state cleared
- Reconnection → fresh child spawned → state renegotiated

**Error Handling Philosophy:**
- Intra-process: Backpressure (wait, don't drop)
- Inter-process: Fail-fast (kick connection, don't continue with bad state)
- Component panics: Whole app crashes (components have static lifetime)
- Parse errors: Connection-level failure (isolate bad peer)

**Why kick on parse failure?** A message that can't be parsed indicates version mismatch, corruption, or bugs. Continuing with undefined state is dangerous - better to close connection, force reconnection, and renegotiate from clean state.

**Design principle:** Errors propagate as backpressure (slow down) or connection failures (reconnect). Never silently drop data or continue with corrupted state.

---

Q: How are components tested in isolation and integration?

A: **Mock other components using `Receiver<T>`** for unit tests, optionally use full integration tests without network.

**Unit Tests (Preferred):**
- Components should expose `Receiver<T>` for their outputs
- Tests create mock receivers and feed messages according to contract
- Example:
```rust
#[test]
fn test_pinger_sends_results() {
    let (tx, rx) = mpsc::channel();
    let pinger = PingerComponent::new(config)
        .with_memdb_sink(tx)  // Mock MemDB as channel
        .start();

    pinger.ping_now("8.8.8.8");

    let result = rx.recv().unwrap();
    assert_eq!(result.target, "8.8.8.8");
}
```

**Integration Tests (Optional):**
- Wire real components together (no network/TCP)
- Tests full component interaction without transport layer
- More thorough but more complex
- Less concern than unit tests (unit tests with mocks are sufficient)

**Network Tests:**
- Use mock transport (in-memory channels, no TCP)
- Already covered in "mock-first development" testing philosophy
- Mock transport validates transport-agnostic design

**Design Principle:** Components are designed for testability - outputs via `Receiver<T>` make mocking trivial. Contract-based testing with mocks is preferred over complex integration tests.

---

Q: How does graceful shutdown work?

A: **Wait for disk writes, tear down everything else immediately.**

**On SIGTERM:**
1. Signal disk writer to finish current write (typically <1 second)
2. Wait for disk write completion
3. Exit - all other components can be torn down immediately

**What doesn't need graceful handling:**
- ❌ Network connections - just close (reconnect is normal)
- ❌ In-memory buffers - acceptable loss (1-2 minutes of data)
- ❌ In-flight messages - fire-and-forget semantics make this safe
- ❌ Child actors - automatically cleaned up on connection close

**What needs protection:**
- ✅ In-progress disk writes - must complete to avoid corruption

**Design principle:** Fire-and-forget architecture makes shutdown trivial. Only disk I/O requires coordination. Data loss on crash is acceptable (minutes of buffer at most).

---

Q: How do components handle authentication and authorization?

A: **Components see permissions only** (not roles or certificates). Auth abstraction handled by zznet-auth, enforcement at connection level.

**Architecture Layers:**
```
mTLS (Transport) → Role (zznet-auth) → Permissions (Component)
```

**Component Perspective:**
- Components declare what **permissions** they require
- Components never see raw roles or certificate data
- Child actors store permissions **per-connection** (simple local state)
- Connection overstepping permissions → **kick immediately** + log error

**Wiring (in main.rs):**
- Components declare: "I need WriteData permission"
- main.rs wires: "Collector role → has WriteData permission"
- zznet-auth translates role → permissions at connection time
- mTLS validates certificate and extracts role

**Per-Connection Authorization:**
- Each child actor receives permissions on spawn
- Child checks permissions before processing messages
- Example:
```rust
// Child actor state
struct CStateConnectionActor {
    peer_id: PeerId,
    permissions: HashSet<Permission>,  // e.g., {WriteData, ReadConfig}
    session_handle: SessionHandle,
}

// Message handling
fn handle_config_change(&self, msg: ConfigUpdate) {
    if !self.permissions.contains(&Permission::WriteConfig) {
        // Kick connection immediately
        self.session_handle.close();
        error!("Peer {} attempted unauthorized config write", self.peer_id);
        return;
    }
    // ... process message
}
```

**Metadata for Logging:**
- Child actors can access PeerIdentity strings for debugging
- Role as string (e.g., "Collector")
- Username as string (e.g., "admin")
- These are for **logging only**, not authorization decisions

**Design Principle:** Complete abstraction - components work with permissions, never raw auth data. Authorization is per-connection (stored in child actor), making enforcement simple and local.

---

# Appendix: Clarification: Collector Identity (No UUIDs!)

**Purpose**: Resolve confusion about collector identity caused by AI-generated UUID references

## The Confusion

Multiple design documents mention "UUID" for collector identity. This is **incorrect** and was introduced by AI agents during documentation generation.

**There is NO UUID in ZZPing architecture.**

## The Reality: Hostname IS the Identity

### Collector Identity = Hostname

A collector is identified by its **hostname**, which is:
- A human-readable string (e.g., `"collector-01"`)
- Configured in the collector's static config file
- Sent in the HELLO handshake when connecting
- The same as `PeerIdentity` in ZzNet

### Where Hostname Comes From

**Preferred**: Static configuration file
```ron
// collector.ron
hostname: "collector-01",
database_address: "database.example.com:9001",
```

**Alternatives considered**:
- `gethostname()` crate for auto-detection (rejected as more trouble than it's worth)
- Auto-generate during installation (rejected as confusing)

**Decision**: Explicit configuration in config file is clearest.

---

## How Identity Works in ZzNet

### 1. HELLO Handshake

When a collector connects to the database:

```rust
// From ZZPing_Network_Layer_Actor_Design_Oct2025.md
struct HelloFrame {
    protocol_family: String,  // "zznet"
    protocol_version: String, // "1.0"
    hostname: String,         // "collector-01" ← THE IDENTITY
    role: AuthRole,           // Collector
}
```

The hostname is sent **within the HELLO frame itself**, not in a subsequent message.

### 2. Trust and Validation

**For regular collectors and databases**: mTLS certificate validation is sufficient. The hostname in HELLO is trusted after TLS handshake completes.

**For ClientRO guests**: Could enforce `username == hostname` or add hostname to certificate fields to prevent forgery. This is overkill for service-to-service communication.

### 3. PeerIdentity in SessionManager

After HELLO completes, the HELLO Handler creates a `PeerIdentity`:

```rust
struct PeerIdentity {
    hostname: String,  // From HelloFrame
    role: AuthRole,    // From HelloFrame
    // ... other fields ...
}
```

This `PeerIdentity` is handed to SessionManager, which uses it for all subsequent operations.

---

## Handoff Scenario: Same Hostname = Same Collector

### The Scenario

```
C1 connects: hostname="collector-01", process_id=123
Database creates PeerSession for C1

C1 disconnects (upgrade)

C2 connects: hostname="collector-01", process_id=456
Database creates NEW PeerSession for C2
```

### How Database Knows C1 and C2 Are Related

The database's **CState component** maintains a mapping:

```rust
// Conceptual (actual implementation deferred)
struct CollectorState {
    hostname: String,              // "collector-01"
    connections: Vec<SessionHandle>, // [C1_session, C2_session]
    role: CollectorRole,           // PRIMARY, STANDBY, SUPERVISING
}
```

When two connections share the same hostname:
- **Same hostname = same logical collector**
- Different SessionHandles = different processes
- CState can track both connections and orchestrate handoff

### Implementation Details: Deferred

The exact mechanism for correlating connections at the database level is **intentionally not specified**. Options include:

- `HashMap<Hostname, Vec<SessionHandle>>`
- `HashMap<SessionHandle, (Hostname, ConnectTime)>` with reverse lookup
- CState component spawning child actors per connection
- Many other viable approaches

**Decision**: Defer to implementation. If `Vec<Connection>` is insufficient, we'll address it then. No need to over-specify now.

---

## What About Certificates?

### Current Design

**Certificate fields**:
- CN (Common Name) = Role (`Collector`, `Database`, `ClientAdmin`, `ClientRO`)
- SAN (Subject Alternative Name) = Username (for client certificates)

**Hostname is NOT in the certificate**. It's sent in the HELLO frame.

### Could We Add Hostname to Certificate?

**Yes**, but it's **overkill** for service-to-service communication:
- Collectors and databases are trusted infrastructure components
- mTLS already validates role (CN)
- Adding hostname validation would complicate certificate generation
- Benefit is minimal for the added complexity

**For ClientRO**: We could enforce `username == hostname` if needed for guest security, since username is already in the SAN.

**Decision**: Current design (hostname in HELLO, not in cert) is sufficient.

---

## Summary: Key Points

1. ✅ **Hostname is the identity** (not UUID)
2. ✅ **Hostname comes from config file** (explicit, not auto-generated)
3. ✅ **Hostname is in HELLO frame** (part of the handshake)
4. ✅ **Same hostname = same collector** (handoff correlation)
5. ✅ **PeerIdentity uses hostname** (not a separate UUID field)
6. ✅ **Implementation details are deferred** (no premature complexity)

---

## Action Items

### Documentation Cleanup

Search for and remove all references to "UUID" in the following documents:
- `ZZPing_Collector_Database_Migration_Zznet.md`
- `ZZPing_Architectural_Vision_II.md`
- `ZZPing_Collector_Software_Architecture_Design.md`
- `ZZPing_Collector_Architectural_Requirements.md`
- `ZZPing_Network_protocol.md`

Replace with "hostname" or "collector identity" as appropriate.

### Code Review

Check if any existing code uses `collector_uuid` or similar:
- Rename to `hostname` or `collector_hostname`
- Update comments and documentation
- Verify HELLO handler implementation uses `hostname` field

---

## Why This Matters

**Clarity**: "UUID" implies auto-generation, uniqueness guarantees, and specific formats (UUIDv4). "Hostname" is clear: it's a configured name.

**Simplicity**: No need for UUID generation, persistence, or lookup logic. Config file has the name, HELLO sends it, done.

**Human-Friendly**: "collector-01" is more meaningful than "a3b2c1d4-e5f6-7890-abcd-ef1234567890" in logs and debugging.

**Avoids Confusion**: Multiple AI agents introduced UUID independently, causing documentation to conflict. This clarification stops the spread.

# Appendix: Clarification: Per-Connection Child Actor Pattern

**Purpose**: Document the per-connection child actor pattern for component-to-network integration

---

## The Core Pattern

**Components spawn child actors, one per connection.**

```
Component Architecture (Database Side):

┌─────────────────────────────────────────────┐
│         CState Component (Parent Actor)     │
│                                             │
│  - Manages overall state                   │
│  - Spawns child actors per connection      │
│  - Maintains list of children              │
│  - Does NOT send/receive network directly  │
│                                             │
│  ┌──────────────┐  ┌──────────────┐       │
│  │  Child for   │  │  Child for   │       │
│  │ Collector-01 │  │ Collector-02 │  ...  │
│  └──────────────┘  └──────────────┘       │
└─────────────────────────────────────────────┘
         │                  │
         │ SessionHandle    │ SessionHandle
         ↓                  ↓
    Connection 1       Connection 2
```

**Key Points:**
- ✅ Parent component spawns **one child actor per connection**
- ✅ Child actor is the **only** entity that sends/receives on that connection
- ✅ Parent maintains a **list of child actors** (implicitly a connection list)
- ✅ Parent communicates with children via **intra-process messages**

---

## Connection Lifecycle and State Management

### When Connection Established

```rust
// Pseudo-code: CState parent actor receives SessionEvent
impl Handler<SessionEvent> for CState {
    fn handle(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Active { peer_id, session_handle, role, ... } => {
                // Spawn child actor for this connection
                let child = CStateConnectionActor::new(
                    peer_id.clone(),
                    session_handle,
                    role,
                ).start();

                // Track the child
                self.connections.insert(peer_id, child);

                // Child now handles all send/receive for this connection
            }
        }
    }
}
```

**What Happens:**
1. SessionManager notifies component of new connection via `SessionEvent::Active`
2. Component spawns **child actor** for that connection
3. Child receives `SessionHandle` - it's the only one that can use it
4. Parent tracks child in internal list/map

### When Connection Dies

```rust
impl Handler<SessionEvent> for CState {
    fn handle(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Inactive { peer_id } => {
                // Remove and stop child actor
                if let Some(child) = self.connections.remove(&peer_id) {
                    // Child actor is stopped (dropped)
                    // All its state is destroyed
                }

                // No cleanup needed - child took care of itself
            }
        }
    }
}
```

**What Happens:**
1. Connection dies (network failure, remote close, etc.)
2. SessionManager sends `SessionEvent::Inactive` to component
3. Component **removes child actor** from its list
4. Child actor is **destroyed** - all its state is gone
5. Any in-flight messages are **lost** (acceptable - TCP guarantees were broken)

**Critical Principle:** Connection dies = Child actor dies = State is cleared.

### When Reconnection Happens

**Important:** Reconnection is treated as a **new connection**, even if it's the same remote process.

```rust
// Same collector reconnects
// peer_id might be the same ("collector-01")
// But this is a NEW SessionHandle, NEW child actor

SessionEvent::Active {
    peer_id: "collector-01",  // Same hostname
    session_handle: new_handle,  // Different handle!
    ...
}

// Component spawns a FRESH child actor
// NO state is carried over from old connection
// State must be renegotiated via messages
```

**Why?**
- Old connection had TCP buffers, pending messages, etc.
- That state is lost - no point pretending otherwise
- Explicit renegotiation is more reliable than trying to "resume"

---

## Fire-and-Forget Guarantees

### What "Fire-and-Forget" Means

**Guarantees:**
1. ✅ **Ordering within a room**: Messages sent to same room arrive in order
2. ✅ **TCP delivery**: If connection exists, message is eventually delivered
3. ✅ **No ACKs**: Sender doesn't know if message was received
4. ✅ **No retries**: Component doesn't retry failed sends

**Non-Guarantees:**
1. ❌ **No delivery confirmation**: Sender never knows if message arrived
2. ❌ **Connection loss = message loss**: Disconnect mid-send loses that message
3. ❌ **No at-least-once**: A disconnect can lose messages (at-most-once semantics)
4. ❌ **No deduplication**: Reconnect might cause re-sends if app logic retries

### Message Loss on Disconnect

```rust
// Child actor sends message
child.session_handle.send_to_room(room_id, message);
// Message goes into TCP send buffer

// Connection dies mid-transmission
// Result: Message is LOST

// Child actor is destroyed
// No retry, no buffering, no recovery
// Parent must handle this via application logic
```

**Acceptable because:**
- TCP provides ordering and delivery **while connected**
- Application layer handles **reconnection state sync**
- Components designed to survive message loss (1+ hour partition tolerance)

### State Renegotiation After Reconnect

```rust
// Example: CState after reconnect

// Old connection child actor (destroyed):
// - Had knowledge of collector's role: PRIMARY
// - Had pending status reports
// All lost.

// New connection child actor (fresh):
// - Knows nothing about collector's previous state
// - Database CState must send: "What's your current role?"
// - Collector responds: "I'm PRIMARY with these targets"
// - State is rebuilt from scratch
```

**Pattern:** Every reconnect triggers full state sync negotiation.

---

## Component Message Passing

### Parent → Child Communication

**Parent cannot send directly to network.** Parent must ask child to send.

```rust
// Parent actor (CState)
impl CState {
    fn broadcast_config_update(&mut self, config: Config) {
        // Parent wants to send to all collectors
        // Must go through child actors

        for (peer_id, child) in &self.connections {
            // Send message to child actor
            child.do_send(SendToCollector {
                room: "intent-config",
                message: ConfigUpdate { config: config.clone() },
            });
        }
    }
}

// Child actor (CStateConnectionActor)
impl Handler<SendToCollector> for CStateConnectionActor {
    fn handle(&mut self, msg: SendToCollector) {
        // Child has the SessionHandle
        self.session_handle.send_to_room(msg.room, msg.message);
    }
}
```

**Why this indirection?**
- SessionHandle is owned by child (one per connection)
- Parent doesn't have access to SessionHandle
- Forces explicit per-connection logic

### Child → Parent Communication

**Child receives network messages, forwards to parent for business logic.**

```rust
// Child actor receives message from network
impl Handler<NetworkMessage> for CStateConnectionActor {
    fn handle(&mut self, msg: NetworkMessage) {
        match msg {
            NetworkMessage::RoleStatusReport { role, targets } => {
                // Forward to parent for processing
                self.parent.do_send(CollectorStatusUpdate {
                    peer_id: self.peer_id.clone(),
                    role,
                    targets,
                });
            }
        }
    }
}

// Parent processes business logic
impl Handler<CollectorStatusUpdate> for CState {
    fn handle(&mut self, msg: CollectorStatusUpdate) {
        // Update internal state
        self.collector_states.insert(msg.peer_id, msg.role);

        // Maybe trigger handoff logic, etc.
    }
}
```

**Why this pattern?**
- Child handles connection-specific logic (send/receive)
- Parent handles business logic (state management, decisions)
- Clean separation of concerns

---

## Simplified Components (No Per-Connection State)

**Not all components need per-connection children.**

### Example: IntentConfig on Collector (Client Side)

```rust
// Collector has ONE connection to database
// IntentConfig doesn't need per-connection children

impl IntentConfigComponent {
    fn handle_network_message(&mut self, msg: IntentConfigMessage) {
        match msg {
            IntentConfigMessage::ConfigUpdate { targets, ping_rate } => {
                // Update local state
                self.current_config = targets;

                // Broadcast to Pinger (intra-process)
                self.pinger.do_send(UpdateConfig { targets, ping_rate });
            }
        }
    }
}
```

**Why simpler?**
- Collector is a **client** (one connection)
- Component doesn't manage multiple peers
- No need for per-connection state tracking

### Example: IntentConfig on Database (Server Side)

```rust
// Database has MANY connections from collectors
// IntentConfig needs to track which collectors to notify

impl IntentConfigComponent {
    // Parent actor maintains connection list
    connections: HashMap<PeerId, Addr<IntentConfigConnectionActor>>,

    fn handle_config_change(&mut self, new_config: Config) {
        // Persist config
        self.persist(new_config.clone());

        // Broadcast to all connected collectors
        for (peer_id, child) in &self.connections {
            child.do_send(SendConfigUpdate {
                config: new_config.clone(),
            });
        }
    }
}
```

**Why more complex?**
- Database is a **server** (many connections)
- Needs to track which collectors are online
- Needs per-connection children to send to each

---

## Summary: The Pattern

### Per-Connection Child Actor Pattern

1. ✅ **Component spawns child actor per connection**
   - Child receives `SessionHandle`
   - Child is sole sender/receiver for that connection

2. ✅ **Parent maintains list of children**
   - `HashMap<PeerId, Addr<ChildActor>>`
   - List doubles as "who's connected" tracker

3. ✅ **Connection dies → Child destroyed**
   - All state for that connection is lost
   - No cleanup needed (child handles it)

4. ✅ **Reconnection = fresh start**
   - New child actor spawned
   - State renegotiated from scratch

5. ✅ **Parent ↔ Child via messages**
   - Parent cannot send to network directly
   - Parent sends messages to child
   - Child forwards to network via SessionHandle

6. ✅ **Fire-and-forget semantics**
   - Ordering guaranteed within room (while connected)
   - No ACKs, no retries
   - Disconnect loses in-flight messages (acceptable)

### When to Use This Pattern

**Use per-connection children when:**
- Component is server-side (many connections)
- Component needs per-connection state
- Example: CState, MemDB on database

**Don't use when:**
- Component is client-side (one connection)
- Component is stateless
- Example: IntentConfig on collector

---

## Open Questions (Deferred to Implementation)

1. **Child actor lifecycle details**: How is child notified to stop? Actix stop message? Drop on remove?
2. **State sync protocol**: What messages are used to renegotiate state after reconnect?
3. **Error handling**: What if child actor panics? Does parent restart it?
4. **Testing**: How to mock per-connection children for unit tests?

These are **implementation details** that will emerge during development.
