
### **The `zznet-demo` Integration Test: An Exhaustive Validation Plan**

**Objective:** Create a new application crate, `zznet-demo`, containing a suite of integration tests that collectively exercise every feature of the `zznet-*` framework, with the goal of achieving 100% code coverage on the framework crates (`zznet-api`, `zznet-hello`, `zznet-peer-manager`, `zznet-router`, `zznet-room`, `zznet-builder`).

---

### **Phase 0: Foundation and Scaffolding**

**Goal:** Create the new `zznet-demo` crate and define the minimal example components and test harness required for all subsequent phases.

**Tasks:**

1.  **Create New Crate:**
    *   Create the directory `src/apps/zznet-demo`.
    *   Initialize it with a `Cargo.toml` that has dependencies on all `zznet-*` crates, `actix`, and testing utilities. It will contain no `src/main.rs` (test-only crate).
    *   Add the new crate to the workspace `members` in the root `Cargo.toml`.

2.  **Define Message Types:**
    *   Create `ComponentAMessage` enum with `Ping(u64)` and `Pong(u64)` variants.
    *   Implement `RoomMessageTrait` for `ComponentAMessage` for network serialization via `RoomActor<T>`.
    *   Create `StateUpdate(u64)` struct as a local-only Actix message (not serialized).
    *   Create `Subscribe(Recipient<StateUpdate>)` message that `ComponentA` handles to register local subscribers.

3.  **Define Example Components:**
    *   Inside `zznet-demo/src/`, create two component modules: `component_a` and `component_b`.
    *   **`ComponentA`:**
        *   `MainActor` manages a simple `u64` counter.
        *   Handles network messages `Ping(u64)` and `Pong(u64)` (received from `NetworkActor`).
        *   Handles local `Subscribe` message to register `Recipient<StateUpdate>` subscribers.
        *   Broadcasts `StateUpdate` to all subscribers when counter changes.
        *   Implements the full "Three-Actor Pattern" (`MainActor`, `NetworkManager`, `NetworkActor`).
        *   `NetworkManager` implements `RoomManager` trait for the `"room-a"` room.
    *   **`ComponentB`:**
        *   `MainActor` stores a `u64` counter.
        *   Receives `ComponentA` address on construction for subscription.
        *   Implements `Handler<StateUpdate>` to update its counter when notified by `ComponentA`.
        *   Purely local component (no network-facing actors).

4.  **Create the Test Harness:**
    *   In `zznet-demo/tests/full_stack_integration_test.rs`, create a reusable test harness.
    *   Define `AppStack` struct containing:
        *   `PeerManagerActor` (tracks peer lifecycle)
        *   `RouterActor` (manages room routing)
        *   `ConnectionManager` (handles HELLO handshake)
        *   Component instances (`ComponentA`, optionally `ComponentB`)
        *   `allowed_roles: HashSet<Role>` for authorization
    *   Implement `AppStack::new()` that wires actors together following the pattern from `zzping-database/network.rs`:
        *   Create `RouterActor` with offered rooms
        *   Create `PeerManagerActor`
        *   Create `ConnectionManager::new(router, allowed_roles)` and start it
        *   Instantiate components with references to router/peer-manager
        *   Components register their `RoomManager` with router via `RegisterManager` message
    *   Implement `connect_to(&mut self, other: &mut AppStack)` helper:
        *   Uses `create_mock_pair()` to create bidirectional mock connections
        *   Sends `HandleTransport` message to each stack's `ConnectionManager`
        *   `ConnectionManager` spawns `HelloActor` to complete handshake
    *   **Note:** This duplicates wiring logic from applications (DRY opportunity for future `zznet-builder` helper).

**Success Criteria for Phase 0:**
*   The `zznet-demo` crate exists and compiles.
*   Message types (`ComponentAMessage`, `StateUpdate`, `Subscribe`) are defined and implement required traits.
*   The example components (`ComponentA`, `ComponentB`) are defined with proper three-actor pattern.
*   The test harness can successfully instantiate two separate `AppStack`s.
*   `AppStack::new()` correctly wires `PeerManagerActor`, `RouterActor`, and `ConnectionManager` using pure actor model.
*   Components register their `RoomManager` implementations with the router on startup.

