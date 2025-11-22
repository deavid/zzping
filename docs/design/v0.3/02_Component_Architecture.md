# ZZPing v0.3: Component Architecture

- **Status:** Authoritative / Active
- **Date:** November 2025
- **Dependency:** `01_Network_Architecture.md`

---

## 1. The "Three-Actor" Pattern

Every networked component in ZZPing (e.g., `MemDB`, `IntentConfig`) is actually a cluster of three distinct actors
working in concert. This separation of concerns is mandatory.

### A. The MainActor (Business Logic)

- **Role:** The Brain.
- **State:** Holds the application state (e.g., the Config struct, the Ping Buffer).
- **Networking:** **Zero.** It knows nothing about TCP, Peers, or Sockets.
- **Communication:** It speaks via:
  - **Input:** Request/Reply messages (`handle(SubmitBatch) -> Result<Ack>`).
  - **Output:** Event Bus (`broadcast::Sender<Event>`).

### B. The NetworkManager (Lifecycle Supervisor)

- **Role:** The HR Department.
- **State:** None (Stateless supervisor).
- **Networking:** Knows _when_ a peer connects/disconnects, but not _what_ they say.
- **Responsibility:**
  - When a peer joins: Spawns a `NetworkActor` for them.
  - When a peer leaves: Stops the `NetworkActor`.
  - Passes the `MainActor`'s Event Bus to new `NetworkActor`s.

### C. The NetworkActor (Translator)

- **Role:** The Translator (One instance per connected peer).
- **State:** Peer-specific context (e.g., `PeerId`, `Permissions`).
- **Networking:**
  - **Inbound:** Receives typed Network Messages from the `RoomActor`. Translates them to MainActor requests (e.g.
    `NetworkMsg::Submit -> MainMsg::Submit`).
  - **Outbound:** Subscribes to the MainActor's Event Bus. Translates events to Network Messages and sends them to the
    `RoomActor`.

## 2. The Construction Lifecycle

Components are not just `new()`'d. They are assembled in phases to prevent startup race conditions.

### Phase 1: The Builder

```rust
// 1. Create the configuration
let config = MemDBConfig::for_collector(...);

// 2. Create the builder (Inert, no actors running)
let builder = MemDBBuilder::new(config);
```

### Phase 2: The Wiring

```rust
// 3. Connect dependencies (Dependency Injection)
// Note: This happens BEFORE the actor starts.
builder.set_router(router_addr);
```

### Phase 3: Activation

```rust
// 4. Launch (Consumes the builder)
let addr = builder.start();
```

## 3. Communication Rules

### Rule 1: Intra-Process (Reliable)

Components talking to components _in the same process_ use **Actix Messages** or **Tokio Channels**.

- **Backpressure:** Handled by bounded channel size. If the channel is full, the sender waits (or drops, depending on
  policy).
- **Reliability:** Guaranteed delivery if the process doesn't crash.

### Rule 2: Inter-Process (Unreliable)

Components talking to remote components use **ZzNet Rooms**.

- **Backpressure:** Handled by TCP buffers.
- **Reliability:** None. Fire-and-forget.
- **Pattern:**
  - **Push:** `MainActor` -> Event Bus -> `NetworkActor` -> Room.
  - **Pull/Request:** `NetworkActor` -> `MainActor` -> Result -> `NetworkActor` -> Room.

## 4. Error Handling Philosophy

### The "Fail-Fast" Rule

If a component encounters an invalid internal state (e.g., a channel closed unexpectedly, a mutex is poisoned), it
should **Panic**.

- **Why?** A corrupted component is dangerous. The OS Supervisor (systemd/docker) will restart the process cleanly.

### The "Network is Hostile" Rule

If a component encounters a network error (serialization failure, unauthorized request):

1. **Log it.**
2. **Drop the message.**
3. **Disconnect the peer.**
4. **NEVER Panic.**

## 5. Configuration: Roles vs. Props

We do not configure components with enums like `Role::Collector` or `Role::Database`. This violates SOLID principles.

Instead, we configure **Capabilities**:

```rust
// GOOD
struct MemDBConfig {
    accept_batches: bool,
    persistence_path: Option<PathBuf>,
}

// BAD
enum MemDBConfig {
    Collector,
    Database,
}
```

This makes components reusable in unexpected contexts (e.g., a "Relay" node that accepts batches but doesn't persist).
