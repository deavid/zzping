## Architectural Decision Record: The ZZNet Framework and Component Model Refactoring

**Date:** 2025-10-27 (Revised)
**Status:** Superseded by `02_ZZNet_SOLID_Refactoring_plan.md`
**Context:** This document is the architectural vision that resulted from the initial audit of the `zznet-*` crates. It correctly identifies the problems and proposes the core solutions (PeerManager/Router split, Three-Actor Pattern). However, the implementation strategy has been refined based on skeptical review and debate. **For the actual implementation plan, see `02_ZZNet_SOLID_Refactoring_plan.md`.**

**Note:** This document remains valuable as the architectural rationale and problem statement. The refined plan in `02_ZZNet_SOLID_Refactoring_plan.md` incorporates lessons learned, addresses concerns about implementation approach, and provides the definitive execution strategy.

### Chapter 1: The Problem Statement and Guiding Principles

#### 1.1. The Problem: Architectural Drift
The project's codebase has suffered from "vibe coding" and uncoordinated work, leading to significant architectural drift. The current implementation, particularly in `zznet-session`, has become a complex monolith that violates core software design principles. It is difficult to reason about, hard to maintain, and brittle. This necessitates a fundamental refactoring based on strict, clear principles.

#### 1.2. The Non-Negotiable Mandates
To correct this, all future work will be governed by the following core principles:

1.  **Strict Separation of Concerns:** Every component, module, and crate must have a single, unambiguous responsibility. Network logic must be completely isolated from business logic. State management must be separated from message routing.
2.  **No Premature Optimization:** Architectural clarity, correctness, and testability are the primary goals. Performance optimizations that compromise these principles (e.g., bypassing clean architectural layers for a minor efficiency gain) are forbidden until proven necessary by profiling.
3.  **SOLID Compliance:** The design must adhere to the SOLID principles. The existing violations are the root cause of the current problems and must be fixed.

### Chapter 2: Skeptical Audit of `zznet-session`

The current `zznet-session` crate is the epicenter of the architectural issues. A skeptical audit reveals multiple violations of SOLID principles.

*   **Single Responsibility Principle (SRP) - VIOLATED:**
    The `SessionManager` is a classic "God Object." It has numerous, unrelated responsibilities: peer lifecycle management, room registration, message routing (unicast and broadcast), protocol negotiation, and state querying. It has far more than one reason to change.

*   **Open/Closed Principle (OCP) - MOSTLY COMPLIANT:**
    The design is correctly open for extension by adding new *room types* through the `RoomHandle` trait. However, its internal logic (routing strategies, lifecycle management) is closed to extension and can only be changed by modification.

*   **Liskov Substitution Principle (LSP) - LARGELY COMPLIANT:**
    The `RoomHandle` trait provides a valid substitution contract at the type level. The main risk is behavioral (e.g., a blocking implementation stalling the system), but the type contract itself is sound.

*   **Interface Segregation Principle (ISP) - VIOLATED:**
    `SessionManager` exposes a single, massive "fat interface." Any client, regardless of its needs, is forced to depend on the entire suite of methods for peer management, routing, and querying. There is no separation of concerns in its API.

*   **Dependency Inversion Principle (DIP) - COMPLIANT:**
    This is the crate's single saving grace and the foundation upon which we can rebuild. It correctly depends on abstractions (`RoomHandle` trait, Tokio channels) rather than concrete implementations (like `zznet-transport-tcp` or specific components). This makes it transport-agnostic and testable.

**Audit Conclusion:** The violations of **SRP** and **ISP** are severe. They confirm that `zznet-session` is doing too much and must be decomposed.

### Chapter 3: The Refactored Framework Layer: Decomposing `zznet-session`

To fix the SOLID violations, `zznet-session` will be deprecated and its responsibilities split into two new, focused crates.

