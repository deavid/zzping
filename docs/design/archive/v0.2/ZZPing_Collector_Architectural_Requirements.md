# ZZPing Collector: Architectural Requirements (v0.3)

NOTE: Deprecated documentation.

Author: [David Martínez Martí](mailto:deavidsedice@gmail.com)

Created Date: Sep 6, 2025

Last Major update: Sep 7, 2025

Last prod zzping version: v0.2.2-beta2

Concept aimed for version: v0.3

Context: This document is created because we keep creating a design architecture that does not meet the vision on
[ZZPing - Architectural Vision II (v0.3-concept)](https://docs.google.com/document/d/1Krf_dfLXVrRvxvgsWf0ZJVObmwS14DM0t-S8jq5qyXE/edit?tab=t.0#heading=h.ryjns72u6nyt)
or
[ZZPing - Network protocol (v0.3 concept)](https://docs.google.com/document/d/1tFi44lH-pCbZpxa5VQ01XT8-8e8-b30nyfLAvGVQNdA/edit?tab=t.0)

Trying to code what we want as requirements should give us a checklist for any possible architecture to validate it.

# Chapter 1: Core Principles & Configuration

This chapter establishes the foundational requirements for the zzping-collector. It defines the collector's identity,
its configuration, and the principles that govern its startup behavior. The primary goal is to ensure that a collector
instance is always safe, predictable, and resilient, capable of operating correctly even in the face of network
interruptions or component restarts.

#### **1.1. File-Based Configuration**

The collector's core operational parameters **MUST** be defined in a local configuration file to ensure explicit,
persistent, and easily auditable settings.

- **Requirement:** The collector **MUST** be configured primarily via a local, human-readable collector.ron file.
- **Implementation Details:**
  - Command-line flags **SHOULD** be strictly limited to overriding the _path_ to this configuration file. Individual
    settings **MUST NOT** be configurable via flags.
  - The collector.ron file **MUST** contain the following fields:
    - hostname: A persistent, human-readable identifier (e.g., "collector-01"). This is the logical identity of the
      collector across all process restarts and binary upgrades. It is sent in the HELLO handshake and remains unchanged
      for the lifetime of the collector deployment on a given host.
    - database_addr: The gRPC address (e.g., "https://zzping-db.local:7878") of the database server.
    - auth_token: The authentication token used to identify and authorize the collector with the database.
- **Rationale:** Centralizing configuration in a single file simplifies deployment and management, providing a single
  source of truth for the collector's identity and connection parameters. It avoids the complexity of managing and
  prioritizing numerous command-line arguments.

#### **1.2. Last-Known Intent Cache**

To ensure operational continuity during database outages, the collector **MUST** maintain a local cache of the last
valid configuration it received.

- **Requirement:** The collector **MUST** persist its last known valid operational configuration (i.e., the list of
  targets and the global ping rate) received from the database.
- **Implementation Details:**
  - This cache **MUST** be stored in a separate, human-readable file named last_intent.ron. This file should be
    considered machine-managed state and stored in an appropriate cache or state directory (e.g., \~/.cache/zzping/).
  - The cache **MUST NOT** use binary serialization formats like bincode, to ensure it can be easily inspected and
    debugged by a human operator.
  - The cache **MUST** only be written or updated when the collector receives a new, valid, and successfully parsed
    HeartbeatResponse from the database.
- **Rationale:** This cache is the key to the collector's resilience. It allows a collector that was previously active
  to restart and continue its essential pinging function using the last known good configuration, even if the database
  is temporarily unreachable.

#### **1.3. Startup State and Safety**

The collector's startup behavior is governed by a "safety first" principle to prevent data races and conflicts,
particularly during automated upgrades.

- **Requirement:** This requirement supersedes any previous assumption of starting in a pinging state (e.g., requirement
  1.3 in the original draft). A collector instance **MUST** always start in STANDBY mode.
- **Implementation Details:**
  - In STANDBY mode, the collector **MUST NOT** initiate any pinging operations. Its sole responsibility is to establish
    a connection to the database.
  - A collector **MUST** only transition out of the initial STANDBY state after it has received an explicit command from
    the database via the gRPC protocol (e.g., a promotion to a PRIMARY or PRIMARY_SUPERVISED role).
- **Resilience Principle:** The STANDBY state is a _transient startup lock_, not a permanent dependency. Once a
  collector has been promoted to a PRIMARY role, it **MUST** continue its pinging operations using the configuration
  from its last_intent.ron cache, even if the connection to the database is subsequently lost.

#### **1.4. Local Discovery and Mutual Exclusion**

To enhance startup safety and enable robust handoffs, the collector **MUST** use a local TCP port lock as a
database-independent mechanism for mutual exclusion.

- **Requirement:** The collector instance holding the PRIMARY role **MUST** acquire and hold an exclusive lock on a
  well-known, local TCP port (e.g., 127.0.0.1:7879).
- **Implementation Details:**
  - This port **MUST** be distinct from the default database gRPC port.
  - Upon startup, a new collector instance **MUST** attempt to acquire this lock before initiating its connection to the
    database.
    - **Failure** to acquire the lock provides immediate, definitive confirmation that another PRIMARY instance is
      already active on the same host.
    - **Success** in acquiring the lock indicates that no other PRIMARY instance is currently active.
  - Crucially, regardless of the outcome of the lock attempt, the new collector **MUST** still proceed by starting in
    STANDBY mode, as per requirement 1.3. The port lock is a discovery and mutex mechanism, **not** a signal for a
    collector to self-promote to PRIMARY.
- **Rationale:** This mechanism provides a fast and reliable way for a new collector instance to discover the state of
  its local environment without needing to communicate with the database. It prevents race conditions where two
  collectors might briefly believe they are both PRIMARY during a chaotic restart scenario.

# Chapter 2: Steady-State Operation

This chapter describes the collector's behavior once it has successfully started and established communication with the
database. It details the main runtime loop, the mechanisms for receiving commands and configuration, how the collector
reports its own health, and how it manages the core pinging tasks to produce valid, reliable data.

#### **2.1. The Hybrid Command Model**

To balance the need for high-precision command delivery with the need for resilience against network failures, the
collector's primary communication model is a hybrid of reliable, polling-based RPCs and a low-latency command stream.

- **Requirement:** The architecture **WILL** use a hybrid model combining a Unary RPC for resilience with a
  server-stream for low-latency commands.
- **Implementation Details:**
  - **Reliable Path (Unary RPC):** The Heartbeat RPC serves as the robust, polling-based backbone. The collector
    **MUST** send a heartbeat periodically (e.g., every second). This channel is the authoritative, slow-path mechanism
    for receiving configuration and commands, and it serves as the ultimate fallback if the fast-path stream fails.
  - **Fast Path (Server Streaming RPC):** A new server-streaming RPC, SubscribeToCommands(request) returns (stream
    Command), **WILL** be added to the protocol. The collector **MUST** establish and maintain this stream with the
    database. This stream is the primary, low-latency channel for receiving time-sensitive commands like PrepareToSwap.
  - **Graceful Degradation:** If the SubscribeToCommands stream breaks for any reason (e.g., network error, database
    restart), the collector **MUST NOT** cease operations. It **MUST** continue its Heartbeat loop and gracefully
    degrade to using the commands received in the HeartbeatResponse. It **MUST** periodically attempt to re-establish
    the command stream.
- **Rationale:** This hybrid model provides the best of both worlds. The Unary RPCs ensure the system is fundamentally
  resilient and can always function. The command stream adds the necessary speed for high-precision operations like the
  handoff protocol, without making the entire system fragile. This avoids the complexity of custom time-synchronization
  protocols while using standard framework features.

#### **2.2. Command Stream Reliability**

To prevent a "zombie stream" scenario—where a TCP connection remains open but commands are not being processed—a
verification loop is required to ensure the fast-path command stream is healthy.

- **Requirement:** A verification loop using the Heartbeat RPC **MUST** be implemented to confirm that commands sent on
  the fast path are being processed.
- **Implementation Details:**
  - **Database Responsibility:** The database **MUST** assign a unique, monotonically increasing command_id to every
    command it sends on the SubscribeToCommands stream. It **MUST** monitor the last_processed_command_id received in
    each collector's heartbeat. If this ID fails to advance after new commands have been sent, the database **MUST**
    consider the command stream unhealthy and fall back to sending commands via the HeartbeatResponse.
  - **Collector Responsibility:** The HeartbeatRequest message **MUST** be expanded to include the field uint64
    last_processed_command_id. The collector **MUST** report the command_id of the last command it successfully received
    and acted upon from the command stream in every heartbeat.
- **Rationale:** This mechanism closes the feedback loop on the fast path. It allows the database to detect a silent
  failure in the collector's command processing logic and revert to the reliable slow path, preventing a handoff or
  other critical operation from failing due to an unresponsive but seemingly connected client.

#### **2.3. Health Reporting**

To solve the "ambiguous blank graph" problem and provide system observability, the collector **MUST** report its own
health status to the database.

- **Requirement:** The collector **MUST** report its operational health status as part of every HeartbeatRequest.
- **Implementation Details:**
  - The HeartbeatRequest message in the .proto file **MUST** be expanded to include the following fields:
    - current_role: The collector's current role/state (e.g., STANDBY, PRIMARY_SUPERVISED), represented as an enum
      value.
    - buffer_record_count: The current size of its in-memory data buffer, measured in the number of records.
    - last_fatal_error: An optional string field for reporting persistent, unrecoverable errors (e.g., "Failed to create
      raw ICMP socket"). This field should be cleared upon successful operation.
- **Rationale:** This health information is critical for system observability. It allows an operator or GUI to
  distinguish between an external network failure (e.g., 100% packet loss) and an internal collector failure. It also
  enables the database scheduler to make smarter decisions, such as avoiding a handoff to a collector that is reporting
  a fatal error.

#### **2.4. Pinger Task Management**

To ensure efficiency and preserve state during role transitions, pinging tasks must be persistent.

- **Requirement:** The TaskSupervisor **MUST** create and keep pinger tasks running for a given target, even when the
  collector is in a non-pinging state like STANDBY or SUPERVISING.
- **Implementation Details:**
  - A role change command (e.g., from PRIMARY to STANDBY) **WILL** be communicated to the active pinger task. The task
    itself **MUST NOT** be destroyed and recreated.
  - Upon receiving a command to enter a non-pinging state, the task **MUST** pause its pinging loop but remain active,
    ready to resume immediately upon receiving a subsequent command.
- **Rationale:** Recreating tasks is an expensive operation that is slow and loses internal state (such as ICMP sequence
  numbers). A persistent task that can be toggled on and off is simpler, faster, and more robust, enabling the collector
  to react to role changes with the high precision required by the handoff protocol.

#### **2.5. Timestamp Generation**

The integrity of all collected data is fundamentally dependent on the correctness of its timestamps.

- **Requirement:** The collector **MUST** generate absolute, monotonic UNIX timestamps for the sent_nanos field of every
  ping record.
- **Implementation Details:**
  - This **WILL** be achieved by maintaining a (SystemTime, Instant) reference pair. This pair **MUST** be re-captured
    periodically (e.g., every minute) to correct for clock drift caused by NTP slew adjustments.
  - The timestamp for each ping is calculated as: reference_system_time \+ (current_instant \- reference_instant).
  - **Monotonicity Guarantee:** The collector **MUST** enforce strict forward-moving time. If a newly calculated
    timestamp is earlier than the previously generated one (due to an NTP step adjustment), the ping for that interval
    **MUST** be skipped. For large backward jumps (e.g., \>5 seconds), the collector **SHOULD** treat this as a fatal
    state and exit, relying on a process supervisor to restart it.
- **Rationale:** This hybrid approach solves the conflict between accuracy (wall-clock time from SystemTime) and
  consistency (monotonicity from Instant). It produces timestamps that are both accurate enough for correlation with
  real-world events and consistent enough to prevent corruption of the time-series database, correctly handling both
  gradual and abrupt clock adjustments.

# Chapter 3: Data Pipeline & Resilience

This chapter details the journey of a single ping measurement, from its generation to its secure submission to the
database. The requirements herein are designed to create a data pipeline that is fundamentally resilient, guaranteeing
data integrity and providing mechanisms for both durable storage and real-time observability.

#### **3.1. Per-Target Data Submission**

To simplify state management, improve concurrency, and isolate failures, the logic for buffering and submitting data for
each monitored target **MUST** be handled by a dedicated, independent component.

- **Requirement:** The data submission logic **WILL** be managed by dedicated, per-target BatchSubmitter instances.
- **Implementation Details:**
  - For each target IP address the collector is instructed to ping, a corresponding BatchSubmitter instance will be
    created.
  - Each BatchSubmitter is a self-contained state machine responsible for:
    1. Receiving RawDataRecords for its specific target.
    2. Managing its own in-memory data buffer.
    3. Tracking its own last_acked_received_nanos cursor.
    4. Executing its own SendBatch RPC calls to the database.
- **Rationale:** This approach supersedes any model using a monolithic, multi-target buffer manager. It dramatically
  simplifies the system's logic by eliminating the need for complex, shared data structures (e.g., HashMap\<IpAddr,
  VecDeque\>) and associated locking. This architectural choice isolates potential failures—a bug or performance issue
  in one BatchSubmitter will not impact the data submission for other targets—and naturally enables concurrent data
  submission across multiple targets.

#### **3.2. Data Integrity via received_nanos Cursor**

To ensure the data stream between the collector and database is strictly ordered and recoverable, the acknowledgment
mechanism **MUST** be based on a guaranteed monotonic value.

- **Requirement:** The SendBatch ACK/DESYNC protocol **MUST** be based on a monotonic cursor. The sent*nanos \+
  rtt_nanos (effectively, the timestamp the ping response was \_received* by the collector) is the only value guaranteed
  to be monotonic, as sent_nanos alone can appear out of order due to network RTT variance.
- **Implementation Details:**
  - The .proto definition for SendBatchRequest and SendBatchResponse **MUST** be updated to reflect this. The relevant
    fields will be renamed to collector_believes_last_acked_received_nanos and
    database_confirms_last_acked_received_nanos, respectively.
  - Any previous use of sent_nanos for this purpose is superseded.
- **Rationale:** This is a critical requirement for data integrity. Relying on sent_nanos would lead to unrecoverable
  data corruption scenarios where a late-arriving ping with a high RTT could cause the database's ACK cursor to move
  backward. Using the monotonic received_nanos makes the DESYNC recovery mechanism mathematically sound, reliable, and
  robust against all network conditions.

#### **3.3. Buffer Implementation**

The collector's in-memory buffer must be implemented with a data structure that can efficiently support the
received_nanos cursor and the DESYNC recovery protocol.

- **Requirement:** The in-memory data buffer **SHOULD** be implemented using a data structure that allows for efficient
  slicing and seeking based on a key.
- **Implementation Details:**
  - A B-Tree (or a similar ordered map structure like std::collections::BTreeMap) keyed by received_nanos is the
    recommended implementation.
  - The value in the map should contain the full RawDataRecord.
- **Rationale:** A simple VecDeque is insufficient for this task. A DESYNC response requires the collector to rewind its
  submission cursor to a specific received_nanos timestamp. A B-Tree provides O(log n) access to this rewind point,
  allowing the BatchSubmitter to efficiently find and re-send the correct slice of data. A VecDeque would require a
  slow, O(n) linear scan, which would cause significant performance degradation and potential stalls with large buffers.

### **3.4. Comprehensive Buffer Management Policy**

To guarantee data resilience during short-term outages while preventing catastrophic failure during prolonged outages,
the collector's buffer **MUST** be managed by a multi-tiered pruning policy. This requirement supersedes any previous,
simpler buffer management rules.

- **Requirement:** The collector's in-memory data buffer **MUST** be pruned according to a strict hierarchy of rules,
  applied in the following order of precedence:
- **Implementation Details: The Pruning Hierarchy**
  - **Rule 1: Pruning by fsync Acknowledgment (Safest)**
    - The GetRecentDataResponse and a future HeartbeatResponse **MAY** contain a last_fsynced_received_nanos field.
    - If this value is present and greater than the collector's last known fsync point, the collector **MUST** safely
      and permanently delete all records in its buffer with a received_nanos less than or equal to this new value.
    - This is the primary and most desirable mechanism for buffer pruning, as it guarantees the data has been made
      durable by the database.
  - **Rule 2: Pruning by Hard Limit (The Circuit Breaker)**
    - The collector **MUST** enforce a configurable, hard upper limit on the number of records stored in its buffer
      (e.g., a default of 1,000,000 records).
    - If adding a new record would cause the buffer to exceed this limit, the collector **MUST** drop the _oldest_
      record from the buffer to make space.
    - This rule acts as a critical safety valve to prevent an out-of-memory (OOM) crash during a prolonged database
      outage. It explicitly trades the oldest, least-valuable data for the continued survival and operation of the
      collector.
  - **Rule 3: Pruning by Time-Based Retention (Long-Term Cleanup)**
    - The collector **MUST** enforce a configurable, long-term time-based retention policy (e.g., a default of 24
      hours).
    - Periodically, the collector **MUST** scan its buffer and permanently delete all records older than this retention
      period.
    - This rule serves as a general cleanup mechanism to manage the buffer's size during normal operation and ensures
      that the collector's memory usage does not grow indefinitely over many days.
- **Rationale:** A single-rule policy is insufficient. This hierarchical approach provides a complete solution.
  - **Rule 1** is the ideal path, ensuring zero data loss by synchronizing with the database's durable state.
  - **Rule 2** provides a critical safeguard against uncontrolled memory growth and catastrophic failure, making the
    collector resilient to long-term outages.
  - **Rule 3** ensures predictable memory usage and prevents slow memory leaks over time.
- Together, these rules create a buffer management system that is safe, resilient, and predictable under all operating
  conditions.

#### **3.5. Real-Time Liveness Stream**

To provide the low-latency data needed for a responsive GUI without compromising the resilience of the primary data
pipeline, a separate, ephemeral data stream **MUST** be implemented.

- **Requirement:** A new Unary gRPC RPC, AnnouncePings(AnnouncePingsRequest), **WILL** be added to the protocol for
  real-time visualization purposes.
- **Implementation Details:**
  - The AnnouncePingsRequest message will contain a list of sent_nanos for pings that have just been sent by the
    collector.
  - The collector **WILL** call this RPC in a "fire-and-forget" manner. It **MUST NOT** block waiting for a response and
    **SHOULD NOT** implement a retry mechanism for this specific call.
  - This data stream has **no delivery or ordering guarantees**. The SendBatch RPC remains the sole source of truth for
    all durable, historical data.
  - The zzping-database is required to hold this ephemeral liveness data in an in-memory cache for at least **one
    minute** to serve to GUI clients.
- **Rationale:** This cleanly separates the concerns of durable, high-integrity data ingestion from ephemeral,
  low-latency visualization. It solves the "ambiguous blank graph" problem by allowing a GUI to know that pings are
  being sent, even before their responses have been received. This provides an immediate and accurate view of the
  network's liveness without adding latency or complexity to the critical path of data preservation.

# Chapter 4: The Zero-Downtime Handoff Protocol

This chapter details the complete, database-orchestrated protocol for performing a zero-downtime "live swap" of a
collector binary. This process is designed as a **supervised trial with automatic rollback**, ensuring that an upgrade
is not only seamless but also safe, automatically reverting to the last known good state in the event of a failure. The
protocol relies on the hybrid command model (defined in Chapter 2\) to achieve the high precision necessary for a
near-zero data gap transition.

#### **4.1. Role Definitions**

The handoff protocol introduces two specialized, transient roles for collectors. The full set of roles used during this
process is as follows:

- **PRIMARY:** The standard active state. The collector is pinging, buffering data, and actively sending that data to
  the database via SendBatch. It holds the local TCP port lock.
- **STANDBY:** The initial state for any new collector instance. The collector is connected to the database but is
  completely passive: it is not pinging and not sending data.
- **PRIMARY_SUPERVISED:** A transitional state for the _new_ collector instance (C2) at the start of a handoff. In this
  state, the collector **is actively pinging** and buffering the results locally. It **MUST NOT** send this new data to
  the database via SendBatch. Its purpose is to begin data collection at the precise swap moment while the old collector
  drains its buffer, preventing data from arriving out of order.
- **SUPERVISING:** A transitional state for the _old_ collector instance (C1) during a handoff. In this state, the
  collector **MUST NOT** ping. Its sole responsibility is to drain its existing buffer to the database by continuing its
  SendBatch loop until the buffer is empty. It serves as a live fallback in case the new instance fails its trial.
- **SHUTDOWN:** The terminal state. The collector has been commanded by the database to exit gracefully.

#### **4.2. Initial State Synchronization**

To prevent a DESYNC loop and to prepare the new collector instance for operation, it must synchronize its state with the
database before it can be considered for promotion.

- **Requirement:** When a collector is commanded to transition into the PRIMARY_SUPERVISED role, its first action
  **MUST** be to call the GetRecentData RPC.
- **Implementation Details:** This call serves two critical purposes:
  1. **Preventing Initial DESYNC:** The GetRecentDataResponse **MUST** include the
     database_confirms_last_acked_received_nanos field. The collector **MUST** use this value to initialize its local
     last_acked_received_nanos cursor before it ever sends its first SendBatch. This guarantees its first data
     submission will be in sync with the database's state.
  2. **Enabling Future Resilience (Buffer Seeding):** The response should also contain recent raw data records. The
     collector populates its buffer with this data. While the buffer is empty at the start of a handoff, this seeding
     ensures that if the database were to restart shortly _after_ the handoff, the new collector would have the
     necessary recent history to re-send and prevent a data gap.

#### **4.3. The Supervised Trial Sequence**

The entire handoff process is a strictly ordered sequence of events orchestrated by the database and executed with high
precision by the collectors.

- **Requirement:** The handoff sequence **MUST** follow this supervised trial protocol:
  1. **Initiation:** The database scheduler detects two instances (C1=PRIMARY, C2=STANDBY) with the same hostname (two
     connections, same collector identity). It initiates the handoff by pushing commands to both collectors
     simultaneously via the SubscribeToCommands stream.
  2. **The Atomic Swap:** The arrival of the command is the high-precision signal.
     - C1 receives the command: "Become SUPERVISING." It **immediately** stops all pinging operations.
     - C2 receives the command: "Become PRIMARY_SUPERVISED." It **immediately** starts its pinging operations.
  3. **The Drain & Buffer Phase:** The system is now in a state where C1 is only sending old data and C2 is only
     collecting new data.
     - C1 continues its SendBatch loop, draining its buffer. It reports its remaining buffer size in every
       HeartbeatRequest (as per requirement 2.3).
     - C2 performs its GetRecentData call, begins pinging, and buffers the new results locally.
  4. **Promotion & Verification Start:** The database monitors C1's reported buffer size. When the buffer size reaches
     zero, it determines the drain is complete.
     - The database pushes a PromoteToPrimary command to C2's stream.
     - Upon receiving this command, C2 transitions to the full PRIMARY role and starts its SendBatch loop.
     - Simultaneously, the database starts a **verification timer** (e.g., 5 seconds). C1 is held in its SUPERVISING
       state as a live fallback.
  5. **Success Condition:** If the database receives a valid SendBatch from C2 within the verification timer's duration,
     the trial is declared a success. The database then sends a final SHUTDOWN command to C1. The handoff is complete.
  6. **Failure Condition (Automatic Rollback):** If the verification timer expires and the database has _not_ received a
     valid SendBatch from C2, the trial has failed. The database **MUST** execute a rollback:
     - It sends a SHUTDOWN command to the faulty C2.
     - It sends a RevertToPrimary command to C1. C1 receives this, transitions back to the PRIMARY role, and resumes
       pinging. The system is now back in its original, stable state.

# Chapter 5: System-Wide Requirements & Dependencies

This chapter details requirements that have system-wide implications, including how the collector interacts with its
gRPC client and the critical behavioral contracts it depends on from the zzping-database service. These requirements are
essential for ensuring the performance, maintainability, and overall resilience of the entire zzping ecosystem.

#### **5.1. Concurrent gRPC Client Access**

To ensure high performance and prevent artificial bottlenecks, the collector's use of its gRPC client **MUST** be
designed for concurrency.

- **Requirement:** The gRPC IngestionClient **MUST** be treated as a lightweight, cloneable handle. It **MUST NOT** be
  wrapped in an Arc\<Mutex\> or any other locking primitive that would serialize all database access.
- **Implementation Details:**
  - Each task requiring database communication (e.g., the Orchestrator's Heartbeat loop, each per-target BatchSubmitter)
    **WILL** receive its own clone of the client.
- **Rationale:** The client generated by the tonic framework is designed for concurrent use. It manages an internal
  connection pool (leveraging HTTP/2 streams) and can handle many simultaneous in-flight requests. Wrapping it in a
  Mutex fundamentally breaks this design, creating a single, system-wide bottleneck where only one RPC call can be
  active at a time. This would cripple the performance of the per-target BatchSubmitter model and make the collector
  unresponsive.

#### **5.2. Client Abstraction**

To improve code clarity, reduce repetition, and make the codebase less error-prone, the logic for creating and
authenticating gRPC requests **SHOULD** be centralized.

- **Requirement:** A dedicated DatabaseClient wrapper struct **SHOULD** be created to encapsulate the logic of creating
  and authenticating gRPC requests.
- **Implementation Details:**
  - This wrapper would handle common tasks like inserting the auth_token into the request metadata.
  - Components like the BatchSubmitter would interact with this clean abstraction rather than manually constructing
    tonic::Request objects.
- **Rationale:** This follows the Don't Repeat Yourself (DRY) principle. Centralizing the request-building logic in one
  place ensures consistency and makes future changes (e.g., modifying the authentication scheme) much simpler to
  implement, as the change would only be required in one location.

#### **5.3. Prerequisite: Persistent Database ACK State**

The collector's entire resilience and DESYNC recovery model is critically dependent on the zzping-database correctly
maintaining a persistent state for each collector's progress.

- **Requirement:** The zzping-database **MUST** fulfill the following behavioral contract:
  1. The database **MUST** persist the last_acked_received_nanos value for each (hostname, target_ip) pair.
  2. Upon restart, the database **MUST** reload this state (e.g., by scanning recent data files to find the latest
     received timestamp for each pair) _before_ it begins accepting collector connections.
- **Rationale:** This is a non-negotiable system-wide requirement. If the database's ACK state is ephemeral (i.e., only
  in-memory), a database restart would wipe it out. A reconnecting collector, with its durable buffer, would attempt to
  send data from its last known ACK point. The newly-restarted database, believing the ACK point to be zero, would
  reject this data with a DESYNC response. This would trigger a permanent, unrecoverable DESYNC loop, causing total data
  loss until the collector's buffer ages out. The entire resilience model of the collector collapses without this
  guarantee from the database.

# Appendix A: Design Rationale

This appendix provides additional context and justification for key architectural decisions that have significant
system-wide implications. Its purpose is to capture the principles and trade-offs that led to the chosen design,
ensuring that the rationale is preserved alongside the requirements themselves.

#### **A.1. Rationale for Automatic Rollback: Human-Centric Resilience**

The design of the zero-downtime handoff protocol (Chapter 4\) culminates in a supervised trial with a critical feature:
automatic rollback on failure. This is a deliberate, foundational choice to prioritize human-centric resilience and
mitigate the high-impact risk of a failed upgrade.

**The Unacceptable Alternative: Manual Intervention**

Without an automatic rollback mechanism, a failed collector upgrade would create an unacceptable scenario for both
developers and end-users. Consider the common failure modes:

- A developer introduces a subtle bug in the SendBatch logic and deploys the new collector binary. The old binary is
  overwritten and no longer exists on disk.
- An end-user performs an upgrade, but forgets a prerequisite step, such as upgrading the zzping-database first to
  support a new protocol feature.

In either case, the handoff would proceed, the old collector (C1) would shut down, and the new, faulty collector (C2)
would fail to send data. At this point, the system is in a broken state with no active data collection. The operator is
now in a crisis, facing two critical problems:

1. **A Mounting Data Gap:** Every second that passes is another second of lost, irretrievable data.
2. **High-Stress Troubleshooting:** The operator is under immense pressure to "fix this ASAP." This stress increases the
   likelihood of further mistakes during the diagnosis and recovery process. The time required to identify the bug, find
   the old binary (or revert the code from version control), rebuild it, and redeploy it could easily range from minutes
   to over an hour.

This scenario—a large, unpredictable data gap combined with high-pressure, manual intervention—directly violates the
project's "Robustness and Resilience First" guiding principle.

**The Chosen Solution: A Predictable and Contained Event**

The supervised trial with automatic rollback transforms this potential crisis into a predictable, low-impact, and fully
automated event.

When the trial fails (the database does not receive data from the new collector within the verification window), the
system automatically reverts to its last known good state. It commands the faulty new collector to shut down and
reactivates the old, proven collector.

- **The Impact:** The result is a small, contained data gap, precisely equal to the duration of the verification timer
  (e.g., 5 seconds).
- **The Outcome:** The system heals itself. Data collection resumes immediately. The operator is informed of the _failed
  deployment_, not a _system outage_. The high-stress crisis is averted, and the operator can debug the failed binary
  offline, without the pressure of an ongoing data loss event.

**Conclusion:** A contained, 5-second data gap resulting from an automatic rollback is an explicit and vastly superior
trade-off to the multi-hour gap and high-stress operator intervention that would result from a failed deployment without
this mechanism. This design choice puts the resilience of the system and the well-being of its operator above the
marginal cost of a few seconds of data during a rare failure event.
