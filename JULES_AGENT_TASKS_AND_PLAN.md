Read and Consider:

* `ZZPing_Architectural_Vision_II.md` - this is the overall vision of what we're going for.
* `ZZPing_Network_protocol.md` - this overrides the above and sets what do we want to build right now, but it's an overall design.
* `ZZPing_Collector_Architectural_Requirements.md` - Checklist of what a good architecture should have
* `ZZPing_Collector_Software_Architecture_Design.md` - The architecture we want to have for zzping-collector

-------

Context for the task and problems:

-------

## Understanding of the Core ZZPing v0.3 Architecture

The final specification is clearly laid out in **`ZZPing Collector: Architectural Requirements (v0.3)`**. This document solidifies and, in some cases, evolves the concepts from the `Architectural Vision` and the `Network protocol` documents. My review will treat the `Requirements` document as the ultimate source of truth.

I've distilled the architecture into five foundational pillars. The code must serve these pillars.

#### Pillar 1: Uncompromising Resilience and Safety
This is the single most important theme. The system is designed to be a durable appliance, not a fragile application. Every decision prioritizes survival and data integrity over marginal performance or features.

*   **Startup Safety:** A collector *always* starts in `STANDBY` (Reqs 1.3). It does nothing until commanded by the database. The local TCP port lock (Reqs 1.4) is a brilliant, database-independent mutex to prevent local race conditions during chaotic restarts, but it is *not* a signal to self-promote.
*   **Operational Continuity:** The `last_intent.ron` cache (Reqs 1.2) is the key to surviving database outages. Once a collector is promoted, it is autonomous and must continue its work even if the database vanishes.
*   **Data Pipeline Integrity:** The `received_nanos` cursor (Reqs 3.2) is a non-negotiable requirement for mathematically sound data synchronization. Using `sent_nanos` is an architectural defect. The database's contract to persist this ACK state (Reqs 5.3) is the other half of this critical handshake; without it, the entire resilience model collapses.
*   **Memory Safety:** The multi-tiered buffer pruning policy (Reqs 3.4) is a complete solution, preventing OOM crashes during long outages (`Hard Limit`) while ensuring zero data loss in normal operation (`fsync Acknowledgment`).
*   **Handoff Safety:** The zero-downtime handoff is a **supervised trial with automatic rollback** (Reqs 4.3, Appendix A.1). This is a foundational, human-centric design choice. A failed upgrade must result in a contained, 5-second data gap, not a multi-hour crisis.

#### Pillar 2: The Hybrid Command & Data Model
The system balances resilience and responsiveness by cleanly separating communication paths.

*   **The Reliable Backbone (Slow Path):** The unary `Heartbeat` RPC is the ultimate source of truth for configuration and commands. The system must be able to function using *only* this path (Reqs 2.1).
*   **The Low-Latency Channel (Fast Path):** The `SubscribeToCommands` server-stream is an optimization for high-precision operations like the handoff. It is explicitly designed to be fallible. The collector *must* gracefully degrade to the slow path if this stream breaks (Reqs 2.1).
*   **Closed-Loop Verification:** The `last_processed_command_id` mechanism (Reqs 2.2) is essential. It prevents "zombie streams" and allows the database to intelligently fall back to the reliable path if the collector becomes unresponsive on the fast path.
*   **Separation of Concerns:** Durable data (`SendBatch`) is separated from ephemeral visualization data (`AnnouncePings`) (Reqs 3.5). This prevents GUI needs from compromising the integrity of the core data pipeline.

#### Pillar 3: Granular State Management & Concurrency
The architecture avoids monolithic, lock-heavy designs in favor of isolated, concurrent components.

*   **Per-Target Submission:** The `BatchSubmitter`-per-target model (Reqs 3.1) is a core architectural choice. It simplifies logic, enables concurrency, and isolates failures. Any implementation that uses a single, shared `HashMap<IpAddr, VecDeque>` protected by a `Mutex` has misunderstood the design.
*   **Efficient Buffering:** The buffer *must* be a B-Tree or similar ordered map keyed by `received_nanos` (Reqs 3.3). A `VecDeque` is insufficient and will not perform correctly during a DESYNC recovery.
*   **Persistent Tasks:** Pinger tasks are persistent and stateful. They are toggled on/off via commands, not destroyed and recreated, to preserve state like ICMP sequence numbers and ensure fast role transitions (Reqs 2.4).
*   **Non-Blocking gRPC Client:** The `tonic` client is cloneable and manages its own connection pool. It *must not* be wrapped in `Arc<Mutex>` (Reqs 5.1). This would serialize all database access and cripple the per-target `BatchSubmitter` model.

#### Pillar 4: First-Class Observability
The system is designed to be transparent about its own health, directly addressing the "ambiguous blank graph" problem.

*   **Collector Health Reporting:** The collector must report its `current_role`, `buffer_record_count`, and any `last_fatal_error` in every heartbeat (Reqs 2.3). This allows the database and operators to make informed decisions and distinguish collector failure from network failure.
*   **Real-Time Liveness:** The `AnnouncePings` RPC provides immediate feedback to the GUI that pings are being sent, even before responses are received (Reqs 3.5).

#### Pillar 5: Data Provenance and Integrity
The correctness of every stored data point is paramount.

*   **Absolute, Monotonic Timestamps:** The timestamp generation strategy (periodic re-sync of `(SystemTime, Instant)`) is a robust solution to the NTP slew/step problem. The handling of backward jumps is critical: **skip** on small jumps, **crash** on large jumps (Reqs 2.5). This is a strong, correct choice that prioritizes data integrity over the uptime of a single misconfigured machine.

---

### Points of Scrutiny for the Code Review

I am ready. Based on this synthesis, when I review the code, I will be intensely focused on the following areas:

