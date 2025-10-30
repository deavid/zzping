### **Finalization Plan: Eradicating Architectural Remnants**

**Objective:** To achieve a single, coherent, and vision-aligned architecture by systematically removing all deprecated patterns, redundant implementations, and obsolete code from the `zznet` framework. The end state will be a codebase that is smaller, simpler, and unambiguously follows the agreed-upon design.

**Guiding Principle:** If a piece of code supports the "DIY networking" anti-pattern or the old `Arc<dyn ...>` trait-object model, it is considered a remnant and must be removed.

---

#### **Phase 1: Purge the Conflicting Room Implementation**

**Goal:** Eliminate the primary source of architectural ambiguity by removing the old, flawed `Room<T>` utility and its associated helpers from `zznet-room`.

*   **Step 1.1: Removal of the Old `Room<T>` Utility.**
    *   **Action:** Delete the file `src/net/zznet-room/src/room.rs`.
    *   **Rationale:** This file contains the `Room<T>` struct, `TypedSender<T>`, and `RoomChannels` struct, which together represent the core of the "DIY networking" anti-pattern. Its existence directly contradicts the new `RoomActor<T>` "magic box" abstraction. Removing it is the most critical step.

*   **Step 1.2: Removal of Associated Test Utilities.**
    *   **Action:** Delete the file `src/net/zznet-room/src/connector.rs`.
    *   **Rationale:** This file provides the `connect_rooms` utility, which was designed exclusively for testing the old `Room<T>` pattern. With `room.rs` gone, this utility is obsolete and serves no purpose.

*   **Step 1.3: Verification.**
    *   **Action:** The codebase must be brought back to a compiling state. Any compilation errors that arise from the deletion of these files must be resolved by migrating the affected code to use the new `RoomActor<T>` and `RoomManager` patterns.
    *   **Success Criterion:** A project-wide search for `zznet_room::room::` yields zero results in the `src` directory.

---

#### **Phase 2: Remove Obsolete Trait-Object APIs**

**Goal:** Complete the transition to a pure actor-messaging model by removing the `Arc<dyn ...>`-based traits that were superseded by direct actor communication.

*   **Step 2.1: Removal of the `MessageRouter` Trait.**
    *   **Action:** In `src/net/zznet-api/src/traits.rs`, delete the `MessageRouter` trait definition.
    *   **Rationale:** The responsibility for routing messages is now handled by sending messages directly to the `RouterActor`. The `MessageRouter` trait object is an unnecessary and outdated layer of indirection.

*   **Step 2.2: Removal of the `PeerRegistry` Trait.**
    *   **Action:** In `src/net/zznet-api/src/traits.rs`, delete the `PeerRegistry` trait definition.
    *   **Rationale:** The responsibility for querying peer state is now handled by sending messages directly to the `PeerManagerActor`. The `PeerRegistry` trait object is an obsolete pattern.

*   **Step 2.3: Verification.**
    *   **Action:** Resolve any compilation errors. Components that were depending on these traits must be updated to use `Addr<RouterActor>` and `Addr<PeerManagerActor>` respectively.
    *   **Success Criterion:** A project-wide search for `Arc<dyn MessageRouter>` and `Arc<dyn PeerRegistry>` yields zero results in the `src` directory.

---

#### **Phase 3: Finalize and Simplify Crate Structure**

**Goal:** Ensure the crate structure is clean and that no crates exist solely to support deprecated patterns.

*   **Step 3.1: Audit `zzping-test-utils`.**
    *   **Action:** Review the contents of `src/test-utils/zzping-test-utils/src/lib.rs`. Identify and remove any helper structs or functions that were designed to support the old trait-object or `Room<T>` patterns (e.g., `DummyRoomHandle`).
    *   **Rationale:** Test utilities must evolve with the architecture. Keeping helpers for obsolete patterns encourages their continued use and makes testing the new architecture more difficult.

*   **Step 3.2: Update Crate-Level Documentation (`README.md`).**
    *   **Action:** Review the `README.md` files within each `zznet-*` crate.
    *   **Rationale:** Ensure that all documentation accurately reflects the final, actor-based architecture and does not contain examples or descriptions of the now-removed patterns. This prevents future confusion for developers.

*   **Step 3.3: Final Dependency Audit.**
    *   **Action:** Review the `Cargo.toml` files for all `zznet-*` and component crates.
    *   **Rationale:** Ensure there are no unnecessary dependencies. For example, a component's `MainActor` crate should not have a dependency on `zznet-router`. Dependencies should be as minimal as possible, primarily on `zznet-api` for shared types. This enforces the architectural boundaries at the compiler level.

---

### **End State and Definition of "Done"**

This finalization effort is considered "done" when the following conditions are met:

1.  **Code Purity:** The codebase successfully compiles and passes all tests.
2.  **No Remnants:** A project-wide search for `RoomHandle`, `RoomAdapter`, `MessageRouter`, `PeerRegistry`, and the old `zznet_room::room::Room` yields zero results in the `src` directory.
3.  **Architectural Unambiguity:** There is only one, clearly defined way to implement network communication for a component: the `RoomManager` factory pattern that creates `RoomActor<T>` instances.
4.  **Documentation Consistency:** All `README.md` files and high-level architectural documents accurately describe the final, actor-first architecture.