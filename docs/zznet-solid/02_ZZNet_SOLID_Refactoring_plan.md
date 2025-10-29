## Architectural Decision Record: The ZZNet SOLID Refactoring Plan

**Date:** 2025-10-27 (Revised)
**Status:** Adopted & Authoritative
**Supersedes:** `01_ZZNet_SOLID_Framework.md` and any prior verbal or written plans.
**Context:** This document is the definitive plan for the architectural refactoring of the `zznet` framework and the `zzping` component model. It is the outcome of extensive critical review, including skeptical analysis by multiple AI agents, architectural debate, and explicit clarification of requirements. The principles and phases outlined herein are the mandatory guide for all subsequent development work.

**Revision History:**
- 2025-10-26: Initial plan from Gemini conversation
- 2025-10-27 (Morning): Updated after skeptical review and clarification of Three-Actor Pattern necessity, Phase 0 approach, and test quality requirements
- 2025-10-27 (Afternoon): Added execution safeguards based on Gemini's final review:
  - Section 2.4: Shared types strategy (`zznet-api` for framework events)
  - Section 5.1: Mandatory task checklist format for all work
  - Phase 3: Dual testing strategy for regression safety
  - Section 6.7: ComponentBuilder utilities deferred until experience gained

### Chapter 1: The Mandate for Change

#### 1.1. The Problem: Architectural Debt and Unenforced Patterns
A thorough audit of the codebase has confirmed that while the project has a sound high-level vision, its implementation has suffered. The `zznet-session` crate, in particular, has become a monolithic "God Object" that violates core SOLID principles, namely the Single Responsibility Principle (SRP) and the Interface Segregation Principle (ISP). This has led to a cascade of implementation-level problems: hybrid and confusing API patterns (`Arc<Mutex>` alongside actor messages), boilerplate, and components with unclear boundaries between business logic and network concerns. These symptoms are a direct result of a flawed architectural foundation.

**The Root Cause:** The inability to understand and clean up the existing codebase stems from the tangled architecture. Business logic and network logic are intertwined to such a degree that it's impossible to identify what is "unused code" versus "network plumbing" versus "incomplete features." **Architectural clarity is the prerequisite for code cleanup**, not the other way around. Once the structure is clear, what needs to be deleted, completed, or fixed will become obvious.

#### 1.2. The Non-Negotiable Principles
To pay down this architectural debt and build a robust, maintainable system, all work on the `zznet` framework and its components will adhere strictly to the following principles:

1.  **Strict Separation of Concerns:** Network logic and business logic must be absolutely isolated from each other. State management and message routing are distinct responsibilities and must live in separate components.
2.  **Architectural Purity over Premature Optimization:** The primary goal is a clean, testable, and easy-to-reason-about architecture. Performance optimizations that compromise this clarity are forbidden unless proven necessary by rigorous profiling on a complete, working system.
3.  **Verifiable, Incremental Progress:** The refactoring will proceed in phases, each with a "stopgap point" where the entire codebase compiles and passes all tests. This ensures the project remains in a functional state throughout the migration.
4.  **Enforced Discipline:** Patterns are not suggestions. The architecture will be enforced through the type system, crate dependencies, and a strict "Definition of Done" for all tasks.

### Chapter 2: The New Framework Architecture: Decomposing `zznet-session`

The core of the refactoring is the dismantling of the monolithic `zznet-session` crate into two new, specialized crates that cleanly separate the control plane from the data plane.

#### 2.1. New Crate: `zznet-peer-manager` (The Control Plane)
*   **Single Responsibility:** To be the application's authoritative registry for peer state, identity, and authentication context.
*   **Responsibilities:** Manages peer lifecycle, tracks connection status and roles (`PeerAdded`, `PeerRemoved`), and provides a queryable API for peer information.
*   **Key Feature:** Publishes `PeerLifecycleEvent`s to a decentralized event bus (`tokio::sync::broadcast`) for other services to consume.
*   **Exclusions:** Has **zero knowledge** of message routing, transport channels, or byte-level data.