#### 3.1. New Crate: `zznet-peer-manager` (The Control Plane)
*   **Single Responsibility:** To be the authoritative registry for peer state and identity. It answers the questions: "Who is connected?" and "What are their properties?"
*   **Responsibilities:**
    *   Manages peer lifecycle (`add_peer`, `remove_peer`).
    *   Tracks connection state and authentication context (`Role`, `PeerIdentity`).
    *   Provides a query API for peer information (`get_peer_role`, `get_peers_with_role`).
*   **Exclusions:** It has **zero knowledge** of message routing, channels, or bytes.

#### 3.2. New Crate: `zznet-router` (The Data Plane)
*   **Single Responsibility:** To manage transport channels and route serialized byte payloads. It answers the question: "How do I send these bytes to this peer's room?"
*   **Responsibilities:**
    *   Maintains a map of `PeerId` to its outbound transport channel (`mpsc::Sender`).
    *   Provides a simple `send_to_peer(peer_id, room_id, bytes)` API.
*   **Exclusions:** It has **zero knowledge** of peer roles, state, or authentication. It is a simple, stateless byte-forwarder.

#### 3.3. Relocated Logic
*   **`PeerSession`:** Is eliminated. Its state is split between the `PeerManager` and `Router`.
*   **`RoomHandle` / `RoomAdapter`:** Move to `zznet-room`, as they are part of the component-facing abstraction.
*   **Room Negotiation:** Moves up to the `ConnectionManager` in `zznet-hello`, which already orchestrates the connection bootstrap.

This split ensures each framework component has a single, testable responsibility, fixing the SRP and ISP violations.

### Chapter 4: The New Component Architecture: The Mandatory Three-Actor Pattern

To ensure strict separation of concerns within components themselves, all networked components will now follow a mandatory three-actor pattern. This prevents network logic from "leaking" into business logic.

**Why Three Actors?** If the Main Actor directly manages per-peer Network Actors, it must hold `HashMap<PeerId, Addr<NetworkActor>>` (network knowledge), subscribe to framework events (network coupling), and implement broadcasts (network operation). This violates the isolation requirement. The Manager layer is **necessary** to keep the Main Actor completely network-oblivious.

#### 4.1. The `Main Component Actor` (e.g., `IntentConfigActor`)
*   **Responsibility:** Pure business logic and authoritative state management.
*   **Quantity:** Singleton (one per application process).
*   **Awareness:** It is **completely unaware of the network**. It does not know about peers, rooms, or connections. Its only external communication is with its dedicated `NetworkManagerActor`.
*   **Constraint:** Must not have any `zznet-*` dependencies (beyond `-api` types). This is enforced by crate dependency rules.

#### 4.2. The `Component Network Manager Actor` (e.g., `IntentConfigNetworkManager`)
*   **Responsibility:** To act as the bridge between the single `MainActor` and the many `NetworkActors`.
*   **Quantity:** Singleton (one per component type).
*   **Awareness:** It knows about the `MainActor` and all of its associated `NetworkActors`. It subscribes to `PeerLifecycleEvent` from `zznet-peer-manager` to spawn/destroy `NetworkActors`. It is responsible for handling broadcasts from the `MainActor` by dispatching them to all relevant `NetworkActors`.
*   **Why Necessary:** This actor owns all network knowledge (peer IDs, connection lifecycle, broadcast fan-out) so the Main Actor doesn't have to.

#### 4.3. The `Component Network Actor` (e.g., `IntentConfigNetworkActor`)
*   **Responsibility:** To handle the network protocol and translation for a **single peer connection**.
*   **Quantity:** One per active peer connection for that component.
*   **Awareness:** It holds the `Room<T>` for that specific connection. It translates inbound network messages into clean, domain-specific commands for the `MainActor` (via the `NetworkManager`) and translates outbound requests into on-the-wire messages. Its lifecycle is strictly tied to the peer connection.

This pattern guarantees perfect encapsulation and makes each part of a component independently testable.

