# ZZPing v0.3: Collector Component Map

- **Status:** Authoritative / Active
- **Date:** November 2025
- **Context:** This document defines the specific components that make up a running Collector process and how they are
  wired together.

---

## 1. Component Definitions

These are the five autonomous actors that live inside the Collector process.

### 1. `zzpinger` (The Engine)

- **Role:** High-precision execution engine.
- **Responsibility:** Wakes up on schedule, sends ICMP packets, and captures timestamps.
- **Behavior:** "Dumb and Obedient." It never makes decisions; it only follows orders.
- **Input:** Target list (from `IntentConfig`), Enable/Disable signal (from `CState`).
- **Output:** Raw Ping Events (to `MemDB`).

### 2. `zzmem-db` (The Buffer)

- **Role:** Reliability layer / Shock absorber.
- **Responsibility:** Holds recent data in RAM to survive network outages.
- **Behavior:** "Fail-Static." If the DB is down, it buffers. If RAM is full, it drops oldest data.
- **Input:** Raw Ping Events (from `Pinger`).
- **Output:** Batched Data (to Network).

### 3. `zzintent-config` (The Orders)

- **Role:** Configuration subscriber.
- **Responsibility:** Maintains the current target list and ping rate.
- **Behavior:** "Cache-First." Updates from the network, but persists to disk (`last_intent.ron`) so we can boot without
  the DB.
- **Input:** Config Updates (from Network).
- **Output:** Target Updates (to `Pinger`).

### 4. `zztcp-lock` (The Safety)

- **Role:** Local mutex.
- **Responsibility:** Ensures only one Collector instance runs per `Installation ID` on this machine.
- **Behavior:** "Greedy." Tries to bind the configured port. Reports success/failure.
- **Input:** None.
- **Output:** Lock Status (to `CState`).

### 5. `zzcollector-state` / `CState` (The Manager)

- **Role:** Mastership decision maker.
- **Responsibility:** Decides if this process is **Primary** (working) or **Standby** (waiting).
- **Behavior:** Synthesizes signals. "If I have the Lock AND the DB says go -> Start Pinger."
- **Input:** Lock Status (from `TCPLock`), Mastership Commands (from Network).
- **Output:** Enable/Disable signal (to `Pinger`).

---

## 2. The Wiring Diagram (Intra-Process)

Components are wired at startup (`main.rs`) via Dependency Injection. They communicate via local Actix messages.

```text
      (Network In)        (Local System)
           │                    │
           ▼                    ▼
  [IntentConfig Actor]   [TCPLock Actor]
           │                    │
           │ (1)                │ (2)
           │ "New Targets"      │ "Lock Acquired"
           │                    │
           ▼                    ▼
           │             [CState Actor]
           │                    │
           │                    │ (3)
           │                    │ "Enable Engine"
           ▼                    ▼
        [Pinger Actor] ◄────────┘
           │
           │ (4)
           │ "Result: 8.8.8.8 = 15ms"
           ▼
      [MemDB Actor] ─────────► (Network Out)
```

### The Conversation Flow

1. **Config Loop:** `IntentConfig` receives a network update. It tells `Pinger`: _"Here is the new schedule."_
2. **Safety Loop:** `TCPLock` tells `CState`: _"I have successfully grabbed port 7879."_
3. **Mastership Loop:** `CState` (seeing the lock is held) tells `Pinger`: _"You are authorized to run."_
4. **Data Loop:** `Pinger` executes. It tells `MemDB`: _"Here is a result."_ `MemDB` buffers it and sends it upstream.

## 3. Startup Sequence

The wiring order is critical to prevent race conditions.

1. **Create:** Instantiate all 5 Component Builders (inert).
2. **Wire:**
   - Give `Pinger` the address of `MemDB`.
   - Give `IntentConfig` the address of `Pinger`.
   - Give `CState` the address of `Pinger`.
   - Give `TCPLock` the address of `CState`.
3. **Start:** Launch all actors.
4. **Connect:** Start the Network Layer (ZzNet), enabling `IntentConfig`, `CState`, and `MemDB` to talk to the Database.