---

### **Phase 1: Connection, Handshake, and Role Validation**

**Goal:** Verify that two stacks can establish a connection, complete the HELLO handshake, and perform basic role-based authorization.

**Test Cases:**

1.  **`test_successful_connection_and_handshake()`:**
    *   Instantiate two `AppStack`s (Stack 1 and Stack 2).
    *   Configure Stack 1 (`server`) with `allowed_roles` containing Stack 2's role.
    *   Call `stack1.connect_to(&mut stack2)`.
    *   **Assert:** The `PeerManagerActor` on both stacks reports that the other peer is present and in the `Connected` state.
    *   **Covers:** Full stack wiring, `ConnectionManager`, `HelloActor` happy path, `PeerManagerActor` state updates, `MockConnection`.

2.  **`test_handshake_failure_due_to_authorization()`:**
    *   Instantiate two `AppStack`s.
    *   Configure Stack 1 with `allowed_roles` that **excludes** Stack 2's role.
    *   Connect the stacks via `connect_to()`.
    *   **Assert:** The `PeerManagerActor` on both stacks shows **no connected peers**.
    *   **Assert:** Logs contain "SECURITY REJECTION" or similar authorization failure messages from `ConnectionManager`.
    *   **Covers:** `ConnectionManager`'s authorizer logic and error paths in `HelloActor`.
    *   **Note:** Authorization is simple: if the peer's declared role is not in `allowed_roles` HashSet, handshake fails.

---

### **Phase 2: Room Negotiation and Bidirectional `A <-> A` Communication**

**Goal:** Verify that rooms are correctly negotiated and that components can send and receive typed messages.

**Test Cases:**

1.  **`test_room_negotiation_and_bidirectional_messaging()`:**
    *   Instantiate two stacks, each containing an instance of `ComponentA`.
    *   Configure both `RouterActor`s to offer `"room-a"`.
    *   Connect the stacks.
    *   **Optional Assert:** If `Router` introspection is simple, confirm `"room-a"` is joined. Otherwise skip (keep test simple).
    *   `ComponentA` on Stack 1 sends a `Ping(42)` message via its `NetworkManager`.
    *   **Assert:** The `MainActor` of `ComponentA` on Stack 2 receives the `Ping(42)` message (via `NetworkActor` → `MainActor`) and updates its internal counter to 42.
    *   The `MainActor` of `ComponentA` on Stack 2 then sends a `Pong(43)` message back.
    *   **Assert:** The `MainActor` of `ComponentA` on Stack 1 receives the `Pong(43)` message and updates its counter to 43.
    *   **Covers:** `RoomManager` registration, `RouterActor` room creation, `PeerChannels` routing, `RoomActor<T>` (de-)serialization, and the full inbound/outbound path of the Three-Actor Pattern.

2.  **`test_room_negotiation_failure_empty_intersection()`:**
    *   Instantiate two stacks. Configure Stack 1's `RouterActor` to offer `"room-a"` and Stack 2's to offer `"room-z"`.
    *   Connect the stacks.
    *   **Assert:** The connection is torn down. The `PeerManagerActor`s should show no connected peers after the handshake attempt. Logs should indicate `SessionError::EmptyIntersection`.
    *   **Covers:** Error handling in `Router`'s `handle_publish_rooms`.

---

### **Phase 3: Local Forwarding (`A -> B`) and Intra-Process Pub/Sub**

**Goal:** Verify the "Local Forwarding" pattern for cross-component communication.

**Test Case:**