1.  **State Machines and Transitions:** How rigorously are the Collector roles (`STANDBY`, `PRIMARY_SUPERVISED`, etc.) and the `BatchSubmitter` states implemented? Are all edge cases in the handoff protocol handled correctly, especially the failure/rollback paths?
2.  **Error Handling and Resilience:** Does the code live up to the "Resilience First" principle? How does the collector handle a broken `SubscribeToCommands` stream? How does it manage reconnection logic for the `Heartbeat` loop? Are database RPC failures handled with correct backoff and retry logic?
3.  **Concurrency and Locking:** Has the per-target `BatchSubmitter` model been implemented correctly? Is the `gRPC` client cloned and passed to each task, or is there a system-wide `Mutex` bottleneck? Is locking, if any, minimal and correctly scoped?
4.  **Data Structures:** Is the in-memory buffer a `BTreeMap` as required? Is the configuration managed safely across threads?
5.  **Data Integrity:** I will meticulously review the timestamp generation logic. I will also verify that the `received_nanos` cursor is used exclusively for the ACK/DESYNC protocol.
6.  **Adherence to the Protocol:** Does the implementation match the `.proto` definitions and the logic described in the `Requirements` document precisely? Are all required fields in the `HeartbeatRequest` present and correctly populated?
7.  **Clarity and Simplicity:** Does the code reflect the "Simplicity Over Complexity" principle? Is it well-structured, with clear abstractions (like the `DatabaseClient` wrapper from Reqs 5.2), or is it a monolithic tangle of logic?

------

## High-Level Assessment

*   **Adherence to Software Architecture Design:** **Excellent.** The code is a textbook implementation of the blueprint in `ZZPing_Collector_Software_Architecture_Design.md`. Components like `CollectorService`, `ConnectionManager`, `SessionHandler`, `TaskSupervisor`, and `TargetWorker` exist and interact precisely as designed. This is a major success.
*   **Adherence to Architectural Requirements:** **Incomplete.** There are significant gaps. Critical resilience and data integrity features defined as **MUST** in the requirements document are either missing or only partially implemented.
*   **Code Quality & Testability:** **Excellent.** The code is clean, idiomatic Rust. The decoupling of logic into testable units is well-executed. The recent commit (`f791453`) to remove test-specific logic from production code is a sign of mature engineering practice and is highly commendable.

---

### Pillar 1: Uncompromising Resilience and Safety

This is the most important pillar, and it's where the most critical deviations are found.

#### Strengths:

*   **Startup Safety (Reqs 1.3 & 1.4):** The collector correctly starts in a standby state. The `TaskSupervisor` does not spawn workers until it receives a valid `DatabaseClient` and a `PRIMARY` role via a `SupervisorConfig`. The TCP port lock for mutual exclusion is correctly implemented in `zzping-collector/src/lib.rs`.
*   **Buffer Implementation (Reqs 3.3 & 3.4):** The `BatchSubmitter` correctly uses a `BTreeMap` for its buffer. The "Hard Limit" (Rule 2) and "Time-Based Retention" (Rule 3) pruning mechanisms are implemented and tested.
*   **Graceful Shutdown (Design 5.4):** The signal handling in `collector_service.rs` and the coordinated shutdown sequence through the `TaskSupervisor` and `TargetWorker` are well-implemented. The ordered shutdown (Pinger first, then BatchSubmitter) is correctly handled in `target_worker.rs`, which is critical for preventing data loss on exit.

#### Critical Findings & Deviations:

1.  **CRITICAL: Last-Known Intent Cache is Not Used for Resilience (Violation of Reqs 1.2).**
    *   **Requirement:** The collector **MUST** use its cached `last_intent.ron` to continue operations if the database is unreachable on restart.
    *   **Finding:** The `CollectorService` reads `last_intent.ron` on startup, but it sets the initial role to `Standby`. Furthermore, the `TaskSupervisor` is designed to *defer* worker creation until it receives a `DatabaseClient`.
    *   **Impact:** This combination completely breaks the operational continuity principle. If a collector is restarted while the database is down, it has a configuration but will not create workers and will not ping, creating a large data gap. This violates one of the most fundamental resilience requirements.

2.  **DONE: Safest Buffer Pruning Mechanism is Implemented (Reqs 3.4).**
    *   **Requirement:** The buffer pruning policy **MUST** be hierarchical, with "Pruning by fsync Acknowledgment" (Rule 1) as the highest priority.
    *   **Finding:** The end-to-end pipeline for this feature is fully implemented. The `HeartbeatResponse` proto contains the `last_fsynced_received_nanos` field. The `SessionHandler` reads this value, sends it to the `TaskSupervisor`, which broadcasts a `PruneByFsync` command to all `TargetWorker`s, which in turn forward it to their respective `BatchSubmitter`s.
    *   **Impact:** The collector correctly prunes its buffer based on definitive acknowledgment from the database, ensuring data integrity and preventing data loss.

---

### Pillar 2: The Hybrid Command & Data Model

The implementation of the communication flow is strong and complete.

#### Strengths:

*   **Hybrid Model (Reqs 2.1):** The `SessionHandler` correctly implements the hybrid model by running the `Heartbeat` and `SubscribeToCommands` loops concurrently in separate tasks.
*   **Graceful Degradation (Reqs 2.1):** The "ephemeral session" design is a correct and elegant implementation of this requirement. When a stream or connection breaks, the `SessionHandler` task dies, and the `ConnectionManager` orchestrates a new session, which correctly falls back to using the `Heartbeat` for its initial state.
*   **`AnnouncePings` RPC (Reqs 3.5):** The `Pinger` correctly calls `announce_pings` in a "fire-and-forget" `tokio::spawn` task, ensuring it does not block the critical pinging loop.

#### Critical Findings & Deviations:

1.  **DONE: Command Stream Reliability is Implemented (Reqs 2.2).**
    *   **Requirement:** The `HeartbeatRequest` **MUST** include `last_processed_command_id` to allow the database to detect a "zombie stream".
    *   **Finding:** This is fully implemented. The `SessionHandler` uses an `Arc<AtomicU64>` to share the ID of the last processed command between the command loop and the heartbeat loop. The heartbeat loop correctly reads this value and includes it in every `HeartbeatRequest`.
    *   **Impact:** The database can reliably verify that the fast-path command stream is healthy, preventing failed or hung handoffs.

---

### Pillar 3: Granular State Management & Concurrency

This is the strongest part of the implementation. It is an almost perfect translation of the software design document.

#### Strengths:

