Read and Consider:

* `ZZPing_Architectural_Vision_II.md` - this is the overall vision of what we're going for.
* `ZZPing_Network_protocol.md` - this overrides the above and sets what do we want to build right now, but it's an overall design.

## **Roadmap for this task:**

This is, overall, what we want to build on the Pull Request:

**Chapter 1: Foundational Migration to Unary RPCs (My original roadmap)**
*   Step 1: Protocol Expansion (Add `Heartbeat`, `SendBatch` to `.proto`)
*   Step 2: DB `Intent Config` & `Heartbeat` Logic
*   Step 3: Collector `Heartbeat` Logic
*   Step 4: DB `SendBatch` Logic (with state check)
*   Step 5: Collector `SendBatch` Logic (with DESYNC handling)
*   Step 6: Cleanup (Remove `IngestStream`)

**Chapter 2: Implementing Zero-Downtime Upgrades**
*   Step 7: Protocol Expansion (Add `GetRecentData` RPC and fields for handoff in `Heartbeat` messages)
*   Step 8: DB Scheduler Logic (Implement the `PRIMARY`/`STANDBY` state machine)
*   Step 9: Collector Handoff Logic (Implement handling for `SUPERVISING`/`SHUTDOWN` roles)
*   Step 10: DB `GetRecentData` Logic
*   Step 11: Collector Buffer Seeding (Call `GetRecentData` after a swap)

**Chapter 3: Hardening Security**
*   Step 12: Evolve Auth (Move from static token to a user/role model in the interceptor).
*   Step 13: Enforce ACLs (Add role checks inside each RPC method on the server).

## Unit test suggestions to prove the work at each step

**Core Principle for Preventing Hangs:**

Every `await` on a network operation or a channel receive in a test must be wrapped in a strict, short `tokio::time::timeout`. A 100ms or 1s timeout is more than enough for a local, in-process test. A hang is a test failure, not something we wait for.

---

### **Testing Strategy by Roadmap Chapter**

#### **Chapter 1: Foundational Migration to Unary RPCs**

*   **Step 1: Protocol Expansion.**
    *   **Proof:** `cargo check` and `cargo clippy` pass. No new logic, so no new tests needed.

*   **Step 2: DB `Intent Config` & `Heartbeat` Logic.**
    *   **Proof:** Pure unit tests in `zzping-database`.
        *   Test A: `test_read_intent_config()` - Verify that a sample `intent.ron` file is parsed into the correct structs. This is a simple file I/O and deserialization test, no network involved.
        *   Test B: `test_heartbeat_rpc()` - This is an in-process integration test.
            1.  Spawn the gRPC server (with the new `Heartbeat` method implemented) on a random port.
            2.  Create a temporary `intent.ron` file on disk for the server to read.
            3.  Create a client, call `client.heartbeat(...).await` (with a timeout).
            4.  Assert that the `HeartbeatResponse` contains the exact configuration from the temp file.

*   **Step 3: Collector `Heartbeat` Logic.**
    *   **Proof:** In-process integration test in `zzping-collector`.
        *   Test A: `test_collector_gets_config_via_heartbeat()`
            1.  Spawn the database test server from Step 2.
            2.  Run a *mocked-up version* of the collector's main loop that *only* performs the `Heartbeat` call.
            3.  Use an MPSC channel to have the mock loop send the config it received back to the main test thread.
            4.  Assert that the received config matches what the test server should be sending.

*   **Step 4: DB `SendBatch` Logic (with state check).**
    *   **Proof:** Unit tests in `zzping-database`. We don't need a full client yet.
        *   Test A: `test_sendbatch_accepts_good_data()` - Call the `send_batch` method directly. Mock the database state to be in sync with the request's `collector_believes...`. Assert it returns `OK` and the correct new `acked_nanos`.
        *   Test B: `test_sendbatch_rejects_desync_data()` - Call the `send_batch` method directly. Mock the database state to be *out of sync*. Assert it returns a `DESYNC` status and the *database's* correct, older `acked_nanos`.