### Chapter 5: How It All Fits Together: The New Flow

This new architecture creates a clean, layered data flow from the network up to the business logic.

1.  **Startup:** The application binary (`main.rs`) is the "composition root." It starts the framework actors (`PeerManager`, `Router`) and the main component actors (`IntentConfigActor`). Each component's `NetworkManager` is started and subscribes to the `PeerManager`'s event bus.

2.  **New Connection:** `zznet-hello`'s `ConnectionManager` handles the handshake. Upon success, it:
    *   Registers the peer's state with the `PeerManager`.
    *   Registers the peer's transport channels with the `Router`.
    *   The `PeerManager` then emits a `PeerAdded` event to its broadcast channel.

3.  **Component Activation (Decentralized):** Each component's `NetworkManager` independently subscribes to `PeerManager`'s event bus. When it receives a `PeerAdded` event:
    *   It decides whether this component needs to communicate with this peer
    *   If yes, it creates a `Room<T>` using channels from the `Router`
    *   It spawns a new per-peer `NetworkActor`, giving it the `Room<T>` and the address of the Main Actor

    **Note:** There is no central `ComponentSpawner`. Each component manages its own network lifecycle independently via its `NetworkManager`.

4.  **Inbound Message Flow:**
    `Transport` → `Router` → `Room<T>` → `NetworkActor` → `NetworkManagerActor` → `MainActor`.
    (Bytes are translated to domain logic).

5.  **Outbound Broadcast Flow:**
    `MainActor` → `NetworkManagerActor` → (For each peer) `NetworkActor` → `Room<T>` → `Router` → `Transport`.
    (Domain logic is translated to bytes).

### Chapter 6: Conclusion

The previous architecture suffered from tangled responsibilities, leading to a violation of core design principles. The solution is a two-pronged refactoring:

1.  **Framework Decomposition:** Splitting `zznet-session` into a state-focused `zznet-peer-manager` and a routing-focused `zznet-router`.
2.  **Component Pattern Enforcement:** Mandating a three-actor (`Main`, `Manager`, `Network`) pattern for all components to strictly isolate business logic from network concerns.
3.  **Decentralized Event Bus:** Using `PeerManager`'s broadcast channel for lifecycle events, allowing each component's `NetworkManager` to independently manage its network actors without a central coordinator.

This new design is not a suggestion; it is the required architecture for moving forward. It prioritizes clarity, testability, and strict separation of concerns over premature optimization, ensuring the project is built on a robust and maintainable foundation.

**Implementation:** See `02_ZZNet_SOLID_Refactoring_plan.md` for the detailed, phased migration strategy that safely implements this architecture while maintaining a working system at every step.



---------------

I’m going to set up a small tracked checklist to guide this investigation and deliver a clear analysis doc without changing code.

Created 5 todos

Starting (1/5) *Review provided docs/context*

Completed (1/5) *Review provided docs/context*

Progress update: I reviewed the attached ADRs and the migration status report; next I’ll synthesize the key reasons the split stalled so most code remained in zznet-session.

Starting (2/5) *Identify root causes of stall*

Completed (2/5) *Identify root causes of stall*

## ZZNet Session Split: Postmortem and Corrective Action Plan

Date: 2025-10-28
Author: Copilot investigation (no code changes)

### Executive summary

- Intent: split the “God object” zznet-session into focused crates: zznet-peer-manager (control plane), zznet-router (data plane), and a light zznet-api for shared types.
- Reality: only a small percentage of code effectively moved; most logic and dependencies still live in zznet-session.
- Core reason: the system kept importing zznet-session primitives and behaviors (types like PeerId/RoomId/SessionError, PeerSession, adapters/traits), and zznet-session retained re-exports and compatibility shims that made the new crates optional rather than canonical. This preserved coupling and stalled the migration.
- Fix: finish Phase A ruthlessly and remove the session-centric type ownership and re-exports; then proceed with control/data plane extractions. Enforce the three-actor pattern via crate dependencies and remove the escape hatches.