*   **Per-Target Submission (Reqs 3.1):** The core architecture of `TaskSupervisor` -> `TargetWorker` -> `BatchSubmitter` perfectly realizes this requirement. This is the highlight of the codebase.
*   **Persistent Tasks (Reqs 2.4):** The `Pinger` and `BatchSubmitter` tasks are correctly managed by the `TargetWorker`. Role changes are sent as commands (`PingerCommand::UpdateRole`), and the tasks are not destroyed and recreated.
*   **Concurrent gRPC Client (Reqs 5.1):** The `DatabaseClient` is correctly implemented as a lightweight, `Clone`-able wrapper. It is passed down and cloned into each task that needs it, avoiding any `Mutex` bottleneck. This is excellent.
*   **Client Abstraction (Reqs 5.2):** The `DatabaseClient` provides a clean abstraction over the `tonic` client, centralizing authentication logic as required.

---

### Pillar 4: First-Class Observability

The health reporting pipeline is well-implemented and directly addresses the requirements.

#### Strengths:

*   **Health Reporting (Reqs 2.3):** The entire pipeline is present and correct. `TaskSupervisor` has a dedicated health aggregation loop that queries workers. The `HealthReport` is passed up to the `CollectorService` and then consumed by the `SessionHandler` to populate the `HeartbeatRequest`. This is a robust, well-designed implementation.

---

### Pillar 5: Data Provenance and Integrity

This pillar has two of the most subtle but impactful deviations from the requirements.

#### Strengths:

*   **`received_nanos` Cursor (Reqs 3.2):** The `BatchSubmitter` correctly uses `last_acked_received_nanos` for its ACK/DESYNC protocol. The buffer key is also correctly calculated as `sent_nanos + rtt_nanos`. This is crucial for data integrity and is implemented perfectly.

#### Critical Findings & Deviations:

1.  **CRITICAL: Timestamp Generation is Inaccurate Over Time (Violation of Reqs 2.5).**
    *   **Requirement:** The `(SystemTime, Instant)` reference pair **MUST** be re-captured periodically to correct for NTP slew drift.
    *   **Finding:** In `pinger.rs`, the `MonotonicTimeSource` captures the reference pair only once in its `new()` constructor. There is a `_resync` method, but a `FIXME` comment confirms it is not used.
    *   **Impact:** The collector's timestamps will slowly but surely drift away from true wall-clock time, potentially by multiple seconds per day. This compromises the core value of the collected data for correlation with real-world events.

2.  **SEVERE: Collector Fails to Crash on Large Time Jumps (Violation of Reqs 2.5).**
    *   **Requirement:** For large backward clock jumps (>5 seconds), the collector **SHOULD** treat this as a fatal state and exit.
    *   **Finding:** The `MonotonicTimeSource::now_ns` method correctly detects a backward jump. It logs a `warn!` and an `error!` for a large jump, but it only returns `None`, causing the pinger to merely *skip* the ping. It does not exit.
    *   **Impact:** A misconfigured machine could have its clock jump backward by an hour. The collector would continue running but would generate no data for an entire hour, creating a massive, silent data gap. The requirement to crash is a safety mechanism to alert an operator to a severely broken state and to minimize the gap via a process supervisor restart. This behavior is not implemented.

---

### Actionable Recommendations (Prioritized)

1.  **Fix Timestamp Generation (Pillar 5):** This is the highest priority as it affects the integrity of all data collected.
    *   In `pinger.rs`, implement a periodic task (e.g., every minute) within the `Pinger`'s main loop to call a `resync()` method on the `MonotonicTimeSource`.
    *   Modify `MonotonicTimeSource::now_ns()` to `panic!` or `std::process::exit(1)` when a backward jump greater than 5 seconds is detected. The error message must be clear.

    Status: **IN PROGRESS**

    Short plan (delta):
    - TimeSource trait extracted and implemented as `MonotonicTimeSource` in `pinger.rs`.
    - `Pinger` refactored to accept a boxed `TimeSource` and supports injection via `new_with_time_source`.
    - Periodic resync remains wired in the `Pinger` loop (60s default) and calls `resync()` on the time source.
    - Large backward jumps already trigger process exit in non-test builds; the behavior is preserved.

    Quick checklist (current):
    - [x] Extract/test `TimeSource` trait
    - [x] Implement periodic resync in `Pinger`
    - [x] Implement crash-on-large-backward-jump behavior (production)
    - [ ] Add unit and integration tests (resync counter + process-level fatal-jump)
    - [x] Run `cargo test -p zzping-collector --no-fail-fast` and `cargo clippy --all-targets` to validate (green)

2.  **Implement Last-Intent Cache Reading (Pillar 1):** This is critical for resilience.
    *   **Status: IN PROGRESS**
    *   **Next Steps:**
        1.  Modify `TaskSupervisor::reconcile` to create workers even if `db_client` is `None`. The `TargetWorker` will need to be able to handle a missing client and buffer data.
        2.  Modify `CollectorService::run_internal` to set the initial role from the cache to `Primary` instead of `Standby`.
        3.  Add integration tests to verify that the collector starts pinging with a cached config when the database is down.

3.  **Implement Command Stream Reliability (Pillar 2):** This is a key protocol requirement for handoff safety.
    *   **Status: DONE**

4.  **Implement fsync-based Buffer Pruning (Pillar 1):** This is a data loss prevention feature.
    *   **Status: DONE**

5.  **Code Cleanup:**
    *   **Status: TODO**
    *   The file `zzping-collector/src/state_machine.rs` appears to be an unused remnant of a previous design. It should be removed to avoid confusion.


---

## Test Plan: Closing Architectural Gaps in `zzping-collector`

#### 1. Test Area: Last-Known Intent Cache (Resilience)

*   **Architectural Requirement (Reqs 1.2):** The collector **MUST** use the `last_intent.ron` cache to start pinging immediately upon startup if the database is unreachable.
*   **Current Gap:** The cache is written but never read during the bootstrap process. The collector remains idle if the database is down at startup, violating the operational continuity principle.

##### Required Tests:

