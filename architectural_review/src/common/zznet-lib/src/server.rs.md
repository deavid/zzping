
# Architectural Review Notes: `src/common/zznet-lib/src/server.rs`

This document records the findings from the architectural review of the `zznet-lib` server connection helper.

### Summary of Findings

This file exhibits an even more severe case of the architectural flaws found in `client.rs`. The logic is dangerously scattered across multiple, detached tasks, making the system exceptionally difficult to reason about or debug. Like its client-side counterpart, this file is an implementation detail of an obsolete architecture and must be removed.

### List of Deviations and Notes

1.  **Primary Deviation (Clarity/Simplicity): Dangerously Scattered Logic**
    *   **Finding:** The process for handling a new client is fragmented across several tasks. `spawn_listener` accepts the connection, sends it to the `ZzNetActor`, which then spawns *another* task (`spawn_per_client_event_handler`) just to forward events for that single client back to the `ZzNetActor`.
    *   **Impact:** This pattern is a significant code smell that severely hinders readability and maintainability. A single logical flow is broken into a hard-to-trace chain of message-passing helper tasks.
    *   **Principle Violated:** The goal of a system that is simple and easy to reason about.
    *   **Recommendation:** This multi-task forwarding pattern must be eliminated. The redesigned `ZzNetActor` should manage all of its event sources (e.g., from multiple client connections) within its own single task, for example by using a `StreamMap`.

2.  **Primary Finding: Obsolete Logic**
    *   **Finding:** The file's purpose is to support the old, imperative `zznet` facade by feeding it connection and event information.
    *   **Impact:** The new declarative, automatic "Room" negotiation model makes this entire workflow incorrect and unnecessary.
    *   **Recommendation:** This file must be deleted as part of the `zznet-lib` redesign.

3.  **Design Smell: Incorrect ID Generation**
    *   **Finding:** The `spawn_listener` function uses a manually incrementing integer to generate the `client_id`.
    *   **Impact:** This is the incorrect ephemeral ID model that has been explicitly marked for replacement.
    *   **Recommendation:** All client identification must be based on the new stable, mTLS-derived ID.

4.  **Documentation Note: Missing Docstrings**
    *   **Finding:** All public functions in this file are undocumented.
    *   **Recommendation:** All public items require documentation.