#### 2.2. New Crate: `zznet-router` (The Data Plane)
*   **Single Responsibility:** To manage the transport channels for each peer and route serialized byte payloads to them.
*   **Responsibilities:** Maintains a map of `PeerId` to their outbound transport channels. Exposes a simple `send_to_peer(peer_id, room_id, bytes)` API.
*   **Exclusions:** Has **zero knowledge** of peer state, roles, or authentication. It is a stateless byte-forwarder.

#### 2.3. Relocation of Responsibilities
*   **`zznet-session`:** Will be deprecated and eventually deleted.
*   **`PeerSession`:** Will be eliminated. Its state logic moves to `zznet-peer-manager`; its channel logic moves to `zznet-router`.
*   **`RoomHandle` / `RoomAdapter`:** Will be relocated to `zznet-room`, as they are part of the component-facing abstraction.
*   **Room Negotiation Logic:** Will be relocated to the `ConnectionManager` in `zznet-hello`, which already orchestrates the connection bootstrap.

#### 2.4. Shared Types and the Role of `zznet-api`

**Design Question:** Where should framework event types (like `PeerLifecycleEvent`) be defined?

**Answer:** In `zznet-api`, alongside existing shared types like `PeerIdentity` and `Role`.

**Rationale:**
- Components' `NetworkManager` actors need to **subscribe to** `PeerLifecycleEvent` but don't need to **produce** them
- If events lived in `zznet-peer-manager`, every component would depend on the full implementation crate (heavy coupling)
- By placing events in `zznet-api`, components depend only on the lightweight type definitions

**What belongs in `zznet-api`:**
- Core data types: `PeerIdentity`, `Role`, `AuthContext`
- Framework event types: `PeerLifecycleEvent` (PeerAdded, PeerRemoved, etc.)
- Abstract traits: `ZzChannel` (if needed)
- Common error types shared across framework crates

**What does NOT belong in `zznet-api`:**
- Implementation logic (belongs in specific crates)
- Concrete types specific to one crate

**Future consideration:** If `zznet-api` grows too large, we may extract `zznet-events` as a separate crate. For now, keep it simple with a single API crate.

### Chapter 3: The New Component Architecture: The Mandatory Three-Actor Pattern

To enforce the strict separation of business logic from network logic, all networked components are **required** to be implemented using the following three-actor pattern.

#### 3.0. Why Three Actors? (Architectural Rationale)

**Critical Design Question:** Why is a middle "Manager" actor necessary? Why not just have a Main Actor that spawns per-peer Network Actors?

**Answer:** Because that would violate the isolation requirement.

If the Main Actor directly manages the `HashMap<PeerId, Addr<NetworkActor>>`, it must:
- Know about individual peer identities (network knowledge)
- Handle peer lifecycle events (network knowledge)
- Implement broadcast fan-out by iterating peers (network knowledge)
- Subscribe to framework event buses (network coupling)

**This pollutes the business logic actor with network concerns.**

The three-actor pattern is **the minimum structure** required to achieve absolute isolation:

```rust
// ❌ WRONG: Main Actor has network knowledge
struct IntentConfigActor {
    config: IntentConfigData,           // ✅ Business logic
    subscribers: HashMap<...>,           // ✅ Business logic
    network_actors: HashMap<PeerId, ...>, // ❌ Network knowledge!
}

// ✅ CORRECT: Network knowledge isolated to Manager
struct IntentConfigActor {
    config: IntentConfigData,           // ✅ Business logic only
    subscribers: HashMap<...>,           // ✅ Business logic only
    network_manager: Addr<...>,          // ✅ Opaque network interface
}

struct IntentConfigNetworkManager {
    main_actor: Addr<IntentConfigActor>,
    network_actors: HashMap<PeerId, ...>, // Network knowledge lives here
}
```

**The Manager is not "bureaucracy" or "forwarding overhead."** It provides critical services:
1. **Lifecycle Management:** Spawns/destroys per-peer actors (network responsibility)
2. **Broadcast Implementation:** Iterates peers to fan-out messages (network operation)
3. **Event Handling:** Subscribes to `PeerLifecycleEvent` bus (framework coupling)
4. **Aggregation:** Collects responses from multiple peers before notifying Main (coordination)

**Without the Manager layer, all of this complexity must live in the Main Actor, which violates the isolation requirement.**