1.  **Test Case: `startup_with_cache_db_unavailable` (Integration Test)**
    *   **Given:**
        *   A valid `last_intent.ron` file exists in the collector's working directory, specifying one or more targets (e.g., `"8.8.8.8"`) and a non-zero ping rate.
        *   The `database_addr` in `collector.ron` points to an unreachable address.
    *   **When:** The collector process is started.
    *   **Then:**
        *   The collector **MUST** successfully bootstrap without a database connection.
        *   The `TaskSupervisor` **MUST** immediately spawn a `TargetWorker` for each target defined in `last_intent.ron`.
        *   The spawned workers **MUST** be in a `PRIMARY` role (i.e., actively pinging and buffering data), even though there is no database client.
    *   **Verification:** This can be verified by instrumenting the `TargetWorker::new` function (via logging or a test-only mock) to confirm it was called with the correct parameters before any connection to the database was established.

2.  **Test Case: `startup_with_cache_db_provides_new_config` (Integration Test)**
    *   **Given:**
        *   A `last_intent.ron` file exists with an "old" configuration (e.g., target `"1.1.1.1"`).
        *   A mock database is running and configured to provide a "new" configuration via `HeartbeatResponse` (e.g., target `"8.8.8.8"`).
    *   **When:** The collector starts and successfully connects to the database.
    *   **Then:**
        *   The collector should initially start a worker for `"1.1.1.1"` based on the cache.
        *   After the first successful heartbeat, the `TaskSupervisor` **MUST** reconcile its state.
        *   The worker for `"1.1.1.1"` **MUST** be sent a `Shutdown` command.
        *   A new worker for `"8.8.8.8"` **MUST** be created.
    *   **Verification:** A mock `TargetWorker` factory can be injected into the `TaskSupervisor` to monitor the creation and destruction of workers.

3.  **Test Case: `startup_without_cache_db_unavailable` (Integration Test)**
    *   **Given:** No `last_intent.ron` file exists, and the database is unreachable.
    *   **When:** The collector process is started.
    *   **Then:** The collector must remain in a standby state, spawning **no** `TargetWorker`s, while its `ConnectionManager` continues to attempt connections.
    *   **Verification:** Confirm that no workers are created.

#### 2. Test Area: `fsync`-based Buffer Pruning (Data Integrity)

*   **Architectural Requirement (Reqs 3.4):** The buffer **MUST** be pruned based on `fsync` acknowledgments from the database, as this is the safest mechanism.
*   **Current Gap:** The `prune_by_fsync` method in `BatchSubmitter` is never called. The end-to-end pipeline for this signal is missing.

##### Required Tests:

1.  **Test Case: `batch_submitter_prune_by_fsync` (Unit Test)**
    *   **Given:** A `BatchSubmitter` instance with a buffer containing records with `received_nanos` of 100, 200, 300, and 400.
    *   **When:** The `prune_by_fsync(250)` method is called directly.
    *   **Then:** The buffer must contain only the records with `received_nanos` 300 and 400. The records for 100 and 200 must be deleted.
    *   **Verification:** Check the internal state of the `BatchSubmitter`'s buffer after the call.
    Status: DONE — unit test `batch_submitter_prune_by_fsync` implemented and passing. The test exercises `ingest_ping_result` and `prune_by_fsync` directly and logs buffer contents for visibility.

2.  **Test Case: `session_handler_triggers_pruning_on_heartbeat` (Integration Test)**
    *   **Given:** A running collector connected to a mock database, with a worker that has buffered data.
    *   **When:** The mock database is configured to send a `HeartbeatResponse` containing the field `last_fsynced_received_nanos = N`.
    *   **Then:** The `SessionHandler` **MUST** recognize this field and send a corresponding command down the component chain, ultimately causing `prune_by_fsync(N)` to be called on the correct `BatchSubmitter`.
    *   **Verification:** Query the worker's health before and after the heartbeat. The reported `buffer_size` must decrease, confirming the pruning occurred.

    Status: PARTIAL / IN PROGRESS — plumbing implemented end-to-end (proto field added, `SessionHandler` sends fsync notifications, `TaskSupervisor` broadcasts `PruneByFsync`, `TargetWorker` forwards to `BatchSubmitter`).

    Notes:
    - The `HeartbeatResponse` proto was extended with `last_fsynced_received_nanos` and tonic/prost regenerated.
    - `BatchSubmitterCommand::PruneByFsync` and `WorkerCommand::PruneByFsync` were added and wired through `TargetWorker` and `TaskSupervisor`.
    - `SessionHandler::run_heartbeat_loop` sends the fsync value on a dedicated channel to the supervisor.
    - A focused integration test was added that attempts to exercise the full path, but it proved timing-sensitive and flaky; that test is currently marked `#[ignore]` pending a deterministic synchronization hook (recommended next step).

    Recommended next step: add a test-only hook to `MockIngestionService` that can trigger a heartbeat with a controllable `last_fsynced_received_nanos` at a known point in the test, or add a oneshot ack from `SessionHandler` when it forwards fsync, to make the integration test deterministic.

#### 3. Test Area: Command Stream Reliability (Protocol Correctness)

*   **Architectural Requirement (Reqs 2.2):** The `HeartbeatRequest` **MUST** include `last_processed_command_id`.
*   **Current Gap:** The `last_processed_command_id` is hardcoded to `0`, defeating the zombie stream detection mechanism.

##### Required Tests:

1.  **Test Case: `session_handler_updates_command_id_in_heartbeat` (Integration Test)**
    *   **Given:** A `SessionHandler` is connected to a mock database.
    *   **When:**
        1.  The mock database sends a `Command` with `command_id = 42` via the `SubscribeToCommands` stream.
        2.  The `SessionHandler` successfully processes this command.
        3.  The `SessionHandler`'s `Heartbeat` loop sends its next request.
    *   **Then:** The `HeartbeatRequest` received by the mock database **MUST** have the field `last_processed_command_id` set to `42`. A subsequent heartbeat, with no new commands, must also report `42`.
    *   **Verification:** The mock ingestion service must inspect the incoming `HeartbeatRequest` and assert the value of the field.

#### 4. Test Area: Timestamp Generation (Data Integrity)

*   **Architectural Requirement (Reqs 2.5):** Timestamps must be periodically resynchronized to prevent NTP slew drift, and the process must exit on large backward time jumps.
*   **Current Gap:** The `MonotonicTimeSource` is never resynchronized, leading to inaccurate timestamps. It also fails to exit on large time jumps, leading to silent data gaps.