1.  **`test_local_forwarding_from_a_to_b()`:**
    *   Instantiate one stack (`Stack 2`) containing both `ComponentA` and `ComponentB`.
    *   Instantiate another stack (`Stack 1`) containing only `ComponentA`.
    *   Within Stack 2, send `Subscribe` message to `ComponentA` with `ComponentB`'s `Recipient<StateUpdate>`.
    *   Connect Stack 1 to Stack 2.
    *   `ComponentA` on Stack 1 sends a `Ping(99)` message via network to `"room-a"`.
    *   **Assert:** The `MainActor` of `ComponentA` on Stack 2 receives the `Ping(99)` (network message) and updates its counter to 99.
    *   **Assert:** `ComponentA` on Stack 2 broadcasts `StateUpdate(99)` (local message) to all subscribers.
    *   **Assert:** The `MainActor` of `ComponentB` on Stack 2 receives the `StateUpdate(99)` via its local subscription and updates its counter to 99.
    *   **Covers:** The complete end-to-end data flow, proving the separation between inter-process (network via `RoomActor<T>`) and intra-process (local Actix messages) communication.

---

### **Phase 4: Disconnection and Lifecycle Management**

**Goal:** Verify that the framework gracefully handles peer disconnections and cleans up all related actors.

**Test Case:**

1.  **`test_disconnection_and_cleanup()`:**
    *   Instantiate and connect two stacks.
    *   Explicitly `drop()` the `MockConnection` on one side of the pair to simulate a network failure.
    *   **Assert:** After a short delay, the `PeerManagerActor` on the remaining stack reports that the peer has disconnected.
    *   **Covers:** `PeerLifecycleEvent::PeerDisconnected`, `NetworkManager`'s event handling, and the RAII cleanup of per-peer actors.
    *   **Note:** We skip health introspection queries to keep the test simple. The peer disconnection event itself proves cleanup occurred.

---

### **Phase 5: Coverage Analysis and Finalization**

**Goal:** Achieve 95-98% test coverage of the `zznet-*` crates by adding targeted tests for any remaining code paths.

**Tasks:**

1.  **Install Coverage Tooling:**
    *   Ensure the development environment has `cargo-llvm-cov` installed (`cargo install cargo-llvm-cov`).

2.  **Run Coverage Report:**
    *   Execute the command: `cargo llvm-cov --test full_stack_integration_test --workspace --html`
    *   This will run *only* the new integration test suite and generate an HTML report showing exactly which lines in the `zznet-*` crates were (and were not) executed.

3.  **Analyze and Remediate:**
    *   Open the HTML report and identify any functions or code branches marked in red (uncovered).
    *   For each uncovered area, evaluate:
        *   **If reachable:** Write a new integration test case in `zznet-demo` that exercises this path.
        *   **If unreachable:** Document why (e.g., defensive panic, debug-only code) and accept as exception.
    *   Focus on critical paths:
        *   Basic error handling (invalid room negotiation, serialization errors)
        *   Core lifecycle events (connection, disconnection, room creation)
        *   Component integration paths (all three actors communicating)
    *   Skip complex error injection scenarios (channel backpressure, mid-send failures, handler panics) - these are better suited for unit tests.
    *   Repeat the process of running the coverage report and adding tests until coverage for all `zznet-*` framework crates reaches 95-98%.

4.  **Document Uncovered Code:**
    *   Create a `COVERAGE.md` in `zznet-demo/docs/` listing any remaining uncovered code with justification.
    *   Examples: unreachable panic branches, impossible error states, debug logging.

**Success Criteria for Phase 5:**
*   The final code coverage report shows 95-98% for all `zznet-*` framework crates.
*   All major code paths (connection, handshake, room negotiation, messaging, disconnection) are exercised.
*   Any uncovered code is documented with clear justification.
*   The `zznet-demo` crate serves as a comprehensive, living documentation of how to use the framework correctly.

---

### **Implementation Notes**

**DRY Opportunity (Future Improvement):**
The `AppStack` test harness duplicates connection wiring logic from `zzping-database/network.rs` and `zzping-collector/network.rs`. Consider extracting this pattern into a `zznet-builder` helper in a future refactor:
```rust
// Future: zznet-builder::helpers
pub fn create_network_stack(
    router: Addr<RouterActor>,
    allowed_roles: HashSet<Role>,
    transport: ConnectionType,  // Mock or TCP
) -> NetworkStack { ... }
```

For now, we accept duplication to maintain momentum and prove the architecture works end-to-end.

---

This plan provides a clear, verifiable path to creating an integration test that proves the architecture works while serving as a powerful tool for ensuring ongoing quality and correctness.