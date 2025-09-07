# ZZPing Collector: Software Architecture Design (v0.3)

**Author:** David Martínez Martí  
**Date:** Sep 7, 2025  
**Version:** v0.3

**Context:** This document is the definitive architectural blueprint for the zzping-collector service. It translates the formal requirements into a concrete, implementable software design. It supersedes all previous architectural concepts and diagrams. See [ZZPing Collector: Architectural Requirements (v0.3)](https://docs.google.com/document/d/1N6-IihKb43YbQrZtZK3_O7XUtZloxvi7hwnlTjdTeTU/edit?tab=t.0#heading=h.pjiwcyys6fu7)

---

# Chapter 1: Introduction

#### **1.1. Purpose**

To present the software architecture for a resilient, testable, and maintainable zzping-collector. This document serves as the definitive blueprint, translating the project's formal requirements into a concrete and implementable software design. It is the single source of truth for the collector's structure, component responsibilities, and internal communication patterns.

#### **1.2. Problem Statement**

The previous proof-of-concept implementation, while functional, suffered from a critical architectural flaw: a **tight coupling between volatile network state and essential, long-lived application state.** In that model, the core stateful components, including the in-memory data buffer, were tied to the lifecycle of the database connection.

This design choice had a catastrophic consequence: any transient network interruption or database restart would cause the entire collector state to be destroyed and re-created. This resulted in the guaranteed loss of all buffered data, a direct and severe violation of the project's "Robustness and Resilience First" guiding principle. The system was fundamentally fragile, failing to meet its primary objective of providing a gap-free, reliable data collection service. A complete architectural redesign is therefore necessary to build a foundation that is inherently resilient by design.

#### **1.3. Core Architectural Principles**

The new architecture is founded upon three core principles designed to directly address the flaws of the previous implementation. These principles create a system that is robust, predictable, and maintainable.

1. **Long-Lived State vs. Ephemeral Tasks**  
   This is the cornerstone of the new architecture. A strict, unbreachable wall is established between the application's permanent, stateful core and the temporary, stateless tasks that handle network I/O.  
   * **Long-Lived State:** Components that hold critical state, such as the TaskSupervisor and the BatchSubmitter data buffers, are created once at process startup and are **never destroyed**. They are shielded from the volatility of network connections.  
   * **Ephemeral Tasks:** Components responsible for network communication, such as the SessionHandler that manages the gRPC connection, are designed to be completely disposable. They are created for the duration of a single healthy connection and are expected to fail. Their failure and subsequent recreation have no impact on the long-lived state.  
2. **Clear Ownership**  
   The architecture employs a strict, hierarchical ownership model that mirrors Rust's own safety principles. Component lifecycles are managed through a clear, top-down chain of responsibility, which prevents circular dependencies and makes resource management predictable.  
   * The main function bootstraps a single, root CollectorService.  
   * The CollectorService owns the TaskSupervisor.  
   * The TaskSupervisor owns and manages the pool of TargetWorkers.  
   * This clear hierarchy ensures that stateful components always outlive the temporary tasks that interact with them, guaranteeing state preservation.  
3. **Separation of Concerns**  
   Each component in the architecture has a single, well-defined responsibility. This design choice makes the system highly modular, enabling individual components to be developed, tested, and reasoned about in isolation.  
   * The ConnectionManager is only responsible for establishing network connections.  
   * The SessionHandler is only responsible for handling gRPC protocols for a single session.  
   * The TaskSupervisor is only responsible for managing the lifecycle of its worker tasks.  
   * The TargetWorker is only responsible for the complete set of operations for a single ping target.  
     This separation makes the system more maintainable and less prone to complex, emergent bugs that arise from tangled responsibilities.

---

# Chapter 2: High-Level Architecture

This chapter presents the high-level structure of the zzping-collector. It introduces the primary software components and the communication patterns between them. The goal is to provide a clear, conceptual "map" of the system that serves as a reference for the detailed component breakdowns in the chapters that follow.

#### **2.1. System Diagram**

The architecture is a hierarchical system of supervised, asynchronous tasks. A permanent, long-lived CollectorService acts as the root of the application, owning all resilient state. It spawns and manages an ephemeral SessionHandler for each healthy database connection. This strict separation between the long-lived stateful core and the volatile network-facing tasks is the key to the system's resilience.

`┌────────────────────────────────────────────────────────────────────────┐`  
`│                          main.rs (Bootstrap)                           │`  
`│     Loads configuration, acquires the local TCP port lock for mutex.   │`  
`│                 Creates and runs the CollectorService.                 │`  
`└───────────────────────────────────┬────────────────────────────────────┘`  
                                    `│ Owns & Runs`  
                                    `▼`  
`┌────────────────────────────────────────────────────────────────────────┐`  
`│                           CollectorService                             │`  
`│       (Long-lived Root Supervisor - Owns all core state, never dies)   │`  
`│                                                                        │`  
``│  - Receives `DatabaseClient` from ConnectionManager.                   │``  
``│  - Receives `ConfigUpdate` for persistence from SessionHandler.        │``  
``│  - Receives aggregated `HealthReport` from TaskSupervisor.             │``  
``│  - Spawns `SessionHandler`, providing it with access to health data.   │``  
``│  - Sends `ClientUpdate` command to TaskSupervisor.                     │``  
`│                                                                        │`  
`└┬─────────────────┬─────────────────┬─────────────────┬─────────────────┘`  
 `│ Owns            │ Spawns          │ Forwards Health │ Updates Client`  
 `▼                 ▼                 │                 ▼`  
`┌──────────────┐ ┌───────────────┐   │   ┌────────────────────────────────┐`  
`│TaskSupervisor│ │SessionHandler │   │   │     Inter-Component Channels   │`  
`└──────────────┘ └───────────────┘   │   │                                │`  
 `▲                 │                 │   │ - ClientUpdate (mpsc)          │`  
 `│                 │                 │   │ - ConfigUpdate (watch)         │`  
 `│ Forwards Config │                 │   │ - ConfigPersistence (mpsc)     │`  
 `└─────────────────┴─────────────────┘   │ - HealthReport (mpsc)          │`  
                                         `└────────────────────────────────┘`  
 `│ Spawns & Supervises                   ▲`  
 `▼                                       │ Receives Client`  
`┌─────────────────────────┐     ┌────────────────────────────────────────┐`  
`│      TaskSupervisor     │     │        ConnectionManager (Task)        │`  
`│ (Long-lived State Owner)│     │     (Permanent, relentless connector)  │`  
`└──────────┬──────────────┘     └────────────────────┬───────────────────┘`  
           `│ Spawns & Commands                       │`  
           `▼                                         │`  
`┌───────────────────────────┐                        │ gRPC Calls`  
`│       TargetWorkers       │                        │`  
`│ (One per ping target)     ├────────────────────────►`  
`└───────────────────────────┘                        │`  
                                          `┌──────────────────────────────┐`  
                                          `│      zzping-database         │`  
                                          `└──────────────────────────────┘`  
 

#### **2.2. Component Summary**

Each component in the diagram has a single, clearly defined responsibility, reflecting the core principle of "Separation of Concerns."

* **CollectorService (The Root Supervisor):** The long-lived root of the application, owning all resilient states (via the TaskSupervisor) and supervising the network connection lifecycle.  
* **ConnectionManager (The Relentless Connector):** A permanent background task that endlessly attempts to establish a healthy, authenticated database connection, providing DatabaseClient handles to the CollectorService.  
* **SessionHandler (The Ephemeral gRPC Session):** An ephemeral task that manages all gRPC communication (Heartbeat, SubscribeToCommands) for the duration of a single, healthy connection, dying gracefully on any network error.  
* **TaskSupervisor (The Worker Pool Manager):** The long-lived manager of the worker pool, responsible for creating, destroying, and commanding workers to match the latest configuration received from the database.  
* **TargetWorker (The Self-Contained Workhorse):** A self-contained, independent task that owns all state and logic for monitoring a single target IP address, including its own Pinger and BatchSubmitter with its durable data buffer.  
* **DatabaseClient (The gRPC Abstraction):** A stateless, cloneable wrapper around the tonic gRPC client that centralizes request creation and authentication logic, enabling safe, concurrent access to the database.

---

# Chapter 3: Component Deep Dive

This chapter provides a detailed breakdown of each component introduced in the high-level architecture. It outlines their specific purpose, key responsibilities, internal state, and the rationale behind their design.

#### **3.1. The CollectorService (Root Supervisor)**

* **Purpose:** To serve as the permanent, un-crashable core of the application. It is the root of the ownership hierarchy and is responsible for owning all resilient state (via its child components) and supervising the overall connection lifecycle.  
* **Key Responsibilities:**  
  * **Own the TaskSupervisor:** It creates the TaskSupervisor instance once at startup and holds it for the entire lifetime of the process. This is the primary mechanism for state preservation.  
  * **Supervise the ConnectionManager:** It spawns the ConnectionManager task at startup and holds the receiving end of a channel to accept new, healthy DatabaseClients from it.  
  * **Manage Sessions:** For each new DatabaseClient it receives, it spawns a new, ephemeral SessionHandler task, providing it with all necessary channels to communicate with the rest of the system.  
  * **Mediate Communication:** It acts as a central hub for inter-component communication. It receives HealthReports from the TaskSupervisor and forwards them to the SessionHandler. It receives ConfigUpdates from the SessionHandler for persistence and ClientUpdates to the TaskSupervisor.  
  * **Persist Configuration:** It is the single component responsible for writing the last\_intent.ron cache to disk, ensuring that this critical I/O operation is handled by a long-lived, stable component.  
* **State / Ownership:**  
  * task\_supervisor: TaskSupervisor  
  * Communication channel senders and receivers for interacting with its child tasks.  
* **Rationale:** This two-tiered supervisor model is the architectural key to resilience. By completely separating the ownership of the stateful TaskSupervisor from the volatile, network-facing SessionHandler, the architecture guarantees that a network failure can *never* cause the data buffers to be dropped.

#### **3.2. The ConnectionManager (Connection Task)**

* **Purpose:** To relentlessly and resiliently provide healthy database connections to the CollectorService. Its entire existence is dedicated to this single task.  
* **Key Responsibilities:**  
  1. Run an infinite connect/retry loop, implementing a sensible backoff strategy (e.g., exponential backoff) to avoid flooding the network during prolonged outages.  
  2. Upon a successful connection, instantiate a new DatabaseClient.  
  3. Send the DatabaseClient handle to the CollectorService via its dedicated channel.  
  4. Wait for a signal that the session has ended (e.g., the SessionHandler dying and being dropped by the CollectorService) before looping to attempt reconnection.  
* **State / Ownership:** It is stateless beyond its loop variables and holds a clone of the application's core Config for the database address.  
* **Rationale:** This component isolates the complex and messy logic of connection management and retries. The rest of the system does not need to concern itself with connection state; it can simply assume that if it receives a DatabaseClient from the CollectorService, it is valid and ready to use.

#### **3.3. The SessionHandler (Ephemeral gRPC Task)**

* **Purpose:** To manage all gRPC communication for the duration of a single, healthy database connection. It is designed to be completely disposable.  
* **Key Responsibilities:**  
  1. Take ownership of a DatabaseClient for a single session.  
  2. Concurrently run the two core gRPC loops: the periodic Heartbeat RPC loop and the long-lived SubscribeToCommands streaming RPC.  
  3. Receive responses and commands from the database and translate them into coherent SupervisorConfig updates.  
  4. Broadcast new SupervisorConfig objects to the TaskSupervisor via a watch channel for immediate action.  
  5. Send new configurations to the CollectorService for persistence to last\_intent.ron.  
  6. Read the latest aggregated HealthReport from its channel to populate outgoing HeartbeatRequests.  
  7. **Exit immediately and completely** upon any unrecoverable gRPC error, which signals the end of the session.  
* **State / Ownership:** Holds the DatabaseClient for one session and the necessary communication channel senders/receivers.  
* **Rationale:** By making this component ephemeral, we radically simplify error handling. Instead of building complex reconnection logic inside the RPC loops, a failure is handled by simply letting the task die. Its death is a clean signal to the CollectorService that the connection is lost, triggering the ConnectionManager to begin its work again.

#### **3.4. The TaskSupervisor (Worker Pool Manager)**

* **Purpose:** To manage the pool of TargetWorker tasks, ensuring the set of running workers perfectly and continuously matches the latest configuration received from the database.  
* **Key Responsibilities:**  
  * Subscribe to the SupervisorConfig channel for live configuration updates.  
  * Maintain a HashMap mapping target IP addresses to their running TargetWorker handles and command channels.  
  * Receive ClientUpdate commands from the CollectorService to safely manage the lifecycle of the active DatabaseClient handle.  
  * Upon receiving a new SupervisorConfig, perform a reconciliation loop:  
    * **Spawn Workers:** For new targets, spawn a new TargetWorker, but **only if** a valid DatabaseClient is currently available. If no client is available, the creation of the worker is deferred.  
    * **Remove Workers:** Send a shutdown command to workers for targets that are no longer in the configuration.  
    * **Update Workers:** Send commands to existing workers to update their role or other operational parameters.  
  * Periodically query all active TargetWorkers for their detailed health status, aggregate the results into a single HealthReport, and send it to the CollectorService.  
* **State / Ownership:**  
  * workers: HashMap\<IpAddr, TargetWorkerHandle\>  
  * active\_client: Option\<DatabaseClient\> (Managed via commands from CollectorService)  
* **Rationale:** This component acts as the "manager" of the actual work. It decouples the CollectorService's high-level decisions from the implementation details of managing individual pinging tasks, providing a clean abstraction layer.

#### **3.5. The TargetWorker (Per-Target Unit of Work)**

* **Purpose:** To be the self-contained, independent unit of work for a single monitored target. It encapsulates all state and logic necessary for this task.  
* **Key Responsibilities:**  
  * Create and own its dedicated BatchSubmitter instance, which contains the critical, long-lived B-Tree data buffer.  
  * Create and own its Pinger task, which performs the ICMP pings and generates absolute, monotonic timestamps.  
  * Establish an mpsc channel to forward PingResults from the Pinger to the BatchSubmitter.  
  * Listen for WorkerCommands from the TaskSupervisor to control its state (e.g., pause/resume pinging).  
  * Provide a mechanism for the TaskSupervisor to query its health status (e.g., the current size of its BatchSubmitter buffer).  
  * Maintain an internal health state (e.g., enum WorkerHealth { Ok, Fatal(String) }). This state must reflect unrecoverable errors, such as a failure to create an ICMP socket, and be available for the TaskSupervisor to query.  
* **State / Ownership:**  
  * batch\_submitter: BatchSubmitter  
  * pinger: Pinger  
* **Rationale:** Encapsulating all logic for a single target into one component is a key design choice for simplicity and robustness. It enables true concurrency of data submission and isolates failures, ensuring that a problem with one target cannot affect the monitoring of others.

#### **3.6. The DatabaseClient (gRPC Abstraction)**

* **Purpose:** To provide a clean, safe, and ergonomic interface to the database's gRPC API, hiding the complexities of the underlying tonic framework.  
* **Key Responsibilities:**  
  1. Provide clean, async methods for each RPC call (e.g., heartbeat(), send\_batch()).  
  2. Encapsulate the logic for inserting the authentication token and any other required metadata into outgoing request headers.  
  3. Be cheaply Clone-able, allowing it to be passed safely to any task that needs to communicate with the database.  
* **State / Ownership:** It is stateless, containing only a clone of the underlying tonic::transport::Channel.  
* **Rationale:** This abstraction follows the DRY principle. By centralizing request-building logic, it ensures consistency, reduces boilerplate, and makes future protocol changes much easier to manage.

---

# Chapter 4: Information Flow and Communication

This chapter details the "wiring" between the components defined in Chapter 3\. It provides a formal definition of the explicit communication channels and traces the flow of data and commands through the system. Understanding these pathways is critical to understanding the collector's dynamic behavior and its resilience to failure.

#### **4.1. Inter-Component Channels**

The architecture relies on a set of strongly-typed and **bounded** tokio channels to ensure safe, asynchronous communication and to prevent uncontrolled memory growth due to backpressure.

* **ClientUpdate Channel (mpsc)**  
  * **From:** CollectorService  
  * **To:** TaskSupervisor  
  * **Message:** enum ClientUpdate { NewClient(DatabaseClient), ClientLost }  
  * **Purpose:** To provide a robust mechanism for the root supervisor to manage the lifecycle of the active DatabaseClient within the TaskSupervisor. This ensures workers are never spawned with a stale or invalid client handle.  
* **ConfigUpdate Channel (watch)**  
  * **From:** SessionHandler  
  * **To:** TaskSupervisor  
  * **Message:** SupervisorConfig { targets: Vec\<String\>, ping\_rate\_pps: u64, role: CollectorRole }  
  * **Purpose:** The broadcast channel for propagating the latest operational configuration from the database to the worker pool. The watch channel is ideal as it ensures the TaskSupervisor always acts on the most recent state.  
* **ConfigPersistence Channel (mpsc)**  
  * **From:** SessionHandler  
  * **To:** CollectorService  
  * **Message:** CachedIntent { targets: Vec\<String\>, ping\_rate\_pps: u64 }  
  * **Purpose:** To delegate the responsibility of writing the last\_intent.ron cache to the long-lived root service, decoupling the volatile SessionHandler from durable I/O operations.  
* **HealthReport Channel (mpsc)**  
  * **From:** TaskSupervisor  
  * **To:** CollectorService  
  * **Message:** struct HealthReport { total\_buffer\_size: u64, role: CollectorRole, fatal\_errors: Vec\<String\> }  
  * **Purpose:** The "upstream" flow for detailed health metrics. It ensures that fatal, non-transient errors from workers are propagated up to the SessionHandler for inclusion in Heartbeat RPCs.

#### **4.2. Downstream Flow: Configuration & Commands**

This sequence traces how a command from the database is received, processed, and ultimately acted upon by a TargetWorker.

1. **Receipt:** The SessionHandler's SubscribeToCommands loop receives a Command (e.g., PromoteToPrimary) from the database's fast-path stream. Alternatively, its Heartbeat loop receives a new configuration or role.  
2. **Translation:** The SessionHandler translates the gRPC message into a new SupervisorConfig state object.  
3. **Broadcast & Persistence:** The SessionHandler performs two actions in parallel:  
   * It broadcasts the new SupervisorConfig on the ConfigUpdate (watch) channel.  
   * It sends the configuration to the CollectorService via the ConfigPersistence channel to be saved to last\_intent.ron.  
4. **Reconciliation:** The TaskSupervisor, which is subscribed to the ConfigUpdate channel, wakes up and receives the new state. It compares the new configuration to its current list of active workers.  
5. **Dispatch:** The TaskSupervisor sends a specific WorkerCommand to the dedicated mpsc channel of the relevant TargetWorker. These commands are designed to carry payloads to trigger specific actions, for example: UpdateRole(PRIMARY\_SUPERVISED) followed by TriggerGetRecentData.  
6. **Execution:** The TargetWorker's command loop receives the WorkerCommand and executes the state change, for example, by signaling its Pinger task to resume pinging.

#### **4.3. Upstream Flow: Ping Data & Health Metrics**

This section traces two parallel data flows that move from the workers "up" towards the database.

**Path A: The Resilient Data Pipeline**

1. **Generation:** The Pinger task within a TargetWorker executes a ping, generating a PingResult containing an absolute, monotonic timestamp.  
2. **Forwarding:** The Pinger sends the PingResult to its parent TargetWorker's BatchSubmitter via a dedicated mpsc channel.  
3. **Buffering:** The BatchSubmitter receives the PingResult, converts it to a RawDataRecord, and inserts it into its B-Tree buffer, keyed by received\_nanos.  
4. **Submission:** Periodically, the BatchSubmitter creates a batch of records from its buffer that have not yet been acknowledged by the database.  
5. **RPC Call:** The BatchSubmitter uses its cloned DatabaseClient to call the SendBatch RPC, sending the data directly to the zzping-database. It then processes the OK/DESYNC response to update its internal cursor.

**Path B: The Health Metrics Pipeline**

1. **Query:** The TaskSupervisor's periodic health aggregation loop sends a request for status (e.g., GetHealth) to each of its active TargetWorkers.  
2. **Response:** Each TargetWorker responds with its current state, primarily the size of its BatchSubmitter's buffer.  
3. **Aggregation:** The TaskSupervisor collects all responses and aggregates them into a single HealthReport struct.  
4. **Reporting:** The TaskSupervisor sends the complete HealthReport to the CollectorService via the HealthReport channel.  
5. **Consumption:** The CollectorService makes this latest HealthReport available to the active SessionHandler.  
6. **Transmission:** The SessionHandler's Heartbeat loop reads the latest HealthReport and includes its contents in the next outgoing HeartbeatRequest to the database.

---

# Chapter 5: Key Architectural Scenarios

This chapter walks through critical use cases to demonstrate how the components and communication channels work together to achieve the system's resilience goals. These narrative walkthroughs illustrate the dynamic behavior of the architecture under both normal and failure conditions.

#### **5.1. Scenario: Collector Startup and First Connection**

This scenario traces the initial bootstrap sequence of a collector starting for the first time on a host.

1. **Bootstrap (main.rs):**  
   * The zzping-collector process is launched.  
   * main.rs loads collector.ron to get its UUID and the database address.  
   * It attempts to load last\_intent.ron, but the file does not exist.  
   * It attempts to acquire the local TCP port lock (127.0.0.1:7879). It succeeds, as no other PRIMARY instance is running.  
   * It instantiates and runs the long-lived CollectorService.  
2. **Service Initialization (CollectorService):**  
   * The CollectorService is created. It immediately instantiates its own permanent child, the TaskSupervisor. The TaskSupervisor starts with an empty list of workers.  
   * The CollectorService then spawns its other permanent child task, the ConnectionManager.  
3. **Connection (ConnectionManager):**  
   * The ConnectionManager begins its connect/retry loop. It successfully connects to the zzping-database and creates a DatabaseClient.  
   * It sends the new DatabaseClient handle to the CollectorService via the ClientUpdate channel.  
4. **Session Start (CollectorService & SessionHandler):**  
   * The CollectorService receives the DatabaseClient.  
   * It immediately spawns a new, ephemeral SessionHandler task, giving it the client and the necessary channels to communicate with the TaskSupervisor and CollectorService.  
5. **First Commands (SessionHandler & TaskSupervisor):**  
   * The SessionHandler starts its Heartbeat and SubscribeToCommands loops.  
   * The database scheduler sees this new collector and, since no other PRIMARY exists, promotes it. It sends a PromoteToPrimary command.  
   * The SessionHandler receives the command, translates it into a SupervisorConfig (with role: PRIMARY, and a list of targets/rate), and broadcasts it.  
   * The TaskSupervisor receives the new config. Seeing new targets, it spawns a TargetWorker for each one, providing them with a clone of the DatabaseClient it received from the CollectorService.  
   * The newly created TargetWorkers start their Pinger and BatchSubmitter tasks. The collector is now fully operational.

#### **5.2. Scenario: Database Connection Loss and Recovery**

This is the most critical scenario, demonstrating the core resilience of the architecture. It assumes the collector is in a fully operational, PRIMARY state.

1. **Failure Event:** The network connection to the database is lost, or the database process crashes.  
2. **Session Death (SessionHandler):**  
   * The SessionHandler's gRPC loops (Heartbeat or SubscribeToCommands) encounter a terminal network error.  
   * As designed, the SessionHandler task exits immediately and completely.  
3. **State Preservation (CollectorService & TaskSupervisor):**  
   * The CollectorService observes that the SessionHandler task has terminated.  
   * Crucially, the CollectorService **does nothing else**. Its primary stateful child, the TaskSupervisor, is completely unaffected. The TaskSupervisor and all its TargetWorkers continue to run.  
   * The Pinger tasks continue to generate data.  
   * The BatchSubmitter tasks continue to receive and buffer this data. Their SendBatch calls now fail, but they simply log the error and will retry on their next tick, preserving all data in their B-Tree buffers.  
4. **Recovery (ConnectionManager & CollectorService):**  
   * The CollectorService, which holds the JoinHandle for the SessionHandler task, awaits the handle. Its completion is the definitive signal that the session has ended. The CollectorService then signals its ConnectionManager to begin reconnection attempts. Meanwhile, it also commands its TaskSupervisor to clear its active client handle.  
   * After some time, the database is restored or the network is fixed. The ConnectionManager successfully establishes a new connection and creates a new DatabaseClient.  
   * It sends this new client to the CollectorService.  
5. **New Session (CollectorService & SessionHandler):**  
   * The CollectorService receives the new client and spawns a *new* SessionHandler task.  
   * The new SessionHandler establishes its gRPC loops. The database recognizes the collector\_uuid and again promotes it to PRIMARY.  
   * The configuration is broadcast to the TaskSupervisor, which sees that the desired state matches its current running state, so no workers are created or destroyed.  
6. **Data Backfill (BatchSubmitter):**  
   * The BatchSubmitter tasks, which have been patiently buffering data, now succeed in their SendBatch RPC calls. They begin streaming the backlog of data that was collected during the outage, automatically filling the data gap. The system has seamlessly recovered with zero data loss.

#### **5.3. Scenario: The Full Zero-Downtime Handoff**

This scenario demonstrates how the components collaborate to execute the complex, supervised handoff protocol.

1. **Initial State:** A collector instance (C1) is running as PRIMARY. Its TaskSupervisor has active TargetWorkers. A SessionHandler is active.  
2. **New Instance Starts (main.rs):** A new collector instance (C2) is launched for an upgrade.  
   * main.rs for C2 attempts to acquire the local TCP port lock and fails, because C1 holds it. It logs this and proceeds.  
   * C2 starts its CollectorService and connects to the database, identifying with the same collector\_uuid. It starts in the STANDBY role.  
3. **Handoff Initiation (SessionHandler):**  
   * The database scheduler now sees two instances (C1=PRIMARY, C2=STANDBY). It decides to initiate a handoff.  
   * It pushes a Become SUPERVISING command to C1's command stream and a Become PRIMARY\_SUPERVISED command to C2's stream.  
4. **State Transition (TaskSupervisor & TargetWorker):**  
   * Both C1's and C2's SessionHandlers receive their respective commands and broadcast the new roles to their TaskSupervisors.  
   * C1's TaskSupervisor commands its TargetWorkers to pause pinging.  
   * C2's TaskSupervisor commands its TargetWorkers to *begin* pinging and to execute the GetRecentData call.  
5. **Drain, Buffer, and Verification:**  
   * C1's BatchSubmitters continue to drain their buffers. C1's SessionHandler reports the shrinking buffer size in its heartbeats.  
   * C2's BatchSubmitters begin accumulating new data.  
   * The database sees C1's buffer size reach zero and promotes C2 to full PRIMARY, starting its verification timer.  
6. **Success and Shutdown:**  
   * C2's BatchSubmitters now start their SendBatch loops. The database receives a valid batch from C2 within the verification window. The trial is a success.  
   * The database pushes a SHUTDOWN command to C1's command stream.  
   * C1's SessionHandler receives the command, signals its CollectorService to terminate, and the C1 process exits gracefully. The handoff is complete.

#### **5.4. Scenario: Graceful Process Shutdown**

This scenario details the collector's behavior when it receives an external termination signal (e.g., SIGTERM from a service manager or SIGINT from Ctrl+C). The primary goal is to prevent data loss by attempting to flush all in-memory data buffers to the database before the process exits.

* **Requirement:** Upon receiving a termination signal, the collector **MUST** initiate a coordinated, graceful shutdown sequence. It **MUST NOT** exit immediately.  
* **Shutdown Sequence:**  
  * **Signal Handling (main.rs):**  
    * The main.rs bootstrap logic **MUST** include a signal handler (e.g., using tokio::signal) that listens for SIGINT and SIGTERM.  
    * When a signal is received, instead of allowing the process to terminate, the handler **MUST** call a dedicated shutdown() method on the root CollectorService.  
  * **Service Shutdown (CollectorService):**  
    * The CollectorService enters a "shutting down" state. It immediately stops accepting new sessions from the ConnectionManager (e.g., by dropping its end of the ClientUpdate channel).  
    * It then forwards a ShutdownGracefully command to its TaskSupervisor.  
  * **Worker Wind-Down (TaskSupervisor):**  
    * The TaskSupervisor receives the ShutdownGracefully command.  
    * It iterates through all active TargetWorkers and sends each a StopPinging command. This immediately ceases the generation of new data across the system.  
  * **Buffer Drain (TargetWorker & BatchSubmitter):**  
    * Each TargetWorker receives the StopPinging command and signals its Pinger task to terminate.  
    * The TargetWorker then waits for its BatchSubmitter task to complete. The BatchSubmitter, no longer receiving new data, continues its SendBatch loop until its buffer is empty, at which point its main loop finishes and the task completes.  
  * **Coordinated Completion and Termination:**  
    * The TaskSupervisor awaits the completion of all its TargetWorker tasks.  
    * Once all workers have successfully drained and exited, the TaskSupervisor's main task completes.  
    * The CollectorService, having awaited the completion of the TaskSupervisor, then exits its run() method.  
    * The main function returns, and the process terminates cleanly with zero data loss.  
* **Handling Timeouts and Failures during Shutdown:**  
  A graceful shutdown is contingent on the database being available to receive the final data batches. If the database is unreachable, the BatchSubmitters will never be able to drain their buffers, and the collector process would hang indefinitely, ignoring the user's request to terminate. This is unacceptable.  
  * **Requirement:** The graceful shutdown sequence **MUST** be bounded by a global timeout (e.g., 10 seconds), initiated in the main.rs signal handler.  
  * **Behavior:** If the CollectorService.shutdown() method does not complete within this timeout, the process **MUST** log a warning indicating that the buffer drain was incomplete and then exit forcefully (e.g., std::process::exit(1)).  
  * **Rationale:** This makes an explicit trade-off. It prioritizes honoring the user's or system's termination request over waiting indefinitely for an unavailable dependency. While this may result in data loss if the database is down *during the shutdown*, it prevents the collector process from becoming a "zombie" that cannot be terminated.

---

# Appendix A: Definitions

This appendix provides clear and formal definitions for the key architectural concepts and CollectorRoles used throughout this document.

#### **A.1. Glossary of Terms**

* **Long-Lived State:** Refers to any data or component instance that is created at process startup and is designed to persist for the entire lifetime of the collector process. This state, primarily held by the CollectorService and its TaskSupervisor, is fundamentally shielded from the volatility of network connections. The in-memory data buffers are the most critical example of long-lived state.  
* **Ephemeral Task:** A task, typically responsible for I/O-bound operations like network communication, that is designed to be disposable. It exists only for the duration of a single, healthy connection or session. Its failure is an expected event that signals a change in external conditions (e.g., a lost connection) and does not affect the collector's long-lived state. The SessionHandler is the primary example of an ephemeral task.  
* **Reconciliation Loop:** The core operational logic of the TaskSupervisor. It is a control loop that continuously compares the *desired state* (received from the latest SupervisorConfig) with the *actual state* (the set of currently running TargetWorkers). It then takes the necessary actions—creating, destroying, or sending commands to workers—to make the actual state match the desired state.  
* **Supervised Trial:** The core principle of the zero-downtime handoff protocol. It is a process where a change (promoting a new collector instance) is not considered complete until the new component has been actively monitored and has *proven* its operational correctness to an external supervisor (the database). This process includes a mandatory, automatic rollback mechanism to revert to the last known good state upon failure.  
* **Human-Centric Resilience:** A design philosophy that prioritizes building systems that are resilient not just to machine failures, but also to human error during operation and maintenance. It favors automated safety mechanisms (like automatic rollback) over procedures that require high-pressure, manual intervention during a failure, even if it involves a small, calculated trade-off in performance or data completeness.

#### **A.2. CollectorRole Definitions**

These are the distinct operational roles a collector instance can be in, as commanded by the zzping-database scheduler.

* **PRIMARY:** The standard, fully active state. A collector in this role is actively performing all of its functions:  
  * It holds the local TCP port lock for mutual exclusion.  
  * It is executing pings against all configured targets.  
  * It is buffering the results.  
  * It is actively sending data to the database via SendBatch.  
* **STANDBY:** The initial, passive state for any new collector instance. A collector in this role is connected and awaiting instructions.  
  * It **is not** pinging.  
  * It **is not** sending data.  
  * Its sole responsibility is to maintain its gRPC connection and be ready for promotion.  
* **PRIMARY\_SUPERVISED:** A transitional state for the *new* collector instance (C2) at the start of a handoff trial.  
  * It **is actively pinging** and buffering the results locally.  
  * It **MUST NOT** send this new data to the database via SendBatch.  
  * Its primary function is to begin data collection at the precise swap moment, ensuring data continuity while the old collector drains its buffer.  
* **SUPERVISING:** A transitional state for the *old* collector instance (C1) during a hand-o trial. It serves as a live fallback.  
  * It **MUST NOT** perform any new pings.  
  * Its sole responsibility is to drain its existing in-memory buffer by continuing its SendBatch loop until the buffer is empty.  
* **SHUTDOWN:** The terminal state. The collector has been commanded by the database to exit gracefully. This typically occurs after a successful handoff (for the old instance) or as part of a rollback (for a faulty new instance).