#### 3.1. The `Main Component Actor` (e.g., `IntentConfigActor`)
*   **Role:** The core of the component, containing only pure business logic and authoritative state.
*   **Quantity:** Singleton (one per component type, per application process).
*   **Constraint:** This actor **must not** have any `zznet-*` dependencies in its crate (beyond `-api` types). It is completely network-oblivious. It communicates only with its `Network Manager Actor`.
*   **Enforcement:** Crate dependency graph must not allow imports of `zznet-session`, `zznet-peer-manager`, `zznet-router`, or `zznet-transport-*`. Any violation is a compilation error.

#### 3.2. The `Component Network Manager Actor` (e.g., `IntentConfigNetworkManager`)
*   **Role:** The bridge between the `Main Actor` and the network. It manages the fleet of per-peer `Network Actors`.
*   **Quantity:** Singleton (one per component type).
*   **Responsibilities:**
    *   Subscribes to the `PeerLifecycleEvent` bus from `zznet-peer-manager`.
    *   Spawns and destroys `Network Actors` as peers come and go.
    *   Handles broadcast requests from the `Main Actor`, dispatching them to all relevant `Network Actors`.
    *   Forwards inbound messages from `Network Actors` to the `Main Actor`.
    *   Aggregates responses from multiple peers when appropriate before notifying the `Main Actor`.

#### 3.3. The `Component Network Actor` (e.g., `IntentConfigNetworkActor`)
*   **Role:** Handles the network protocol and translation for a single peer connection.
*   **Quantity:** One per active peer connection for that component.
*   **Responsibilities:**
    *   Holds the `Room<T>` for the connection.
    *   Receives raw messages from the `Room<T>`, deserializes them, and translates them into domain-specific messages for the `Main Actor` (sent via its `Manager`).
    *   Its lifecycle is strictly tied to the peer connection.

### Chapter 4: The Aggressive, Incremental Migration Plan

This refactoring will be executed in a phased, verifiable manner. The `SessionManager` will temporarily act as a **Facade** to the new crates, allowing for incremental migration without a "big bang" rewrite.

**Critical Principle:** Each phase must result in a **fully functional, testable system**. We are not preserving flawed code for convenience; we are creating a structured path to replace it entirely while maintaining the ability to verify correctness at every step.

*   **Phase 0: Foundation - Prove It Works First**

    **Rationale:** Creating empty crates with `todo!()` APIs is premature. We must first extract and prove the new architecture works **within** the existing structure before committing to a new crate boundary.

    **Actions:**
    1. Create the empty `zznet-peer-manager` and `zznet-router` crate directories with placeholder `lib.rs` files (containing only module documentation).
    2. **Inside `zznet-session`**, create new modules: `peer_manager_internal.rs` and `router_internal.rs`.
    3. Mark `zznet-session` with a prominent `FIXME` block at the top of `session_manager.rs`:
       ```rust
       // FIXME: DEPRECATED CRATE - BEING DISMANTLED
       // This entire crate is deprecated and being split into:
       // - zznet-peer-manager (control plane for peer state/identity)
       // - zznet-router (data plane for byte routing)
       //
       // DO NOT ADD NEW FUNCTIONALITY HERE.
       // All new code should target the new crates after Phase 1/2 completion.
       //
       // Migration tracking:
       // - Phase 0: Foundation (IN PROGRESS)
       // - Phase 1: PeerManager extraction (NOT STARTED)
       // - Phase 2: Router extraction (NOT STARTED)
       // - Phase 3: Component migration (NOT STARTED)
       // - Phase 4: Nuke this crate (NOT STARTED)
       ```

    **Stopgap:** Code compiles, all tests pass. New crate directories exist but contain only documentation stubs. Internal extraction modules exist but are empty.