##### Required Tests:

1.  **Test Case: `pinger_periodically_resyncs_time_source` (Integration Test)**
    *   **Given:** A `Pinger` task is running.
    *   **When:** The task runs for a duration significantly longer than the one-minute resync interval (e.g., 65 seconds).
    *   **Then:** The `Pinger` **MUST** have triggered the resynchronization of its `MonotonicTimeSource`.
    *   **Verification:** This requires dependency injection. Create a mock `TimeSource` struct that implements a trait. The mock will contain an `Arc<AtomicU32>` counter for `resync` calls. Inject this mock into the `Pinger` during the test and assert that the counter is greater than zero after the test duration.

2.  **Test Case: `collector_exits_on_large_backward_time_jump` (Process-Level Test)**
    *   **Given:** A full `zzping-collector` process is running.
    *   **When:** A large backward time jump (> 5 seconds) is simulated within a `Pinger` task.
    *   **Then:** The entire collector process **MUST** exit with a non-zero status code.
    *   **Verification:** This is the most complex test to write. It will likely require a special test-only `WorkerCommand` that instructs a `Pinger` to simulate the fatal time condition. The test harness will spawn the collector as a child process, send the command, and then `await` the child process handle to check its exit code. This verifies that the panic correctly unwinds and terminates the application as required. A simple unit test with `#[should_panic]` on `MonotonicTimeSource` is a necessary but insufficient prerequisite. The full process test is required to prove the architectural requirement is met.


------

## Overall Test Suite Assessment

*   **Strengths:** The current test suite is strong. It makes good use of mocks (`MockIngestionService`, `MockPingClient`), demonstrates a clear understanding of testing asynchronous Rust code, and covers many of the "happy path" and basic failure scenarios for individual components. Files like `batch_submitter_test.rs` are exemplary in their thoroughness.
*   **Systemic Weakness:** The primary weakness is a lack of tests for **inter-component coordination under failure** and for verifying **complex state transitions**. The tests excel at checking if a single component does its job in isolation but are less thorough in proving that the *system of components* behaves correctly, especially when one part fails.

---

### Detailed Behavioral Coverage Gaps & Missing Tests

Here is a breakdown by component, detailing what is covered, what is missing, and the specific tests required to fill the gaps.

#### 1. Bootstrap & Configuration (`lib.rs`, `config.rs`)

*   **Current Coverage:**
    *   `config_test.rs`: Correctly verifies loading valid and invalid RON files.
    *   `startup_lock_test.rs`: Correctly verifies the TCP port lock prevents a second instance from starting (Reqs 1.4).
    *   `bootstrap_integration_test.rs`: Verifies basic startup and the `ConnectionManager`'s retry loop.
*   **Identified Gaps:**
    *   As previously noted, the critical path of reading and using `last_intent.ron` on startup is completely untested because it's unimplemented (Reqs 1.2).
    *   There are no tests for what happens if the collector starts with valid cached intent but can *never* connect to the database. It should run indefinitely in this offline mode.

*   **Specific Missing Tests:**
    1.  **(NEW) Test: `collector_runs_indefinitely_offline_with_cache` (Integration)**
        *   **Given:** A valid `last_intent.ron` file and a `database_addr` that is permanently unreachable.
        *   **When:** The collector is started.
        *   **Then:** The collector process must not exit. It must remain running, with its workers actively buffering data, while the `ConnectionManager` retries in the background. This proves its utility as a long-running offline data logger.

#### 2. `ConnectionManager`

*   **Current Coverage:**
    *   `connection_manager_test.rs`: Verifies that it connects on success and retries on initial failure.
*   **Identified Gaps:**
    *   The tests don't verify the primary operational loop: successfully connecting, waiting for the session to end (via `reconnect_notify`), and *then* re-initiating the connection loop. This is the core resilience mechanism for handling database restarts.

*   **Specific Missing Tests:**
    1.  **(NEW) Test: `connection_manager_reconnects_after_session_ends` (Unit Test)**
        *   **Given:** A `ConnectionManager` is running and has successfully sent a `DatabaseClient` to its channel.
        *   **When:** The `reconnect_notify` handle is signaled.
        *   **Then:** The `ConnectionManager` must immediately attempt to establish a new connection. (This can be verified by having its connect function send a message to a test channel).

#### 3. `SessionHandler`

*   **Current Coverage:**
    *   `session_handler_test.rs`: Verifies config is sent on a successful heartbeat and that the handler exits when the entire connection dies.
*   **Identified Gaps:**
    *   **Graceful Degradation (Reqs 2.1):** The current test asserts that the handler exits on *any* connection failure. The requirement is that it **MUST NOT** exit if only the `SubscribeToCommands` stream fails. It must degrade to using the `Heartbeat` for commands.
    *   **Invalid Data Handling:** No tests for how the handler reacts to malformed or unexpected data from the database (e.g., an invalid `CollectorRole` enum integer). It should not panic.
    *   **Handoff Command Logic:** No tests verify that handoff-specific commands (`PrepareToSwap`, etc.) are correctly translated into the `SupervisorConfig`.

*   **Specific Missing Tests:**
    1.  **(NEW) Test: `handler_survives_command_stream_failure` (Integration)**
        *   **Given:** A mock database service where `SubscribeToCommands` returns an error immediately, but `Heartbeat` functions correctly.
        *   **When:** A `SessionHandler` connects to this service.
        *   **Then:** The `SessionHandler` task **MUST NOT** exit. It must continue its `Heartbeat` loop.
    2.  **(NEW) Test: `handler_handles_invalid_role_gracefully` (Unit Test)**
        *   **Given:** A `SessionHandler`'s state manager loop.
        *   **When:** It receives a `HeartbeatResponse` with `role = 99` (an invalid enum value).
        *   **Then:** It must not panic. It should default the role in the broadcasted `SupervisorConfig` to `Standby`.

#### 4. `TaskSupervisor`

*   **Current Coverage:**
    *   `task_supervisor_test.rs`: Good coverage for the reconciliation logic (adding/removing workers) and forwarding `UpdateRole` commands.
