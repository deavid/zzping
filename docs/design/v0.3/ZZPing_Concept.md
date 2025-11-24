# ZZPing: The Historical Network Journal

**Architectural Concept & Vision Definition**

## 1. The Core Mission

ZZPing is a distributed system designed to create a **high-fidelity, irrefutable historical journal of network
quality**.

Unlike ephemeral tools like `ping` or `mtr`, which are designed for real-time debugging ("Is it broken _now_?"), ZZPing
is designed for long-term observability ("Was it broken at 3:00 AM last Tuesday?"). Its primary goal is to provide the
data necessary to hold ISPs accountable for intermittent issues and packet loss.

To achieve this, the system prioritizes **Data Integrity** and **Operational Resilience** above availability. It follows
the maxim: _"Better to crash than to lie, better to stop than to record ghost data."_

## 2. The Philosophy of Truth

ZZPing operates under a strict set of architectural axioms that dictate its behavior:

### A. The Journal is Truth (Backpressure)

We do not generate traffic for the sake of traffic. We generate traffic to measure the network. If the recording
pipeline (Network -> Database -> Disk) is clogged or broken, the Pinger **stops scheduling**.

- **Why:** Dropping result packets to keep the pinger running creates "Ghost Data" (events that happened in reality but
  aren't in the journal), which destroys user trust. If the system cannot record the event, the event must not happen.

### B. The Doctrine of Obedience (No Auto-Scaling)

The Collector is "dumb." It explicitly rejects adaptive logic. If the user configures 30 pps (pings per second), the
Collector sends 30 pps—even if packet loss is 100%, even if latency is 5000ms.

- **Why:** Slowing down during an outage masks the severity of the problem. "100% loss at 30Hz" is valuable data. "1
  sample at 1Hz" (because the system backed off) is corrupted data.

### C. Fail-Static

If the network disconnects, the Collector enters a "Fail-Static" mode. It continues to ping and buffers results in a
high-performance in-memory Ring Buffer. It does not crash; it assumes the network is hostile and waits for the Database
to return.

## 3. System Architecture: The Actor Model

ZZPing has moved away from monolithic RPC services to a pure **Actor Model** architecture (using Actix/Tokio).

### The Nodes

1. **The Collector:** A lightweight agent (often running on Raspberry Pis) that generates ICMP traffic. It is stateless
   regarding long-term storage but stateful regarding short-term buffering.
2. **The Database:** The central server that ingests streams from Collectors, persists data to disk, and serves queries
   to the GUI.

### The "Three-Actor" Pattern

Every functional component (e.g., `MemDB`, `IntentConfig`) is built as a cluster of three distinct actors to isolate
concerns:

1. **MainActor:** Pure business logic. Knows nothing about the network.
2. **NetworkManager:** Manages peer lifecycles. Knows when peers join/leave but not what they say.
3. **NetworkActor:** A per-peer translator. Converts raw network bytes into domain messages for the MainActor.

This pattern ensures that business logic is 100% testable in isolation without mocking TCP sockets.

## 4. The ZzNet Protocol

ZZPing uses a custom application-layer protocol called **ZzNet** over a single TCP connection per peer.

### The "Room" Abstraction

ZzNet multiplexes multiple logical streams over one TCP socket. These streams are called **Rooms**.

- A Room is a typed, bidirectional channel between two specific component instances (e.g., Collector `MemDB` talking to
  Database `MemDB`).
- Messages are serialized (MessagePack/RON), framed, and routed to the correct local actor.

### Protocol Symmetry

Components are symmetric. The code running on the Collector and the Database is often the same component, just
configured with a different "Role" (e.g., `MemDB` on Collector _pushes_ batches; `MemDB` on Database _ingests_ batches).

## 5. The Data Pipeline

The flow of data is designed to capture the exact state of the network:

1. **Unified Stream:** The system treats "Ping Sent" (In-Flight) and "Ping Result" events as a single stream. It does
   not prioritize one over the other. This allows the GUI to visualize a packet traveling in real-time before the result
   arrives.
2. **Buffering:** The Collector holds a Ring Buffer (e.g., 1 hour of data). If the connection to the DB drops, data
   accumulates. When reconnected, the buffer drains to the DB.
3. **Ingestion:** The Database receives batches, appends them to a "Hot Tier" (memory), and eventually compresses them
   into immutable blocks on disk.

## 6. Orchestration & Mastership

To prevent "Split-Brain" (two collectors pinging from the same location), ZZPing implements a strict Mastership
protocol:

- **Identity:** A Collector is identified by its `installation_id` (config), not just its certificate.
- **Locking:** A Collector must acquire a local **TCP Port Lock** to become active. This prevents multiple processes on
  the same machine from fighting.
- **Handoff:** The Database orchestrates "Zero-Downtime Upgrades." It commands the old Collector to stop and the new one
  to start in a precise sequence to ensure no gaps in the ping history.

## 7. Security

- **Authentication:** Strict **mTLS** (Mutual TLS). Both sides must present valid certificates signed by the internal
  CA.
- **Authorization:** Decoupled from Identity. A generic `collector.pem` certificate authorizes a node to _be_ a
  collector, but the node's specific configuration determines _which_ data stream it owns.
- **Encryption:** All traffic is encrypted via TLS 1.3.

## 8. Summary

When reasoning about ZZPing, assume:

- **Correctness > Availability.**
- **Components are isolated Actors.**
- **Network is custom (ZzNet/Rooms), not gRPC/HTTP.**
- **Testing is done via in-memory network simulation ("Sociable Unit Tests"), not end-to-end spawns.**
