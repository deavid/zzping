# ZZPing v0.3: Vision & Philosophy

- **Status:** Authoritative / Active
- **Date:** November 2025
- **Context:** This document supersedes all previous "Architectural Vision," "Requirements," and "Design" documents from
  the v0.2 (gRPC) era.

---

## 1. The Core Mission

ZZPing is a distributed system designed to create a **high-fidelity historical journal of network quality**.

Unlike ephemeral ping tools (like `ping` or `mtr`) which are designed for real-time debugging, ZZPing is designed for
**long-term observability**. Its primary value proposition is the ability to answer: _"Was my internet bad at 3:00 AM
last Tuesday?"_

To achieve this, the architecture prioritizes **Data Integrity** and **Operational Resilience** above all else.

## 2. The Architectural Pivot: From Monolith to Actors

The transition from v0.2 (gRPC) to v0.3 (ZzNet) represents a fundamental shift in how we build the system.

### The Old Way (v0.2 - gRPC)

- **Model:** Monolithic Service.
- **Structure:** Hierarchical ownership (`Collector` -> `Supervisor` -> `Worker`).
- **Communication:** RPC calls (`client.send_batch()`).
- **Problem:** Tight coupling. Testing required spinning up the whole world. Network state (gRPC streams) was tightly
  bound to application state (buffers).

### The New Way (v0.3 - ZzNet)

- **Model:** Pure Actor Model.
- **Structure:** Isolated, autonomous Components (`MemDB`, `Pinger`, `IntentConfig`).
- **Communication:** Asynchronous Message Passing (Intra-process channels, Inter-process Rooms).
- **Benefit:**
  - **Testability:** Every component can be tested in isolation with mock channels.
  - **Resilience:** Application state (buffers) is decoupled from Network state (TCP sockets).
  - **Symmetry:** The code running on the Collector and the Database is the same component, just configured with a
    different Role.

## 3. The Architectural Axioms

These are the non-negotiable rules that govern every design decision in ZZPing v0.3.

### Axiom 1: The Journal is Truth

**"If it is not stored, it didn't happen."**

We do not generate traffic for the sake of traffic. We generate traffic to measure the network.

- **Implication:** If the recording pipeline (`MemDB` -> Network -> Database -> Disk) is clogged or broken
  (backpressure), we **STOP PINGING**.
- **Anti-Pattern:** Dropping result packets to keep the pinger running. This creates "Ghost Data" (events that happened
  in reality but aren't in the journal), which destroys user trust.

### Axiom 2: The Doctrine of Obedience

**"The Pinger is Dumb. The User is Smart."**

We explicitly reject "adaptive" logic in the collector.

- **No Auto-Scaling:** If the user configures 30 pps, we send 30 pps. Even if packet loss is 100%. Even if latency is
  5000ms.
- **Why?** Slowing down during an outage masks the severity of the problem. "100% loss at 30Hz" is valuable data. "1
  sample at 1Hz" (because we backed off) is corrupted data.

### Axiom 3: Component Symmetry

**"Components talk to themselves."**

We do not write asymmetric "Client" and "Server" code for business logic.

- **The Rule:** `MemDB` on the Collector talks to `MemDB` on the Database via the `memdb` room.
- **The Benefit:** All protocol logic for a specific domain lives in one crate. Protocol symmetry is enforced by design.

### Axiom 4: Fail-Static / Fail-Stop

**"Better to crash than to lie."**

- **Network Failure:** If the network disconnects, we buffer (Fail-Static). We do not crash. We assume the network is
  hostile.
- **Internal Failure:** If an internal invariant is violated (e.g., `MemDB` actor dies, or a channel is closed
  unexpectedly), the process should **CRASH** (Fail-Stop). We rely on process supervisors (systemd/docker) to restart us
  in a clean state rather than trying to "heal" a corrupted process.

## 4. The "Simplicity Doctrine"

We solve complex distributed problems by simplifying the requirements, not by adding complex code.

1. **Identity:** We use **Shared mTLS Certificates** for authorization (simplifying deployment) but **Unique
   Configuration** for identity (simplifying logic).
2. **Data Stream:** We treat "In-Flight" events and "Ping Results" as a **Single Unified Stream**. We do not build
   complex priority queues to save one at the expense of the other. If the pipe is full, everything stops.
3. **Configuration:** There is **One Global Config**. We do not calculate per-collector sharding in the database. Every
   collector receives the full intent and executes it.

## 5. Summary

ZZPing v0.3 is built to be:

- **Dumb** where it counts (Pinger execution).
- **Smart** where it matters (State isolation, Mock-first testing).
- **Honest** above all else (Backpressure halts execution).
