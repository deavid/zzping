# ZZPing v0.3: Network Architecture (ZzNet)

- **Status:** Authoritative / Active
- **Date:** November 2025
- **Dependency:** `00_Vision_and_Philosophy.md`

---

## 1. The "Room" Concept

The core abstraction of ZzNet is the **Room**.

It is critical to unlearn the "IRC/Chat Room" mental model. In ZzNet:

> **A "Room" is a 1:1, bidirectional, typed channel between two specific component instances.**

### The Topology

- **One TCP Connection:** Each Collector opens exactly ONE TCP connection to the Database.
- **Multiplexed Rooms:** Over this single connection, multiple logical "Rooms" operate in parallel.
- **Isolation:** A message sent to the `memdb` room will NEVER be seen by the `intent-config` handler.

### Example

If `Collector-A` is connected to `Database-1`:

1. `MemDB` (Collector) <--> `memdb` Room <--> `MemDB` (Database)
2. `IntentConfig` (Collector) <--> `intent-config` Room <--> `IntentConfig` (Database)
3. `CState` (Collector) <--> `cstate` Room <--> `CState` (Database)

All three streams share one TCP socket, but they are logically distinct.

## 2. The Layer Cake

The network stack is strictly layered to ensure testability.

```text
┌───────────────────────────────────────────────────────┐
│  1. Application Component (e.g., MemDB)               │
│     - Pure Business Logic                             │
│     - Sends/Receives Rust Structs (e.g. SubmitBatch)  │
│     - Knows NOTHING about TCP, Bytes, or Sockets      │
└───────────────────────────┬───────────────────────────┘
                            │ (Typed Message)
┌───────────────────────────▼───────────────────────────┐
│  2. SessionManager / NetworkActor                     │
│     - Routes messages to the correct Room             │
│     - Handles Request/Reply pairing                   │
│     - Pure Actor Logic (Testable in memory)           │
└───────────────────────────┬───────────────────────────┘
                            │ (Typed Message)
┌───────────────────────────▼───────────────────────────┐
│  3. RoomActor<T> (The Serialization Boundary)         │
│     - Serializes Struct -> Bytes (MessagePack)        │
│     - Deserializes Bytes -> Struct                    │
│     - The ONLY layer that knows about serialization   │
└───────────────────────────┬───────────────────────────┘
                            │ (TransportFrame)
┌───────────────────────────▼───────────────────────────┐
│  4. Transport Layer (zznet-transport-tcp)             │
│     - Handles Framing (Length Prefixing)              │
│     - Handles mTLS Encryption                         │
│     - Writes/Reads from Socket                        │
└───────────────────────────────────────────────────────┘
```

**Golden Rule:** If you can't swap Layer 4 (TCP) for a Mock Channel without changing Layer 1 (Component), the
architecture is broken.

## 3. Connection Lifecycle

### Step 1: The HELLO Handshake

Before any application data flows, a metadata handshake occurs at the Transport layer.

- **Collector Sends:**
  - `Protocol Version`
  - `Role` ("collector")
  - `Hostname` ("living-room-pi") <- **This is the Identity**
  - `Offered Rooms` (["memdb", "cstate", "intent-config"])
- **Database Responds:**
  - `Offered Rooms` (["memdb", "cstate", "intent-config", "admin"])

### Step 2: Room Intersection

The system computes the intersection of offered rooms.

- **Result:** `["memdb", "cstate", "intent-config"]`.
- **Action:** The `SessionManager` automatically "spawns" the handlers for these rooms.
- **Note:** The `admin` room is NOT spawned because the Collector didn't offer it. This negotiation happens
  automatically at connection time.

## 4. Identity Model

ZzNet separates **Authorization** from **Identification**.

### A. Authorization (Who are you allowed to be?)

- **Mechanism:** mTLS Certificates.
- **Scope:** Broad Roles.
- **Example:** All 50 collectors in your house share the **same** `client-collector.pem` certificate. This certificate
  proves "I am a valid Collector authorized to talk to the DB."

### B. Identification (Which specific instance are you?)

- **Mechanism:** The `Hostname` field in the HELLO handshake + Config File.
- **Scope:** Unique Instance.
- **Example:** `collector-01`, `collector-02`.
- **Usage:** This is what `CState` uses to detect if "Collector-01" is connecting a second time (triggering a Handoff).

## 5. Failure & Recovery

### The "Clean Slate" Rule

**A TCP Disconnect is a State Reset.**

If the connection drops:

1. The `SessionManager` tears down all Room actors.
2. Local components are notified (if they subscribed to lifecycle events).
3. **Reconnection** acts as a brand new session. We do **not** try to "resume" a session or replay partial frames.
4. Components must re-sync state (e.g., `MemDB` sends its backlog, `IntentConfig` requests the latest config).

This simplifies the mental model: You are either **Connected** (and consistent) or **Disconnected** (and buffering).