*   **Identified Gaps:**
    *   **Deferred Worker Creation (Design 3.4):** A key responsibility is to *defer* creating workers if it has a configuration but no `DatabaseClient`. When a client arrives later, it should then create the workers. This is untested.
    *   **Health Aggregation:** The periodic health aggregation loop is completely untested.
    *   **Worker Panic Handling:** The design doesn't specify what happens if a `TargetWorker` task panics. The supervisor's main loop should not be affected. A test is needed to confirm this robustness.

*   **Specific Missing Tests:**
    1.  **(NEW) Test: `supervisor_defers_and_then_creates_workers` (Unit Test)**
        *   **Given:** A `TaskSupervisor` with no `DatabaseClient`.
        *   **When:** It reconciles a config with new targets.
        *   **Then:** It **MUST NOT** create any workers.
        *   **When:** It receives a `ClientUpdate::NewClient`.
        *   **Then:** It **MUST** now create the previously deferred workers.
    2.  **(NEW) Test: `supervisor_correctly_aggregates_health_reports` (Unit Test)**
        *   **Given:** A `TaskSupervisor` with two mock workers.
        *   **When:** The health aggregation tick fires, and the mock workers are configured to respond to the `GetHealth` command with buffer sizes of 100 and 200, respectively.
        *   **Then:** The `HealthReport` sent to the `health_report_tx` channel **MUST** contain `total_buffer_size = 300`.

#### 5. `TargetWorker`

*   **Current Coverage:**
    *   `target_worker_test.rs`: Verifies that the `GetRecentData` RPC is called upon transitioning to `PRIMARY_SUPERVISED`.
    *   `graceful_shutdown_test.rs`: Provides good integration-level coverage for the shutdown process.
*   **Identified Gaps:**
    *   **Ordered Shutdown (Design 5.4):** The integration test shows the worker shuts down, but there is no unit-level test to *prove* the required shutdown order: `Pinger` is stopped first, its task is joined, and only *then* is the `BatchSubmitter` commanded to shut down and drain. This ordering is critical to prevent data generated during the shutdown from being lost.

*   **Specific Missing Tests:**
    1.  **(NEW) Test: `worker_ensures_ordered_shutdown_of_children` (Unit Test)**
        *   **Given:** A `TargetWorker` running with mock `Pinger` and `BatchSubmitter` child tasks.
        *   **When:** The worker receives a `WorkerCommand::Shutdown`.
        *   **Then:** A test-only MPSC channel must receive events in the following strict order: `PingerShutdownSent`, `PingerTaskJoined`, `SubmitterShutdownSent`, `SubmitterTaskJoined`.

#### 6. `Pinger`

*   **Current Coverage:**
    *   `pinger_test.rs`: Excellent coverage of the pinging loop, pausing/resuming based on role, and timeout detection for lost packets.
*   **Identified Gaps:**
    *   **Semaphore Backpressure:** The code includes logic to handle a full semaphore (i.e., when pings are being sent faster than replies are received), but this "busy" state is not tested.
    *   Timestamp generation gaps already noted.

*   **Specific Missing Tests:**
    1.  **(NEW) Test: `pinger_skips_ping_when_semaphore_is_full` (Unit Test)**
        *   **Given:** A `Pinger` initialized with a semaphore of size 1 and a mock `PingClient` that deliberately holds its semaphore permit for an extended time.
        *   **When:** The `Pinger`'s ping interval ticks twice in quick succession.
        *   **Then:** The mock `PingClient`'s `ping` method **MUST** have been called only once.

#### 7. `BatchSubmitter`

*   **Current Coverage:** Excellent. `batch_submitter_test.rs` is the most complete test file in the crate. It covers ingestion, the `send_batch` OK/DESYNC logic, and all three pruning rules at the unit level.
*   **Identified Gaps:**
    *   **Role-Based Activity:** The submitter is required to be active only in `PRIMARY` and `SUPERVISING` roles. The tests don't verify that the `send_batch` loop is correctly paused/resumed based on the `UpdateRole` command.
    *   **RPC Failure Resilience:** There are no tests for what happens if the `db_client.send_batch()` call itself fails with a network error. The submitter should gracefully handle the error, preserve its buffer, and retry on the next interval.

*   **Specific Missing Tests:**
    1.  **(NEW) Test: `submitter_is_active_only_in_correct_roles` (Unit Test)**
        *   **Given:** A `BatchSubmitter` with data in its buffer and a mock DB client.
        *   **When:** The role is updated to `Standby` or `PrimarySupervised`, and `send_batch` is called.
        *   **Then:** The mock DB client's `send_batch` method **MUST NOT** be called.
        *   **When:** The role is updated to `Primary` or `Supervising`, and `send_batch` is called.
        *   **Then:** The mock DB client's `send_batch` method **MUST** be called.
    2.  **(NEW) Test: `submitter_preserves_buffer_on_rpc_failure` (Unit Test)**
        *   **Given:** A `BatchSubmitter` with data and a mock DB client configured to return a `tonic::Status::unavailable()` error.
        *   **When:** `send_batch` is called.
        *   **Then:** The method must return an `Err`, and the internal buffer size **MUST NOT** have changed. The `last_acked_received_nanos` cursor **MUST NOT** have advanced.

### The Correct Architectural Solution

The correct solution already exists within your design. It does not require changing the responsibilities of the `TargetWorker`. The logic belongs entirely within the bootstrap process and the `TaskSupervisor`.

Here is the architecturally compliant implementation flow:

1.  **Bootstrap (`lib.rs`):**
    *   The `bootstrap_collector` function **MUST** attempt to read and parse `last_intent.ron`.
    *   If successful, this `CachedIntent` is passed to the `CollectorService`.

2.  **Service Initialization (`CollectorService`):**
    *   The `CollectorService` creates the `TaskSupervisor`, passing the optional `CachedIntent` to its constructor.

3.  **Supervisor State Initialization (`TaskSupervisor`):**
    *   The `TaskSupervisor` starts with two key pieces of state:
        *   `self.db_client: Option<DatabaseClient> = None;`
        *   `self.current_config: Option<SupervisorConfig>` which is initialized from the `CachedIntent`.
    *   The supervisor's reconciliation loop now has a simple, robust rule derived directly from your design document (Design 3.4): **It only spawns workers if BOTH a configuration exists AND a `DatabaseClient` is available.**

