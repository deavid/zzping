# Analysis of the `zzpinger` Component Design

This document outlines the understanding of the design document for the `zzpinger` component rework, as specified in
`docs/design/ZZPINGER_COMPONENT_DESIGN.md`.

## Core Understanding

The project owner wants to scrap the existing `zzpinger` component and rebuild it from scratch following a strict, new
design. The previous "vibe-coding" approach by AI agents resulted in an overly complex and messy implementation. The new
vision prioritizes simplicity, precision, and a clear separation of concerns, explicitly rejecting features that are not
absolutely necessary (YAGNI).

## Key Requirements & Vision

Based on the design document, the key goals are:

1. **High Precision, Clock-Aligned Pinging:** This is the most critical requirement. The component must send ICMP pings
   at a constant, system-clock-aligned interval (e.g., exactly at `HH:MM:SS.000`, `HH:MM:SS.500` for 2 pps). The
   acceptable jitter is extremely low, under 500 microseconds (0.5ms). This precision is non-negotiable as it is
   fundamental for downstream data compression in `MemDB`.

2. **Decoupled Actor Architecture:** The design mandates a two-part actor model:

   - **`PingerActor` (Scheduler):** Its sole responsibility is scheduling. It determines _when_ pings should be sent for
     each target based on the configured pps. To minimize jitter, it must run on its own dedicated thread.
   - **`PingerBackendActor` (Executor):** Its responsibility is to execute the pings as commanded by the scheduler. It
     will handle the final sub-millisecond wait to meet the precision target. It runs in a thread pool (`SyncArbiter`)
     to handle concurrent ping requests.

3. **Clear, Minimal Interfaces:**

   - **Inputs:** The component only receives configuration from two sources via Actix messages: `IntentConfig` (targets,
     pps) and `CState` (enable/disable). It does not read any files or command-line flags itself.
   - **Output:** The only output is a stream of `PingEvent`s sent to the `MemDB` component.
   - **Backend Dependency:** The backend (real or mock) is not created by the component. It is instantiated in the main
     application and passed into the `PingerActor`'s builder, ensuring a clean separation of concerns.

4. **Rejection of Unnecessary Complexity (YAGNI):** The design explicitly forbids several features present in the old
   implementation:

   - **No `zznet` tooling:** The component is self-contained and does not communicate with other processes over `zznet`.
   - **No Health Stats:** All metrics can and should be derived from the data in `MemDB`. The component will not emit
     any metrics itself.
   - **No Configurable Timeout:** The ICMP timeout is fixed at 10 seconds. The backend is responsible for generating
     `TimedOut` events.

5. **Robust Failure & Backpressure Handling:**

   - If `MemDB` cannot receive events, the `PingerActor` must stop sending pings until the situation recovers to avoid
     memory leaks.
   - The component should accept and degrade gracefully under misconfiguration (e.g., extreme pps values) rather than
     trying to enforce policy.

6. **Testability:** The architecture must be testable. The `PingerBackendActor` abstraction is key, allowing a
   `MockPingBackend` to be swapped in during tests to validate scheduling logic without real network traffic. The
   problem of mocking `SystemTime` is acknowledged but deferred to avoid premature design.

In summary, the owner's intent is to build a highly specialized, precise, and reliable component that does one thing and
does it exceptionally well, while aggressively avoiding feature creep and complexity. The focus is on robust, simple,
and maintainable code that adheres strictly to the provided architectural vision.
