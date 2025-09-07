Read and Consider:

* `ZZPing_Architectural_Vision_II.md` - this is the overall vision of what we're going for.
* `ZZPing_Network_protocol.md` - this overrides the above and sets what do we want to build right now, but it's an overall design.
* `ZZPing_Collector_Architectural_Requirements.md` - Checklist of what a good architecture should have
* `ZZPing_Collector_Software_Architecture_Design.md` - The architecture we want to have for zzping-collector

## **Implementation Roadmap: Refactoring the Collector to the New Architecture**

**Guiding Principle:** Each step is a self-contained unit of work that should result in a Pull Request. The entire workspace must be in a clean, test-passing state upon completion of each step.

#### **Chapter 1: Laying the Foundation (The Un-crashable Core)**

**Goal:** Gut the old, fragile `Orchestrator` and replace it with the new, resilient `CollectorService` and its decoupled connection logic. At the end of this chapter, the collector will start, connect, and handle disconnects, but will not yet perform any pinging.

*   **Step 1.1: Create the Root Service & Supervisors.**
    *   **Tasks:**
        1.  Create the new `CollectorService` and `TaskSupervisor` structs.
        2.  Implement the `CollectorService` to own the `TaskSupervisor`.
        3.  Implement the shell of the `TaskSupervisor`'s reconciliation loop. For now, it will only log the actions it *would* take (e.g., "Received new config, would create worker for 1.1.1.1").
    *   **Verification:** New unit tests for the `TaskSupervisor`'s basic reconciliation logic pass.

*   **Step 1.2: Decouple the Connection Logic.**
    *   **Tasks:**
        1.  Create the new `ConnectionManager` task.
        2.  Implement its infinite connect/retry loop, which sends a `DatabaseClient` on a channel upon success.
    *   **Verification:** Unit tests confirm the `ConnectionManager` can be spawned and correctly attempts to connect.

*   **Step 1.3: Rewrite the Bootstrap Process.**
    *   **Tasks:**
        1.  Rewrite `main.rs` to bootstrap the new architecture: load config, acquire the port lock, and create/run the `CollectorService`.
        2.  The `CollectorService` will now spawn and manage the `ConnectionManager`.
        3.  Delete the old `orchestrator.rs` file and all related code.
    *   **Verification:** A new integration test starts the collector. It must successfully connect to a test database and log that it is "ready". The process must remain running and attempt to reconnect if the test database is shut down.

#### **Chapter 2: Implementing the Data Pipeline (The Workers)**

**Goal:** Build the self-contained `TargetWorker`s and their sub-components. This chapter focuses on the "upstream" data flow, from ping generation to data submission.

*   **Step 2.1: Implement the `TargetWorker`.**
    *   **Tasks:**
        1.  Create the `TargetWorker` struct and its main task loop.
        2.  Implement its command channel (`mpsc`) for receiving commands from the `TaskSupervisor`.
    *   **Verification:** Unit tests show a `TargetWorker` can be spawned and correctly responds to commands.

*   **Step 2.2: Build the Resilient `BatchSubmitter`.**
    *   **Tasks:**
        1.  Implement the new, per-target `BatchSubmitter`.
        2.  The internal buffer **MUST** be a `BTreeMap` keyed by `received_nanos`.
        3.  Implement the full, multi-tiered buffer management policy (pruning by `fsync` signal, hard limit, and time).
        4.  Implement the `SendBatch` loop with the `received_nanos` cursor and non-blocking `DESYNC` handling.
    *   **Verification:** Extensive, isolated unit tests for the `BatchSubmitter` are critical. These tests must cover all buffer pruning rules and the `DESYNC` recovery logic.

*   **Step 2.3: Implement the `Pinger` and Timestamp Generation.**
    *   **Tasks:**
        1.  Create the `Pinger` task.
        2.  Implement the absolute, monotonic timestamp generation logic (`SystemTime` + `Instant`) as required.
        3.  Implement the "fire-and-forget" `AnnouncePings` RPC call within the `Pinger`.
    *   **Verification:** Unit tests for the timestamp generation logic must prove it is both absolute and monotonic, correctly handling simulated clock drift and jumps.

#### **Chapter 3: Activating the System (The Session Handler)**

**Goal:** Wire the network-facing components to the stateful core. This chapter makes the collector fully interactive and responsive to the database.

*   **Step 3.1: Implement the Ephemeral `SessionHandler`.**
    *   **Tasks:**
        1.  Create the `SessionHandler` task.
        2.  Implement its concurrent `Heartbeat` and `SubscribeToCommands` loops.
        3.  Implement the logic to translate gRPC responses into `SupervisorConfig` updates.
        4.  Implement the broadcasting of these updates to the `TaskSupervisor` (`watch` channel) and the `CollectorService` (`mpsc` channel for persistence).
    *   **Verification:** Unit tests for the translation logic.

*   **Step 3.2: Implement the Health Reporting Pipeline.**
    *   **Tasks:**
        1.  Implement the "upstream" health reporting channels.
        2.  The `TaskSupervisor` gets its health aggregation loop.
        3.  The `CollectorService` acts as the hub.
        4.  The `SessionHandler` consumes the aggregated health report and includes it in `Heartbeat` requests.
    *   **Verification:** An integration test where a `TargetWorker`'s buffer size is changed, and the change is correctly reflected in an outgoing `HeartbeatRequest` captured from a test server.

*   **Step 3.3: Full Integration and Live Operation.**
    *   **Tasks:**
        1.  Connect all the pieces. The `TaskSupervisor` now receives real configs from the `SessionHandler` and spawns fully functional `TargetWorker`s.
    *   **Verification:** A full, end-to-end integration test. Start a test database, start the collector, change the database's `intent.ron` file, and verify that the collector dynamically starts and stops the correct pinging tasks.

#### **Chapter 4: Finalizing and Hardening**

**Goal:** Implement the final, most complex features of the system: graceful shutdown and the zero-downtime handoff.

*   **Step 4.1: Implement Graceful Shutdown.**
    *   **Tasks:**
        1.  Add the signal handler to `main.rs`.
        2.  Implement the `shutdown()` method on the `CollectorService` and the full, coordinated shutdown sequence across all components.
        3.  Implement the shutdown timeout to prevent the process from hanging.
    *   **Verification:** An integration test that starts the collector, sends it data, then sends a `SIGTERM`. The test must verify that the collector attempts to flush its buffers to the test database before exiting within the timeout.

*   **Step 4.2: Implement the Zero-Downtime Handoff Protocol.**
    *   **Tasks:**
        1.  Implement the full state machine logic for all handoff roles (`PRIMARY_SUPERVISED`, `SUPERVISING`) within the `TargetWorker`.
        2.  This requires close coordination with the implementation of the database's scheduler.
    *   **Verification:** This is the most complex test. It will likely require a multi-process test harness that can launch C1, launch C2, monitor the state of both, and verify that the handoff, verification, and final shutdown of C1 occur correctly. A separate test must verify the automatic rollback on a simulated failure of C2.