*   **Phase 1: Control Plane Extraction - Internal Implementation First**

    **Actions:**
    1. **Inside `zznet-session/src/peer_manager_internal.rs`**, implement the `PeerManager` struct with all state-related logic extracted from `SessionManager`. This includes:
       - `peers: HashMap<PeerId, PeerState>` (state only, no channels)
       - All peer lifecycle methods (`add_peer`, `remove_peer`)
       - All query methods (`get_peer_role`, `peers_with_role`, etc.)
       - The `PeerLifecycleEvent` broadcast publisher (using `tokio::sync::broadcast`)
    2. Refactor `SessionManager` to hold an internal `PeerManager` instance and delegate all state operations to it.
    3. **Write comprehensive tests** for `PeerManager` within the `zznet-session` crate (in `#[cfg(test)] mod peer_manager_tests`).
    4. Once proven working, **move** `peer_manager_internal.rs` to `zznet-peer-manager/src/lib.rs` and adjust imports.
    5. Mark the delegating methods in `SessionManager` as `#[deprecated(note = "Use PeerManager directly")]`.

    **Test Requirements:**
    - **Not "100% coverage"** (which is a misleading metric).
    - **Behavioral completeness:** Every public method must have tests for:
      - Success cases
      - Error cases (e.g., peer already exists, peer not found)
      - Edge cases (e.g., max_peers limit, empty peer list)
    - **Integration scenarios:** Tests that verify PeerLifecycleEvent publication works correctly.
    - **Test quality review:** Each test must validate a specific, meaningful behavior. "Tests for looks" that just call methods without meaningful assertions are forbidden.

    **Stopgap:** Code compiles (with new deprecation warnings), all tests pass. `PeerManager` is in its own crate and has comprehensive behavioral test coverage. `SessionManager` delegates to it.

*   **Phase 2: Data Plane Extraction - Internal Implementation First**

    **Actions:**
    1. **Inside `zznet-session/src/router_internal.rs`**, implement the `Router` struct with all channel management and routing logic extracted from `SessionManager` and `PeerSession`. This includes:
       - `routes: HashMap<PeerId, mpsc::Sender<(RoomId, Vec<u8>)>>`
       - `send_to_peer(peer_id, room_id, bytes)` method
       - Channel registration/deregistration logic
    2. Refactor `SessionManager` to hold an internal `Router` instance and delegate all routing operations to it.
    3. **Write comprehensive tests** for `Router` within the `zznet-session` crate.
    4. Once proven working, **move** `router_internal.rs` to `zznet-router/src/lib.rs` and adjust imports.
    5. Mark the delegating methods in `SessionManager` as `#[deprecated(note = "Use Router directly")]`.

    **Test Requirements:**
    - **Behavioral completeness:** Tests for successful sends, channel closed errors, peer not found errors.
    - **Concurrent behavior:** Tests that verify routing works correctly with multiple simultaneous senders.
    - **Integration with PeerManager:** Tests that verify Router + PeerManager work together (Router handles bytes, PeerManager knows if peer exists).

    **Stopgap:** Code compiles (with more deprecation warnings), all tests pass. `Router` is in its own crate with comprehensive tests. `SessionManager` is now a thin facade over `PeerManager` + `Router`.