*   **Step 5: Collector `SendBatch` Logic (with DESYNC handling).**
    *   **Proof:** In-process integration test in `zzping-collector`.
        *   Test A: `test_collector_sends_batch_and_prunes_buffer()` - Run the collector against a test server that always returns `OK`. Send pings into the collector, and use an MPSC channel to inspect the collector's internal buffer size. Assert that the buffer is pruned after the `SendBatch` call.
        *   Test B: `test_collector_rewinds_buffer_on_desync()` - Run the collector against a test server that is programmed to return `DESYNC` on the first call. Assert that the collector does *not* prune its buffer and instead attempts to re-send the correct, older data on its next attempt.

*   **Step 6: Cleanup.**
    *   **Proof:** `cargo test` still passes. The tests for the old `IngestStream` will be deleted along with the code, but all the new tests for the Unary RPCs must remain and pass.

#### **Chapter 2: Implementing Zero-Downtime Upgrades**

This is more complex, but the same principles apply.

*   **Step 7: Protocol Expansion.**
    *   **Proof:** `cargo check`.

*   **Step 8: DB Scheduler Logic.**
    *   **Proof:** Unit tests in `zzping-database`.
        *   Create an instance of the scheduler.
        *   Simulate heartbeats from `C1`, then `C2`, then `C1` again.
        *   Check the internal state of the scheduler to assert that it correctly assigns `PRIMARY` and `STANDBY` roles.
        *   Test the logic for triggering and rolling back a trial period. This is all in-memory logic, no network needed.

*   **Step 9 & 11: Collector Handoff & Buffer Seeding Logic.**
    *   **Proof:** A new, complex, in-process integration test in `zzping-collector`.
        *   Test A: `test_full_successful_handoff()`
            1.  Spawn the DB test server (now with the scheduler logic).
            2.  Start a mock `C1` collector task.
            3.  Let it run and send some data.
            4.  Start a mock `C2` collector task.
            5.  The test will need to use MPSC channels to monitor the internal states of both `C1` and `C2`.
            6.  Verify that `C1` is told to become `SUPERVISING` and `C2` is told to become `TRIAL`.
            7.  Verify that `C2` then calls `GetRecentData`.
            8.  Verify that the DB eventually tells `C1` to `SHUTDOWN`.
            9.  The test passes if this entire sequence completes successfully within a timeout.

#### **Chapter 3: Hardening Security**

*   **Step 12: Evolve Auth.**
    *   **Proof:** Pure unit test of the interceptor function. Feed it mock requests with different JWTs (or whatever token format is chosen) and assert that it correctly extracts roles or rejects invalid tokens.

*   **Step 13: Enforce ACLs.**
    *   **Proof:** In-process integration tests for each RPC.
        *   `test_sendbatch_fails_with_reader_role()` - Spawn the server. Create a client. Create a token with only the "reader" role. Call `send_batch` and assert that it fails with `Status::PermissionDenied`.
        *   `test_querydata_fails_with_collector_role()` - Same pattern for the query endpoint.


## **Task Document: Migration to a Resilient Unary RPC Architecture**

Hello Jules,

The initial proof-of-concept for migrating our project to gRPC was successful, but a deeper architectural analysis has revealed that the chosen bi-directional stream model is inherently fragile and does not meet our core requirements for resilience.

We have completed a new architectural design that is simpler, more robust, and better aligned with the project's guiding principles. This new architecture is based exclusively on **transactional, Unary gRPC RPCs**.

Your mission is to execute the full migration from our current stream-based PoC to this new Unary architecture. The work is broken down into a series of chapters and steps. Each step is a self-contained deliverable that must leave the entire workspace in a state where `cargo test --all-targets` and `cargo clippy --all-targets` pass.

**Core Principle for Testing:** All tests involving network operations or asynchronous waiting must use aggressive timeouts (e.g., `tokio::time::timeout`) to prevent hangs and ensure rapid feedback. A test that hangs is a failed test.

---

### **Chapter 1: Foundational Migration to Unary RPCs**

The goal of this chapter is to replace the core data submission mechanism, moving from the fragile `IngestStream` to the robust `Heartbeat` and `SendBatch` Unary RPCs.

**Step 1.1: Protocol Redefinition**

