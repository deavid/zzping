
# Architectural Review Notes: `src/common/zznet-lib/src/facade.rs`

This document records the findings from the architectural review of the `zznet-lib` facade.

### Summary of Findings

This facade is the source of several major architectural deviations. Its design is based on an imperative, request-response model for what should be a declarative, message-passing system. It incorrectly treats inter-process "Rooms" (network connections) the same as intra-process `zzchorale` channels. The entire facade requires a fundamental redesign to align with the project's core principles.

### List of Deviations and Notes

1.  **Primary Deviation (Architecture): Incorrect Interaction Model**
    *   **Finding:** The facade exposes an imperative API (`request_channel`, `listen_for_channel`) that encourages a blocking, RPC-style interaction. This is most evident in the `ZzNetApi` trait and the `ActorCommand` enum using `oneshot::Sender` for replies.
    *   **Impact:** This violates the "pure Actor model" (fire-and-forget) principle, pushing complexity into components and risking deadlocks.
    *   **Principle Violated:** Pure asynchronous communication; making invalid states impossible.
    *   **Recommendation:** The facade must be completely redesigned around a declarative, two-phase model:
        1.  **Automatic Negotiation:** Upon connection, the `zznet` layer automatically negotiates a set of available "Rooms." The server should be the source of truth, generating a static mapping of `Room Name -> Room ID` on boot and providing it to connecting clients.
        2.  **Automatic Provisioning:** The `zznet` component must automatically deliver the negotiated Room handles to the components that have declaratively registered a need for them (e.g., via a `UsesNetworkRoom` trait).

2.  **Primary Deviation (Testability): Monolithic `run` Method**
    *   **Finding:** The `ZzNetActor::run` method is a single, untestable `select!` loop that mixes command handling, internal events, and shutdown logic.
    *   **Principle Violated:** The framework should promote testability by default.
    *   **Recommendation:** The actor's logic must be refactored into smaller, testable units (`handle_command`, `handle_internal_event`). The framework itself should own the `select!` loop and transparently manage shutdown signals.

3.  **Primary Deviation (Clarity): Naming Collision**
    *   **Finding:** The code uses the word "Channel" for both intra-process `zzchorale` connections and inter-process `zznet` connections.
    *   **Impact:** This creates significant confusion, as the two have fundamentally different properties and lifetimes.
    *   **Recommendation:** The term "Channel" should be reserved for `zzchorale`. The inter-process `zznet` connections must be renamed to **"Rooms"** throughout the codebase.

4.  **Consequence: API is Obsolete**
    *   **Finding:** The `ZzNetApi` trait and its methods are artifacts of the incorrect imperative model.
    *   **Recommendation:** This trait should be deleted entirely. Components should not have an imperative API to control the network layer; they should declaratively state their needs and be provisioned by the framework.

5.  **Framework Gap: Boilerplate**
    *   **Finding:** The `ZzNetBuilder::start` method and `ZzNetActor::run` loop contain boilerplate logic for actor spawning and shutdown.
    *   **Recommendation:** This logic should be abstracted away and managed by the `zzchorale` framework itself.