*   **Phase 3: Component-by-Component Migration**

    **Actions:**
    The `SessionManager` is now officially just a temporary facade. We will pick **one component** (e.g., `zzintent-config`) and refactor it completely to the new architecture.

    **Definition of Done for a Component Migration:**
    This refactoring is only "done" when the following are true for the target component (e.g., `zzintent-config`):

    1. **Architecture Compliance:**
       - The `IntentConfigActor` (Main Actor) has **zero `zznet-*` imports** (except for data types from `zznet-api` if needed). This is enforced by the crate's `Cargo.toml` dependencies.
       - The new `IntentConfigNetworkManager` and `IntentConfigNetworkActor` exist and handle all network interaction.
       - The `IntentConfigNetworkManager` subscribes to the `PeerManager`'s event bus to manage its children.
       - The component **no longer uses the `SessionManager` facade**. It interacts directly with `PeerManager` and `Router` actors.

    2. **Test Quality:**
       - A **new suite of tests** is written that independently tests:
         - The `MainActor` with mock network inputs (verifying pure business logic)
         - The `NetworkManagerActor` with mock peer events (verifying peer lifecycle handling)
         - The `NetworkActor` with mock Room<T> messages (verifying protocol translation)
       - Old, tangled integration tests for this component are either **deleted** (if made obsolete) or **updated** to test the new architecture.
       - **Test review:** Each test must be reviewed for meaningfulness. Tests that don't validate specific, useful behavior are deleted.

    3. **Migration Regression Safety (Dual Testing Strategy):**

       **Objective:** Prove that the refactored component is behaviorally equivalent to the old implementation.

       **Process:**
       - If the component has existing integration tests, create a **temporary dual-test harness** that runs those tests twice:
         1. Once with the component wired to the **old SessionManager facade**
         2. Once with the component wired to the **new PeerManager + Router** directly
       - Both test runs must pass with identical results (same outputs, same state transitions, same behavior)
       - If results differ, investigate and fix until behavioral equivalence is proven
       - Once equivalence is verified, **delete the facade-based test** and keep only the new-API test

       **If no integration tests exist:**
       - Document that no regression test is possible
       - Rely on the new actor-level tests for verification
       - Consider adding integration tests as future work (track as technical debt)

       **Why This Matters:** This catches subtle behavioral changes that might slip through unit tests. It ensures the migration doesn't inadvertently break functionality that worked before.

    4. **Code Quality:**
       - **Unused code removed:** Any code in the component that is no longer reachable after the refactor is deleted.
       - **TODOs addressed:** All `TODO`, `FIXME`, `HACK` comments in the component are either:
         - Completed (the functionality is implemented)
         - Elevated to tracked issues (if deferring to later)
         - Deleted (if no longer relevant)

    5. **Verification:**
       - The refactored component is fully functional on the new architecture.
       - All other components continue to function via the `SessionManager` facade.
       - The entire workspace compiles and passes all tests.

    **Stopgap:** Repeat this phase for each networked component (`zzmem-db`, `zzcollector-state`, etc.). After each component migration, the system must compile and all tests must pass.

*   **Phase 4: The Nuke**

    **Actions:**
    1. **Verification:** Run `grep -r "zznet_session" src/` (excluding the `zznet-session` crate itself). The result must be zero occurrences.
    2. **If verification fails:** Identify remaining usages, migrate them to the new APIs, return to Phase 3.
    3. **If verification passes:** Delete the `src/net/zznet-session` directory entirely.
    4. Remove `zznet-session` from workspace `Cargo.toml` members list.
    5. Run `cargo clean && cargo test --workspace` to verify the project builds and all tests pass without the old crate.

    **Stopgap:** The project is fully migrated. The codebase is smaller, cleaner, and strictly follows the new architecture. All tests pass. The `zznet-session` crate no longer exists.

### Chapter 5: Definition of Done

To combat the problem of incomplete, "half-assed" work, every task in this refactoring is bound by a strict **Definition of Done**. A task is only complete when all of the following are met:

1.  **New Code is Written:** The feature is implemented according to the new architecture.

2.  **Old Code is Dealt With:** The code it replaces is either **deleted** or explicitly marked with `#[deprecated]` and a `FIXME` pointing to the new implementation.

3.  **High-Quality Tests are Written:** The new code is accompanied by its own suite of focused, **behaviorally complete** tests that validate its specific responsibilities.

    **"High-Quality" means:**
    - ✅ **Each test validates a specific, meaningful behavior** (not just "calls the method without crashing")
    - ✅ **Success cases, error cases, and edge cases are all covered**
    - ✅ **Tests are reviewed for usefulness**, not just coverage percentage
    - ❌ **"Tests for looks" are forbidden** - tests that don't assert meaningful behavior must be deleted
    - ❌ **"100% coverage" is not the goal** - behavioral completeness is the goal

    **Examples of good vs. bad tests:**
    ```rust
    // ❌ BAD: "Test for looks" - no meaningful assertion
    #[test]
    fn test_add_peer() {
        let mut manager = PeerManager::new();
        let peer = PeerState::new(...);
        manager.add_peer(peer_id, peer); // What does this prove?
    }

    // ✅ GOOD: Tests specific behavior with meaningful assertion
    #[test]
    fn test_add_peer_emits_lifecycle_event() {
        let mut manager = PeerManager::new();
        let mut event_rx = manager.subscribe_events();

        manager.add_peer(peer_id, peer);

        let event = event_rx.try_recv().unwrap();
        assert_matches!(event, PeerLifecycleEvent::Added(id) if id == peer_id);
    }

    // ✅ GOOD: Tests error case
    #[test]
    fn test_add_duplicate_peer_returns_error() {
        let mut manager = PeerManager::new();
        manager.add_peer(peer_id.clone(), peer).unwrap();

        let result = manager.add_peer(peer_id, peer2);

        assert_matches!(result, Err(PeerManagerError::AlreadyExists(_)));
    }
    ```