*   **Goal:** Update the `.proto` contract to define the new Unary RPCs.
*   **Tasks:**
    *   In `zzping-proto/proto/ingestion.proto`, **remove** the `IngestStream` RPC.
    *   **Add** the new Unary RPCs and their associated messages. The protocol should now look like this:

        ```proto
        service Ingestion {
          // Collector calls this periodically for liveness and config.
          rpc Heartbeat(HeartbeatRequest) returns (HeartbeatResponse);

          // Collector calls this to send batches of ping data.
          rpc SendBatch(SendBatchRequest) returns (SendBatchResponse);

          // GUI/CLI calls this to get recent data for a handoff.
          rpc GetRecentData(GetRecentDataRequest) returns (GetRecentDataResponse);

          // GUI/CLI calls this for historical data.
          rpc QueryData(QueryRequest) returns (QueryResponse);
        }

        // --- Message Definitions for Heartbeat ---
        message HeartbeatRequest {
          string collector_uuid = 1;
          // You may need to add other transient info here later, like PID.
        }
        message HeartbeatResponse {
          // This will eventually contain role info for handoffs.
          // For now, it contains the global config.
          repeated string targets = 1;
          uint64 ping_rate_pps = 2;
        }

        // --- Message Definitions for SendBatch ---
        message SendBatchRequest {
          string collector_uuid = 1;
          repeated RawDataRecord records = 2;
          uint64 collector_believes_last_acked_nanos = 3;
        }
        message SendBatchResponse {
          enum Status {
            OK = 0;
            DESYNC = 1;
          }
          Status status = 1;
          uint64 database_confirms_last_acked_nanos = 2;
        }

        // ... other messages like GetRecentData, QueryData, RawDataRecord ...
        ```
*   **Verification:** The project must compile after these changes, although many parts will be broken. The immediate goal is just to have the new RPC stubs generated.

**Step 1.2: Implement Database `Heartbeat` Logic**

*   **Goal:** Make the database the source of truth for configuration.
*   **Tasks:**
    *   Implement a mechanism for the database to read a master `intent.ron` configuration file. This file will contain the global `ping_rate` and `targets` list.
    *   Implement the server-side `heartbeat` method in `IngestionServiceImpl`. It should read the `intent.ron` file and return its contents in the `HeartbeatResponse`.
    *   Add a unit test that spawns the server, creates a temporary `intent.ron`, and verifies that a client call to `heartbeat` receives the correct configuration.

**Step 1.3: Refactor Collector to Use `Heartbeat` for Configuration**

*   **Goal:** Make the collector a "dumb worker" that gets its instructions from the database.
*   **Tasks:**
    *   Remove the `targets` and `rate` arguments from the `zzping-collector` `Cli` struct.
    *   Modify the collector's `runner` to implement a main loop that calls the `heartbeat` RPC every second.
    *   The collector must receive the configuration from the `HeartbeatResponse` and dynamically start/stop its internal pinging tasks to match the received configuration.
    *   For now, the ping data generated will be discarded. The focus is solely on the configuration mechanism.
    *   Add an integration test to verify the collector correctly applies the configuration it receives from a test server.

**Step 1.4: Implement Database `SendBatch` Logic**

*   **Goal:** Implement the core data integrity and resilience mechanism.
*   **Tasks:**
    *   Implement the server-side `send_batch` method in `IngestionServiceImpl`.
    *   This method **must** perform the critical state check: compare the `collector_believes_last_acked_nanos` from the request with its own persisted state for that `collector_uuid`.
    *   If they match, accept the data and return an `OK` response with the new last-acked timestamp.
    *   If they do not match, reject the data and return a `DESYNC` response with the database's authoritative last-acked timestamp.
    *   Add unit tests for both the `OK` and `DESYNC` scenarios.

**Step 1.5: Refactor Collector to Use `SendBatch` for Data Submission**

*   **Goal:** Switch the collector to the new transactional data submission model.
*   **Tasks:**
    *   Replace the logic that was discarding data with a new loop that prepares batches and calls the `send_batch` RPC.
    *   The collector must correctly manage its buffer, pruning it when it receives an `OK` response.
    *   The collector must correctly handle a `DESYNC` response by "rewinding" its buffer and re-sending the data the database is missing.
    *   Add integration tests to verify both the successful pruning and the rewind-on-desync behaviors.