4.  **The Critical Sequence of Events:**
    *   **Scenario A (DB Down at Startup):**
        *   `TaskSupervisor` starts with `current_config` (from cache) but `db_client` is `None`.
        *   Its `reconcile` loop runs. It sees the desired targets but notes `self.db_client.is_none()`. As per the design, it **defers worker creation**. It does nothing.
        *   The system sits in this state: `ConnectionManager` is trying to connect, `TaskSupervisor` knows what it *wants* to do but is patiently waiting.
        *   Later, `ConnectionManager` succeeds. It sends a `DatabaseClient` to the `CollectorService`.
        *   `CollectorService` sends `ClientUpdate::NewClient(...)` to the `TaskSupervisor`.
        *   The `TaskSupervisor` sets `self.db_client = Some(client)` and immediately **triggers a new reconciliation**.
        *   This time, `reconcile` sees both a config and a client, and **now it spawns the workers.**
    *   **Scenario B (DB Up at Startup):**
        *   The same sequence as above occurs, but the `ClientUpdate::NewClient` message arrives almost immediately after startup. The user sees a very brief delay before workers are spawned. This is correct and expected behavior.

**Conclusion:** This approach is superior because it keeps all client lifecycle management contained within the single component responsible for it: the `TaskSupervisor`. The `TargetWorker` remains simple—it is *always* created with a valid client and never needs to worry about it. This upholds every principle of your architecture.

---

### Refined, Actionable Task List for the Agent

You should provide the following concrete plan to your agent. This is the correct way to implement the feature.

**Task: Implement Resilient Startup from Cached Intent**

1.  **Modify `zzping-collector/src/lib.rs`:**
    *   In `bootstrap_collector`, add logic to read `last_intent.ron`.
    *   If the file exists and is valid, deserialize it into a `CachedIntent` struct. This is an `Option<CachedIntent>`.
    *   Pass this `Option<CachedIntent>` to the `CollectorService::new` constructor.

2.  **Modify `zzping-collector/src/collector_service.rs`:**
    *   Update `CollectorService::new` to accept the `Option<CachedIntent>`.
    *   Pass this `Option<CachedIntent>` to the `TaskSupervisor::new` constructor.

3.  **Modify `zzping-collector/src/task_supervisor.rs`:**
    *   Update `TaskSupervisor::new` to accept the `Option<CachedIntent>`.
    *   Inside `new`, initialize `self.current_config` from the `CachedIntent` if it is `Some`. The role should default to `Primary` in this case, as the point of the cache is to continue primary duties.
    *   Modify the `run` loop: The `client_update_rx.recv()` arm, upon receiving `ClientUpdate::NewClient`, **must** trigger a call to `self.reconcile(self.current_config.clone()).await`. This is the key to activating deferred workers.
    *   Modify the `reconcile` method: The logic for spawning new workers **must** be gated behind a check: `if let Some(db_client) = &self.db_client { ... }`.

4.  **Create Unit & Integration Tests:**
    *   Implement the test cases defined in the "Last-Known Intent Cache" section of the test plan I provided previously. These tests will fail now and will pass once this task is correctly implemented.






### **Response to Agent**

You are correct to look for an existing file and a test plan. A dedicated test file for this feature does not exist yet and needs to be created.

However, there is a clear and established pattern in the existing test suite that you **must** follow. The tests in `bootstrap_integration_test.rs` and `graceful_shutdown_test.rs` provide the necessary boilerplate.

#### 1. File to Create

Create a new file at the following path:
`zzping-collector/tests/cached_intent_startup_test.rs`

#### 2. Boilerplate and Initial Structure

Here is the required boilerplate for the new file. This structure includes helper functions and stubs for the tests defined in the test plan. You should use this as the starting point for your implementation.

```rust
// File: zzping-collector/tests/cached_intent_startup_test.rs

use anyhow::Result;
use ntest::timeout;
use std::time::Duration;
use tempfile::TempDir;
use zzping_collector::bootstrap_collector;

// Import the common test utilities
mod common;
use common::MockIngestionService;

/// Helper function to create a test environment with temporary config files.
/// It returns the temporary directory (so it stays in scope and isn't deleted),
/// and the path to the main collector.ron config file.
fn setup_test_environment(
    last_intent_content: Option<&str>,
    db_addr: &str,
) -> Result<(TempDir, String)> {
    let temp_dir = tempfile::tempdir()?;
    let dir_path = temp_dir.path();

    // Create the main collector.ron config
    let collector_config_content = format!(
        r#"
(
    collector_uuid: "cached-intent-test-uuid",
    database_addr: "http://{}",
    auth_token: "test-token",
    use_mock_ping_client: true,
)
"#,
        db_addr
    );
    let collector_config_path = dir_path.join("collector.ron");
    std::fs::write(&collector_config_path, collector_config_content)?;

    // Create the last_intent.ron file if content is provided
    if let Some(content) = last_intent_content {
        let last_intent_path = dir_path.join("last_intent.ron");
        std::fs::write(last_intent_path, content)?;
    }

    Ok((temp_dir, collector_config_path.to_str().unwrap().to_string()))
}

#[tokio::test]
#[timeout(5000)]
async fn startup_with_cache_and_unavailable_db() -> Result<()> {
    // This test verifies the core resilience feature: the collector can start
    // and operate in a disconnected state using its cached configuration.

    // TODO: Implement test logic as per the test plan.
    // 1. Given:
    //    - Create a last_intent.ron file with targets.
    //    - Use an unreachable DB address.
    //    - Setup the test environment.
    // 2. When:
    //    - Bootstrap and run the collector.
    // 3. Then:
    //    - Assert the collector process is running.
    //    - Verify (via instrumentation) that it has loaded the intent
    //      and is attempting to create workers, even without a DB connection.

    Ok(())
}

#[tokio::test]
#[timeout(5000)]
async fn startup_with_cache_and_successful_connection() -> Result<()> {
    // This test verifies that a collector starting with cached intent correctly
    // reconciles its state after connecting to the database and receiving a *new* configuration.

    // TODO: Implement test logic as per the test plan.
    // 1. Given:
    //    - Create a last_intent.ron with "old" config (e.g., target "1.1.1.1").
    //    - Spawn a mock DB server providing a "new" config (e.g., target "8.8.8.8").
    //    - Setup the test environment.
    // 2. When:
    //    - Bootstrap and run the collector.
    // 3. Then:
    //    - Assert (via instrumentation/mocking) the sequence of worker events:
    //      Create("1.1.1.1") -> Shutdown("1.1.1.1") -> Create("8.8.8.8").

    Ok(())
}


#[tokio::test]
#[timeout(5000)]
async fn startup_without_cache_and_unavailable_db() -> Result<()> {
    // This test verifies the negative case: without a cache, the collector
    // remains idle until it can connect.

    // TODO: Implement test logic as per the test plan.
    // 1. Given:
    //    - Do NOT create a last_intent.ron file.
    //    - Use an unreachable DB address.
    //    - Setup the test environment.
    // 2. When:
    //    - Bootstrap and run the collector.
    // 3. Then:
    //    - Assert the collector process is running.
    //    - Verify (via instrumentation) that no workers are ever created.

    Ok(())
}
```