4.  **All Workspace Tests Pass:** The full `cargo test --workspace` suite must pass without errors.

5.  **Documentation is Updated:** All relevant `README.md`, design documents, and code comments are updated to reflect the new state. Stale documentation is considered a bug.

6.  **Code Cleanup Performed:** (For Phase 3 component migrations only)
    - Unused code is deleted (not commented out, not left "just in case")
    - All `TODO`, `FIXME`, `HACK` comments are addressed (completed, tracked, or deleted)
    - Half-finished features are either completed or explicitly marked as deferred with tracking

**Enforcement:** Any pull request or task submission that does not meet ALL criteria above will be rejected as incomplete. No exceptions.

#### 5.1. Task Specification Checklist Format (Mandatory)

To prevent "half-assed" work, every task assigned to any developer (human or AI) must be specified as an **explicit, verifiable checklist** that directly mirrors the Definition of Done. Tasks are not complete until every checkbox is checked.

**Example: Task Specification for Phase 1**

```markdown
## Task: Implement PeerManager (Phase 1)

**Objective:** Extract peer state management from SessionManager into a new PeerManager component.

**Checklist:**
- [ ] Create `peer_manager_internal.rs` module within `zznet-session/src/`
- [ ] Implement `PeerManager` struct with:
  - [ ] `peers: HashMap<PeerId, PeerState>` field
  - [ ] `add_peer()` method with PeerAlreadyExists error handling
  - [ ] `remove_peer()` method with PeerNotFound error handling
  - [ ] `get_peer_role()` query method
  - [ ] `peers_with_role()` query method
  - [ ] `PeerLifecycleEvent` broadcast channel publisher
- [ ] Write test file `peer_manager_tests.rs` with:
  - [ ] Test: `add_peer()` success case + emits PeerAdded event
  - [ ] Test: `add_peer()` duplicate returns error
  - [ ] Test: `remove_peer()` success case + emits PeerRemoved event
  - [ ] Test: `remove_peer()` non-existent returns error
  - [ ] Test: `get_peer_role()` returns correct role
  - [ ] Test: `get_peer_role()` for non-existent peer returns None
  - [ ] Test: `peers_with_role()` filters correctly
  - [ ] Test: max_peers limit enforcement
- [ ] Refactor `SessionManager` to:
  - [ ] Hold internal `PeerManager` instance
  - [ ] Delegate all state operations to PeerManager
  - [ ] Mark delegating methods with `#[deprecated]`
- [ ] Quality checks:
  - [ ] Run `cargo test --workspace` - all tests pass
  - [ ] Run `cargo clippy --workspace` - zero warnings
  - [ ] Run `grep -r "TODO\|FIXME" zznet-session/src/peer_manager_internal.rs` - all addressed
- [ ] Documentation:
  - [ ] Add module-level documentation to `peer_manager_internal.rs`
  - [ ] Update `02_ZZNet_SOLID_Refactoring_plan.md` Phase 1 status to "IN PROGRESS"
- [ ] Commit with message: "Phase 1: Extract PeerManager internally - [list completed items]"