**Step 1.6: Final Cleanup**

*   **Goal:** Remove all obsolete code from the old streaming model.
*   **Tasks:**
    *   Delete the old `IngestStream` RPC and its server-side implementation.
    *   Delete the old `Target Manager` logic in the collector.
*   **Verification:** The entire workspace must pass `cargo test` and `cargo clippy`. At this point, the foundational migration is complete.

---

### **Chapter 2: Implementing Zero-Downtime Upgrades**

The goal of this chapter is to build the sophisticated "live swap" mechanism on top of the new Unary RPC foundation.

**Step 2.1: Protocol Expansion for Handoffs**

*   **Goal:** Add the necessary RPCs and message fields to support the handoff protocol.
*   **Tasks:**
    *   Add the `GetRecentData` RPC to the `.proto` file.
    *   Add fields to the `Heartbeat` messages to communicate roles (`PRIMARY`, `STANDBY`, `SUPERVISING`, `SHUTDOWN`) and the pre-scheduled swap timestamp.

**Step 2.2: Implement Database Scheduler for Handoffs**

*   **Goal:** Make the database aware of and able to orchestrate a collector swap.
*   **Tasks:**
    *   Modify the database's `heartbeat` logic to detect when two processes claim the same `collector_uuid`.
    *   Implement the state machine that manages the `PRIMARY`/`STANDBY`/`TRIAL`/`SUPERVISING` lifecycle.
    *   Implement the logic to calculate a future timestamp and send the `PrepareToSwap` commands.
    *   Add unit tests for this new scheduler logic.

**Step 2.3: Implement Collector Handoff State Machine**

*   **Goal:** Enable a collector to participate in a database-orchestrated swap.
*   **Tasks:**
    *   The collector must be able to handle the new role commands in the `HeartbeatResponse`.
    *   It must implement the logic to pause pinging when commanded (`SUPERVISING`) and to exit gracefully (`SHUTDOWN`).
    *   It must implement the logic to act on a pre-scheduled swap command.

**Step 2.4: Implement Buffer Seeding**

*   **Goal:** Solve the buffer transfer problem by having the new collector fetch recent data from the database.
*   **Tasks:**
    *   Implement the server-side `get_recent_data` method. It should serve data from the database's in-memory "hot tier."
    *   The collector, after being promoted to `PRIMARY` in a swap, must call `get_recent_data` to populate its buffer.
*   **Verification:** A comprehensive integration test is required for this chapter. It must simulate the full handoff scenario: `C1` running -> `C2` starts -> DB orchestrates swap -> `C2` takes over -> `C2` seeds its buffer -> `C1` shuts down.

---

### **Chapter 3: Hardening Security with ACLs**

The goal of this final chapter is to implement a proper authorization layer.

**Step 3.1: Implement a User/Role Model**

*   **Goal:** Move beyond a single static token to a flexible authentication model.
*   **Tasks:**
    *   The `check_auth` interceptor in the database should be enhanced. Instead of checking a static string, it should be prepared to handle a more structured token (like a JWT) that contains a `user_id` and a list of `roles`. For testing, you can use simple, non-encrypted tokens.
    *   The interceptor must attach the validated user identity and roles to the request extensions.
*   **Verification:** Unit test the interceptor with various mock tokens to ensure it correctly validates, extracts, and attaches user/role information.

**Step 3.2: Enforce ACLs in RPC Methods**

*   **Goal:** Ensure that only authorized users can perform specific actions.
*   **Tasks:**
    *   Modify every RPC method in `IngestionServiceImpl` (`heartbeat`, `send_batch`, `query_data`, etc.).
    *   The first action in each method must be to retrieve the `UserIdentity` from the request extensions.
    *   Add logic to check if the user's `roles` list contains the required role for that specific action (e.g., `send_batch` requires the "collector" role).
    *   If the check fails, return a `Status::PermissionDenied` error.
*   **Verification:** Add integration tests for each RPC that attempt to call it with an invalid role and assert that the call is correctly rejected with a `PermissionDenied` status.