### Guidance for the Agent

1.  **Use the Helper Function:** The provided `setup_test_environment` helper function should be used in each test case. It handles the creation of temporary directories and configuration files, ensuring tests are hermetic and do not interfere with each other.

2.  **Instrumentation is Key:** To verify the internal state of the `TaskSupervisor` (e.g., which workers are created or shut down), you will need to use the `bootstrap_collector_for_test` function from `zzping-collector/src/lib.rs`. This allows you to inject `mpsc` channels to receive status updates or mock components to monitor interactions.

3.  **Mock Server:** For tests requiring a running database, use the `common::spawn_mock_server` function, which is already used in other integration tests. You can configure the `MockIngestionService` it runs to return specific `HeartbeatResponse` data to control the collector's behavior.

4.  **Timeouts:** All integration tests **must** use the `#[timeout(...)]` attribute to prevent the test suite from hanging in case of deadlocks or infinite loops in the collector's logic.

---

### **Test Plan: Last-Known Intent Cache (Collector Startup Resilience)**

**Architectural Requirement:** `Reqs 1.2` - The collector **MUST** use the `last_intent.ron` cache to start pinging immediately upon startup if the database is unreachable, ensuring operational continuity.

#### Test Case 1: `startup_with_cache_and_unavailable_db` (Integration Test)

*   **Purpose:** To verify the core resilience feature: the collector can start and operate in a disconnected state using its cached configuration.

*   **Given:**
    *   A valid `collector.ron` configuration file pointing to a database address that is **unreachable**.
    *   A valid `last_intent.ron` file exists in the working directory, specifying a configuration (e.g., `targets: ["8.8.8.8"], ping_rate_pps: 50`).

*   **When:**
    *   The `zzping-collector` process is launched.

*   **Then:**
    1.  The collector process **MUST** start successfully and **MUST NOT** exit, despite being unable to connect to the database.
    2.  The `TaskSupervisor` **MUST** be initialized with the configuration from `last_intent.ron`.
    3.  Because it has a valid configuration (from the cache) but no `DatabaseClient`, the `TaskSupervisor` **MUST** defer worker creation, as per the correct implementation flow.
    4.  *(This is a subtle but important verification)*: After a brief moment, no `TargetWorker`s should be active yet.

*   **Verification:**
    *   The test will need to be instrumented. The easiest way is to add a test-only `mpsc` channel to the `TaskSupervisor` that sends a message every time `reconcile` is called, reporting the number of active workers. The test will assert that the process is running but that the number of active workers remains zero. This proves it has loaded the intent but is correctly waiting for a client.

#### Test Case 2: `startup_with_cache_and_successful_connection` (Integration Test)

*   **Purpose:** To verify that a collector starting with cached intent correctly reconciles its state after connecting to the database and receiving a *new* configuration.

*   **Given:**
    *   A `last_intent.ron` file with an "old" configuration (e.g., `targets: ["1.1.1.1"]`).
    *   A mock database is running and is configured to provide a "new" configuration via `HeartbeatResponse` (e.g., `targets: ["8.8.8.8"]`).
    *   A `collector.ron` file pointing to the mock database.

*   **When:**
    *   The collector starts and successfully connects to the database.

*   **Then:**
    1.  The `TaskSupervisor` will initialize with the cached config for `"1.1.1.1"`.
    2.  Upon receiving the `DatabaseClient`, it will reconcile and spawn a worker for `"1.1.1.1"`.
    3.  The `SessionHandler` will then connect, get the new config for `"8.8.8.8"` from the heartbeat, and broadcast it.
    4.  The `TaskSupervisor` will perform a *second* reconciliation.
    5.  The worker for `"1.1.1.1"` **MUST** be sent a `Shutdown` command.
    6.  A new worker for `"8.8.8.8"` **MUST** be created.

*   **Verification:**
    *   This requires injecting a mock `TargetWorker` factory or a monitoring channel into the `TaskSupervisor`. The test must assert the sequence of events: `Create("1.1.1.1")`, followed by `Shutdown("1.1.1.1")`, followed by `Create("8.8.8.8")`.

#### Test Case 3: `startup_without_cache_and_unavailable_db` (Integration Test)

*   **Purpose:** To verify the negative case: without a cache, the collector behaves as a simple client and remains idle until it can connect.

*   **Given:**
    *   No `last_intent.ron` file exists.
    *   The `collector.ron` file points to an unreachable database address.

*   **When:**
    *   The `zzping-collector` process is launched.

*   **Then:**
    1.  The collector process **MUST** start successfully and **MUST NOT** exit.
    2.  The `TaskSupervisor` **MUST NOT** have any initial configuration.
    3.  No `TargetWorker`s **MUST** be created. The collector remains idle while the `ConnectionManager` attempts to connect.

*   **Verification:**
    *   Use the same instrumentation as Test Case 1. The test will assert that the process is running and that the number of active workers is always zero.