**Definition of Done:** All checkboxes above are marked complete. No exceptions.
```

**Why This Works:**
- Transforms abstract principles into concrete, verifiable steps
- Prevents task from being marked "done" when only partially complete
- Provides clear scope boundaries (what's in, what's out)
- Creates audit trail of what was actually completed
- Forces explicitness about error cases, edge cases, and quality checks

**Mandatory for All Phases:** Every task in Phases 0-4 must have a checklist specification before work begins.

---

### Chapter 6: Lessons Learned from Skeptical Review

This chapter documents the critical questions, concerns, and architectural debates that shaped this plan. Future implementers should understand the reasoning behind these decisions.

#### 6.1. Question: Is the Three-Actor Pattern Over-Engineered?

**Initial Skepticism:** The three-actor pattern seems like unnecessary bureaucracy. Why not just have the Main Actor spawn per-peer actors directly? The middle "Manager" layer appears to just forward messages.

**Resolution:** This skepticism was **incorrect**. The three-actor pattern is **the minimum structure** required to achieve the stated isolation goal.

**The Critical Insight:** If the Main Actor directly manages per-peer Network Actors, it must:
- Hold `HashMap<PeerId, Addr<NetworkActor>>` (network knowledge)
- Subscribe to `PeerLifecycleEvent` bus (network coupling)
- Implement broadcast by iterating peers (network operation)
- Handle peer connection/disconnection lifecycle (network concern)

**All of this is network logic polluting the business logic actor.** The requirement is explicit: the Main Actor must have **zero network knowledge**. It must not know about individual peers, connections, or PeerIds. It must contain **only** business logic.

**The Manager provides essential services:**
1. Lifecycle management (spawning/destroying per-peer actors)
2. Broadcast implementation (fan-out to multiple peers)
3. Event bus subscription (framework coupling point)
4. Response aggregation (collecting multi-peer responses)

**Verdict:** The three-actor pattern is **mandatory**. It is not over-engineering; it is the correct solution.

#### 6.2. Question: Should We Add Explicit Code Cleanup Phases?

**Initial Proposal:** Add "Phase 1.5: Audit & Cleanup" phases between each extraction to delete unused code, complete half-finished features, and fix meaningless tests.

**Counter-Argument (From Owner):** "The broken architecture is the worst pain point I have. I can't clean up because I don't understand the mess I have."

**Resolution:** The counter-argument is **correct**. Cleanup cannot precede architectural clarity.

**The Root Issue:** In tangled code, it's impossible to distinguish:
- Unused code (safe to delete)
- Network plumbing (looks unused but is critical)
- Half-finished features (needs completion)
- Actually broken code (needs fixing)

**Once the architecture is clean:**
- Business logic lives in Main Actor → obviously necessary or delete it
- Network logic lives in Manager/Network Actors → obviously necessary or delete it
- Anything else → clearly unused, safe to delete
- TODOs become obvious → either in business logic (clear scope) or network logic (clear scope)

**Verdict:** Cleanup is a **natural consequence** of architectural clarity, not a separate phase. The refactoring plan proceeds as-is. Code cleanup will become trivial once structure is clear.

#### 6.3. Concern: Phase 0 Creates Premature APIs

**Initial Plan:** Create empty crates with `todo!()` APIs in Phase 0.

**Skeptical Analysis:** This is premature. We're defining APIs before we've extracted the implementation. This leads to:
- APIs that don't match what the implementation actually needs
- Rework when we realize the API is wrong during Phase 1/2
- False sense of progress

**Resolution:** **Concern accepted.** Phase 0 has been revised.

**New Approach:**
1. Create **empty crate directories** (with documentation stubs only)
2. Extract `PeerManager` and `Router` **internally within zznet-session first**
3. Prove the new structure works with real implementation
4. **Then** move the proven implementation to separate crates

**Benefit:** We only commit to a crate boundary after we've proven the design works. The API emerges from working code, not speculation.

#### 6.4. Concern: "100% Unit Test Coverage" is Misleading

**Initial Requirement:** "The PeerManager must have 100% unit test coverage for all its public methods."

**Skeptical Analysis:** This metric incentivizes:
- Trivial tests (call method, assert it doesn't crash)
- Over-testing getters/setters
- Under-testing complex interactions
- "Tests for looks" that have high coverage but test nothing meaningful

**Resolution:** **Concern accepted.** The requirement has been revised.

**New Requirement: "Behavioral Completeness"**
- Each public method has tests for success cases, error cases, and edge cases
- Critical interactions have integration tests
- Tests are reviewed for **meaningfulness**, not coverage percentage
- "Tests for looks" are explicitly forbidden and must be deleted

**Example:** A test that calls `add_peer()` without asserting the peer was actually added, or that a lifecycle event was emitted, is worthless. It must be replaced with a test that verifies specific, useful behavior.

#### 6.5. Why the Facade Pattern?

**Question:** Why not just rewrite everything in one go? Why the complexity of a temporary facade?

**Answer:** Because "big bang" rewrites are high-risk in production systems.

**The Facade Strategy:**
1. SessionManager becomes a thin wrapper over PeerManager + Router
2. Old code continues working through the facade
3. New code migrates to direct API usage incrementally
4. Once all callers migrated, delete the facade

**Benefits:**
- System remains functional at every step
- Each phase is independently verifiable
- Risk is distributed across multiple small changes
- Rollback is possible at any stopgap point

**Cost:** Temporary complexity (the facade itself). This is an acceptable trade-off for safety.

#### 6.6. Why No Central ComponentSpawner?

**Initial Design:** A single `ComponentSpawner` actor that knows about all components and spawns their network actors when peers connect.

**Rejection:** This creates a new God Object. It violates Open/Closed Principle (must be modified for every new component) and creates tight coupling.

**Correct Design:** **Decentralized Event Bus**
- `PeerManager` publishes `PeerLifecycleEvent` to a `tokio::sync::broadcast` channel
- Each component's `NetworkManager` subscribes independently
- When it sees `PeerAdded`, it decides whether to spawn a `NetworkActor` for that peer
- No central coordinator needed

**Benefits:**
- Zero coupling between components
- Adding new components requires no framework changes
- Each component manages its own network lifecycle
- Fully decentralized and scalable

#### 6.7. Future Consideration: Component Builder Utilities (DEFERRED)

**Question Raised:** Will the three-actor pattern become tedious boilerplate as we create more components?

**Decision:** **Defer any helper utilities until we have real implementation experience.**

**Rationale:**
- We don't yet know what code is actually repetitive vs. component-specific
- Premature abstraction might encode the wrong patterns
- We need to implement 2-3 components manually first to understand the pain points
- Better to have explicit, understandable code than "magic" abstractions

**Action Plan:**
1. Implement first component (`IntentConfig`) manually in full detail during Phase 3
2. Document patterns discovered: what's repetitive, what's unique
3. Implement second component (`MemDB` or `CState`), again in full detail
4. After 2-3 components, evaluate if a `ComponentBuilder` helper or macro would genuinely help
5. **Only then** consider building abstractions

**Evaluation Criteria (for later):**
- Is there clear, repetitive boilerplate that follows the same pattern?
- Would a helper actually simplify development or just hide complexity?
- Can we create an abstraction without sacrificing explicitness?

**Risk of Premature Abstraction:**
- Encoding patterns before we understand them fully
- Creating inflexible frameworks that constrain future development
- Obscuring what's actually happening behind "magic" layers

**Note to Future Self:** Resist the urge to abstract too early. The three-actor pattern should be explicit and understandable in each component until we're absolutely certain what to abstract.

---

### Chapter 7: Summary and Next Steps

**This plan represents the definitive path forward.** It incorporates:
- Rigorous architectural principles (SOLID compliance)
- Skeptical review and debate (questioning every assumption)
- Pragmatic risk mitigation (facade pattern, stopgap points, dual testing)
- Quality enforcement (meaningful tests, code cleanup, Definition of Done, task checklists)
- Execution safety (shared types in zznet-api, regression testing, explicit verification)

**The refactoring will proceed in strict order:**
1. Phase 0: Foundation and internal extraction
2. Phase 1: PeerManager proven and moved to crate
3. Phase 2: Router proven and moved to crate
4. Phase 3: Components migrated one-by-one to three-actor pattern
5. Phase 4: SessionManager deleted

**At every stopgap point, the system must compile and all tests must pass.**

**The end state:**
- `zznet-peer-manager`: Control plane (state/identity/events)
- `zznet-router`: Data plane (byte routing)
- `zznet-api`: Shared types and events (`PeerLifecycleEvent`, `PeerIdentity`, `Role`)
- Components: Three-actor pattern (Main/Manager/Network)
- Zero architectural debt
- Clear boundaries that make cleanup obvious
- Foundation for sustainable development

**Critical Execution Safeguards Added:**
1. **Task Checklist Format (Section 5.1):** Every task must have explicit, verifiable checkboxes
2. **Shared Types Strategy (Section 2.4):** Framework events live in `zznet-api` for minimal coupling
3. **Dual Testing Strategy (Phase 3):** Regression safety via testing both old and new APIs
4. **ComponentBuilder Deferred (Section 6.7):** No premature abstraction until we have experience

**Work begins with Phase 0.**


