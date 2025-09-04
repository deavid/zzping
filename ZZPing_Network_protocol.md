# ZZPing \- Network protocol 

## *v0.3 concept, architectural decision record*

Author: [David Martínez Martí](mailto:deavidsedice@gmail.com)

Created Date: Sep 3, 2025

Last Major update: Sep 4, 2025

Last prod zzping version: v0.2.2-beta2

Concept aimed for version: v0.3

**Context:** This document supersedes the initial network protocol concepts outlined in [ZZPing - Architectural Vision II (v0.3-concept)](https://docs.google.com/document/d/1Krf_dfLXVrRvxvgsWf0ZJVObmwS14DM0t-S8jq5qyXE/edit?tab=t.0) \- It is the result of a rigorous architectural analysis aimed at designing a robust, resilient, and simple communication protocol for the zzping ecosystem.

# 1\. The Core Challenge: The Trilemma of Control, Resilience, and Simplicity

The fundamental design challenge for the zzping network protocol is to reconcile three competing requirements:

1. **Centralized Control (The Smart Database):** The zzping-database must be the single source of truth for configuration. It dictates what collectors should ping and at what rate. Collectors must be "dumb workers" that execute instructions.  
2. **Resilience to Failure (The Unreliable World):** The protocol must gracefully handle independent component failures. Database restarts, collector restarts, and transient network interruptions must not lead to data loss or corruption. A key operational requirement is the ability to perform a "live swap" of a collector or database binary for upgrades without causing a data gap.  
3. **Implementation Simplicity (The Maintainable System):** The protocol's implementation should be as simple and foolproof as possible to minimize bugs, reduce the burden of testing, and adhere to the project's "Simplicity Over Complexity" guiding principle.

Initial proof-of-concept work revealed that a simple, custom TCP protocol, while appealing, forces the developer to re-solve complex, well-understood problems like message framing, serialization contracts, and RPC patterns. This led to the decision to adopt a mature framework, gRPC with tonic, to handle the underlying transport and protocol mechanics.

However, the introduction of gRPC did not solve the application-level architectural problem; it merely provided a new set of tools and trade-offs to consider. Our subsequent analysis focused on finding the right gRPC pattern to solve the trilemma.

# 2\. Architectural Alternatives Considered and Rejected

## 2.1. Rejected: The Long-Lived Bi-directional Stream

* **Description:** A single, persistent gRPC bi-directional stream is established per collector. The collector sends data, and the database sends acknowledgments and unsolicited commands over this stream.  
* **Rationale for Rejection:** While offering the lowest-latency control, this model is architecturally fragile. A gRPC stream is a single, stateful RPC call; an error on either the send or receive side terminates the entire session. The application logic on both the client and server must perfectly manage this complex state machine to avoid deadlocks and silent failures. Our PoC demonstrated that this complexity is a significant source of bugs and violates the "Simplicity" principle.

## 2.2. Rejected: The "Collector as a Server" (Beacon and Callback) Model

* **Description:** Collectors run their own gRPC servers on dynamic ports. They announce their presence on the LAN via UDP broadcast beacons. The database listens for these beacons and then initiates outbound gRPC connections to the collectors to issue commands and pull data.  
* **Rationale for Rejection:** This model inverts the network topology and moves immense complexity onto the database. The database becomes a stateful client managing discovery, scheduling, and N outbound connections. It makes "live swaps" difficult due to port collisions on a single host and introduces a second, less reliable protocol (UDP). It violates the "Simplicity" and "Robustness" principles.

## 2.3. Rejected: Purely Local Handoff Protocol for Live Swaps

* **Description:** The "live swap" for a collector upgrade is handled exclusively between the old and new collector processes on the same machine, using a local-only communication channel (e.g., a separate local gRPC server). The database is completely unaware that a swap is occurring.  
* **Rationale for Rejection:** While this model excels at transferring the in-memory buffer, it fails to meet the strict requirement for a **supervised trial with automatic rollback**. The old collector (C1) has no way to verify from a trusted, external source (the database) that the new collector (C2) is actually functioning correctly post-handoff. A failure in C2 after C1 has already shut down would result in a permanent outage until an operator manually intervenes. This was deemed an unacceptable risk, favoring the slightly more complex but far more resilient database-orchestrated model.

# 3\. The Chosen Architecture: Unary RPC with Heartbeat Polling

After analyzing the trade-offs, the chosen architecture is based exclusively on **simple, transactional, Unary gRPC RPCs**. This model prioritizes robustness and simplicity above all else.

The interaction is defined by two core RPCs:

**RPC 1: Heartbeat(HeartbeatRequest) returns (HeartbeatResponse)**

* **Purpose:** Serves as both a liveness signal and the primary mechanism for configuration delivery.  
* **Interaction:**  
  1. The collector calls Heartbeat on a fixed interval (e.g., **1 second**). The 1-second interval is chosen to provide excellent responsiveness for user-initiated configuration changes, which is deemed more important than the minor overhead of frequent polling.  
  2. The HeartbeatRequest contains the collector's persistent, unique ID (collector\_uuid).  
  3. The database receives the request. Its logic is stateless per-call. It updates a "last seen" timestamp for that UUID in its persistent storage. It then reads the Intent Config.  
  4. The HeartbeatResponse contains the full Effective Config for that collector (the global list of targets and the single global ping rate).

**RPC 2: SendBatch(SendBatchRequest) returns (SendBatchResponse)**

* **Purpose:** Reliable, transactional submission of ping data.  
* **Interaction:**  
  1. The collector sends a batch of records (e.g., up to 1024).  
  2. **Crucially, every SendBatchRequest includes a collector\_believes\_last\_acked\_nanos field.** This is the collector's belief of the last timestamp the database has successfully stored.  
  3. The database receives the request. Before processing the data, it performs a **mandatory state check**, comparing the collector's belief with its own persisted truth.  
  4. **If they match,** the data is accepted, and the SendBatchResponse returns an OK status with the new, updated database\_confirms\_last\_acked\_nanos.  
  5. **If they do not match,** the data is **rejected**, and the SendBatchResponse returns a DESYNC status along with the authoritative database\_confirms\_last\_acked\_nanos. The collector must then rewind its buffer and re-send the correct data.  
* **Resilience:** This transactional, check-before-write mechanism makes it impossible for a collector to create a data gap after a database restart.

# 4\. Solving the "Live Swap" Challenge

The challenge of performing a zero-downtime collector upgrade is solved by making it a **database-orchestrated process**, which is enabled by the frequent Heartbeat poll.

* **Discovery:** The database detects a handoff scenario when it receives heartbeats from two different processes claiming the same collector\_uuid.  
* **Orchestration:** The database uses the HeartbeatResponse messages to command the collectors through a supervised trial.  
  1. It designates the old collector (C1) as PRIMARY and the new one (C2) as STANDBY (instructing it not to ping).  
  2. To avoid polling delays, it issues a **pre-scheduled swap command** to both collectors, giving them a precise future timestamp at which to atomically swap roles.  
  3. At the designated time, C2 becomes PRIMARY and C1 becomes SUPERVISING (stops pinging but continues heartbeating for verification).  
  4. The database verifies that C2 is sending data correctly.  
  5. If the trial is successful, the database commands C1 to shut down. If it fails, the database commands C1 to resume its PRIMARY role and C2 to shut down (automatic rollback).  
* **Buffer Handoff:** The problem of transferring C1's in-memory buffer is solved by **not transferring it at all.** Instead, after C2 successfully becomes PRIMARY, it makes a one-time Unary RPC call (GetRecentData) to the database to "seed" its buffer with the most recent data from the database's own in-memory hot tier. This keeps the database as the single source of truth and eliminates the need for any complex collector-to-collector communication.

# 5\. Summary of Rationale and Trade-offs

This final architecture was chosen because it best aligns with the project's guiding principles:

* **It is Simple:** It uses the simplest, most robust gRPC pattern (Unary RPCs). The complex logic is centralized in the database's scheduler, making the numerous collectors simple and "dumb."  
* **It is Robust:** The check-before-write mechanism in SendBatch provides strong guarantees against data gaps and corruption. The database-orchestrated handoff provides a mechanism for safe, automated, zero-downtime upgrades.  
* **What is Not Solved/Less Optimal:**  
  * **Control Latency:** There is a maximum 1-second delay for a collector to receive a new configuration. This is deemed an acceptable trade-off for the immense gain in architectural simplicity and robustness compared to a streaming model.  
  * **Data Loss During a Failed Swap:** While the buffer from the old collector is not transferred, the "Post-Handoff Buffer Seeding" greatly mitigates this. We accept that in the rare event of a *failed* upgrade, a few seconds of data might be lost. This is an acceptable trade-off to avoid the complexity of a local IPC handoff protocol.

# 6\. Configuration Model: A Shift to Global Simplicity

In alignment with the "Simplicity Over Complexity" guiding principle, the configuration model for collectors has been greatly simplified from the initial vision. The concept of a central database scheduler that dynamically load-balances ping rates across a fleet of collectors has been **rejected**.

Instead, the v0.3 architecture adopts a **Global Rate and Target model**.

* **Intent Config:** The master configuration in the database will contain a single, global ping\_rate (in pings per second) and a single, global list of targets.  
* **Database Logic:** The database's role is simplified to that of a configuration broadcaster. When a collector sends a Heartbeat, the database's response will always contain this same global configuration. It does not perform any per-collector calculation.  
* **Collector Logic:** Each collector, upon receiving the configuration, is responsible for pinging **every target** in the list at the specified **global rate**.

This decision dramatically reduces the complexity of the database, makes the system's behavior highly predictable, and is more than sufficient for the intended home network use case.

# 7\. Collector Identity and The "Live Swap" Handoff Protocol

A core resilience requirement is the ability to perform a zero-downtime upgrade of a collector binary. This is achieved through a database-orchestrated handoff protocol that relies on a stable collector identity and the Heartbeat RPC.

* **Collector Identity:** Each logical collector instance is identified by a **persistent, unique ID (collector\_uuid)**, which is stored in its local configuration file. This allows the database to recognize it as the same entity across process restarts and binary upgrades.  
* **Handoff Discovery:** The database discovers a handoff scenario when it begins receiving Heartbeat requests from two different processes (identifiable by their transient session info) that are both claiming the same persistent collector\_uuid.  
* **Orchestration via Pre-scheduled Swap:** To ensure a seamless, near-zero-gap transition, the database orchestrates the swap using a future timestamp:  
  1. It designates the incumbent collector (C1) as PRIMARY and the new collector (C2) as STANDBY.  
  2. It calculates a swap time a few seconds in the future and sends PrepareToSwap commands (containing the role and the exact timestamp) to both collectors via their HeartbeatResponses.  
  3. Both collectors, having synchronized clocks on the same machine, atomically swap roles at the designated time. C1 stops pinging and enters a SUPERVISING state, while C2 begins pinging.  
  4. The database verifies that C2 is submitting data correctly. If the trial succeeds, it commands C1 to shut down. If it fails, it commands C1 to resume its PRIMARY role and C2 to exit, triggering an automatic rollback.

# 8\. The "Post-Handoff Buffer Seeding" Mechanism

A critical challenge in the handoff is transferring the in-memory buffer of unsent pings from the old collector to the new one. To avoid the complexity of a direct, local collector-to-collector communication protocol, this architecture solves the problem by keeping the database as the single source of truth.

This requires a new Unary RPC in the protocol:

**RPC 3: GetRecentData(GetRecentDataRequest) returns (GetRecentDataResponse)**

* **Purpose:** Allows a newly promoted collector to "seed" its buffer with recent data, ensuring data continuity.  
* **Interaction:**  
  1. After C2 is successfully promoted to PRIMARY in a handoff, its buffer is empty.  
  2. Its first action is to call GetRecentData, requesting the last N minutes of data for its collector\_uuid.  
  3. The database serves this data from its in-memory "hot tier" of recently ingested records.  
  4. C2 populates its buffer with this response. It now has the necessary recent history to continue operation as if it had been running all along, ready to handle any subsequent database restarts without creating a data gap.

This mechanism accepts the small, calculated risk of losing a few seconds of data during a *failed* upgrade in exchange for a vastly simpler and more robust overall architecture that avoids any direct IPC between collector processes.

# 9\. Authorization (ACLs)

The Unary RPC model provides a standard, robust foundation for implementing Access Control Lists (ACLs). While the protocol itself is only concerned with authentication (verifying *who* a client is), the tonic framework enables a clean separation for authorization (verifying *what* a client is allowed to do).

* **Mechanism:** Authorization will be enforced within each RPC method on the server side.  
* **Authentication:** A tonic interceptor will be responsible for validating the client's token and attaching a trusted UserIdentity object (containing a user ID and a list of roles like "collector", "admin", or "reader") to the request.  
* **Authorization Logic:** Each RPC method will inspect the UserIdentity and check if the necessary role is present before proceeding. For example:  
  * SendBatch will require the "collector" role.  
  * A future UpdateConfig RPC will require the "admin" role.  
  * QueryData will require the "reader" role.

This pattern cleanly separates the concerns of authentication and authorization and provides a clear, testable, and secure way to manage permissions in the system.

