
# Architectural Review Notes: `src/common/zznet-lib/src/client.rs`

This document records the findings from the architectural review of the `zznet-lib` client connection helper.

### Summary of Findings

This file is a direct casualty of the necessary redesign of `facade.rs`. It exists only to support the old, imperative architecture and its logic is made entirely redundant by the new declarative model for `zznet`.

### List of Deviations and Notes

1.  **Primary Finding: Obsolete Logic**
    *   **Finding:** The `spawn_connection_manager` function implements a connection/reconnection loop that feeds raw `Connection` objects to the `ZzNetActor`.
    *   **Impact:** This workflow is incorrect in the new declarative model. The `ZzNetActor` should not be fed raw connections; it should be a higher-level session manager that orchestrates "Room" negotiation after a connection is established by the underlying `zznet` runtime.
    *   **Recommendation:** This file should be deleted. The responsibility for managing the client-side connection state machine should be encapsulated within the redesigned `ZzNetActor`.

2.  **Design Smell: Scattered Logic**
    *   **Finding:** The file defines a free-floating `spawn_...` function that acts as a helper task for a component. The component's core logic is therefore scattered outside of its own actor.
    *   **Impact:** This makes the system harder to understand and reason about, as it requires tracing channel interactions between the main actor and its detached helper tasks.
    *   **Principle Violated:** Encapsulation.
    *   **Recommendation:** A component's logic should be self-contained. This reinforces the decision to delete this file and move its responsibilities inside the main component actor.

3.  **Documentation Note: Missing Docstrings**
    *   **Finding:** The public `spawn_connection_manager` function is not documented.
    *   **Recommendation:** All public functions require documentation explaining their purpose and relationship to the system.