---

## What we intended vs. what happened

### Intended architecture (per 01 + 02 ADRs)
- Decompose zznet-session:
  - zznet-peer-manager: authoritative peer state and lifecycle; emits PeerLifecycleEvent via broadcast.
  - zznet-router: routes serialized byte payloads; transport channel registry; no role/auth knowledge.
  - zznet-api: canonical shared types and framework event types (PeerId, RoomId, Role, PeerIdentity, ConnectionState, SessionError, PeerLifecycleEvent).
- Components adopt a mandatory three-actor pattern (Main, Manager, Network) that isolates business logic from network concerns.
- zznet-session becomes a temporary façade and is then deleted.

### Actual state (per migration status)
- zznet-peer-manager and zznet-router exist and re-export types from zznet-api (good), but:
- zznet-session still holds the majority of implementation and exposes a compatibility layer:
  - Re-exports of canonical types (PeerId, RoomId, SessionError, ConnectionState) and behaviors (PeerSession, adapters/traits).
  - Many crates still import zznet_session::types rather than zznet_api::types (~43 instances noted).
- Outcome: the new crates didn’t become the single source of truth. zznet-session remained a de facto hub, keeping the original coupling and responsibilities intact.

---

## Why 99% stayed in zznet-session

1. Canonical type ownership wasn’t fully flipped
   - Even after adding types to zznet-api, zznet-session re-exported them. This removed urgency to fix imports. Code kept leaning on zznet_session::types as the canonical path.

2. Behavioral monoliths (PeerSession, room traits/adapters) remained in zznet-session
   - The biggest chunks of behavior—peer connection state/translation (PeerSession) and room abstractions/adapters—weren’t relocated. As long as these live in zznet-session, most business and network code must depend on it.

3. Compatibility shims made the split non-enforcing
   - Re-exports from zznet-session and from new crates back to session types created circular convenience. This preserved the old “import everything from session” pattern and made it trivial to defer migration.

4. Ambiguity over ownership boundaries
   - In practice, it was not decided (or enforced) where PeerSession and room abstractions belong (peer-manager vs router vs a new zznet-room). Without clear ownership, teams avoided the risk of moving them.

5. Lack of immediate, localized tests in the new crates
   - The refactor plan requires “move + test” for each extracted responsibility. With tests still in zznet-session or not yet ported, moving behavior felt brittle, so it was postponed.

6. Incremental plan paused at the easiest step
   - Phase A (types) is the safest and lowest risk. After that succeeded and the workspace still passed, the hard migrations (PeerSession, router internals, traits/adapters) weren’t executed immediately.

7. No enforcement at the dependency graph level
    - Components (Main actors) could still depend on zznet-session. Without dependency rules or local checks to block this, the three-actor isolation and framework split could be bypassed.

---

## Evidence lines (from the status report and ADRs)

- Canonical types in zznet-api exist; zznet-session re-exports them.
- New crates (peer-manager/router) re-export api types, but many external crates still import zznet_session::types (≈43 matches across components, hello, test-utils, tests).
- PeerSession and room traits/adapters still in zznet-session; large files like peer_session.rs (~1.6k LOC) remain.
- The plan explicitly called for a zznet-room for room abstractions; not yet executed.
- All tests pass today, indicating we preserved functionality but not isolation.

---

## Risks of the current state

- SOLID violations persist in practice:
  - SRP/ISP: zznet-session remains a central dependency and “fat” interface, so responsibilities are not actually separated.
  - DIP: Components depend on a heavyweight implementation crate rather than stable interfaces in zznet-api.
- Migration fatigue: with compatibility shims, there’s no forcing function to complete the extraction.
- Hidden coupling: new crates appear independent, but real code paths still transit zznet-session types and behaviors.
