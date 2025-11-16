# Review of Executed Plan 03: `zzpinger` Rework

- **Author:** David Martínez Martí
- **Date:** November 16, 2025
- **Status:** Needs Rework

This document provides a skeptical and blunt review of the work executed following `03_plan_ahead.md`. The goal was to
refactor the `zzpinger` component to align with the new vision in `ZZPINGER_COMPONENT_DESIGN.md`.

## Overall Assessment

The execution of the plan was aggressive, but the results are mixed. The team successfully followed the "scorched earth"
policy, removing the old `zznet` integrations and scaffolding the new actor structure. However, the implementation of
the core logic is riddled with significant flaws that violate the spirit and letter of the design document.

The component is **not "fully done"**. It's a facade that looks correct from a distance but is broken on the inside. It
requires another major pass to fix critical logic flaws.

---

## Detailed Review

### The Good

- **Annihilation Complete:** The old, misaligned files (`network_*`, `permissions.rs`) and dependencies (`zznet-*`,
  `bincode`) have been successfully removed. The project structure is now clean and free of the old `zznet` integration.
  This part of the plan was executed perfectly.
- **Actor Scaffolding Correct:** The new file structure (`scheduler.rs`, `backend.rs`) and the basic actor definitions
  are in place as per the plan. The module structure in `lib.rs` is clean.
- **`SyncArbiter` Backend:** The `PingerBackendActor` is correctly implemented as a `Sync` actor
  (`type Context = SyncContext<Self>;`). This was a critical and non-obvious requirement from the design document to
  handle blocking I/O and precise timing, and it was implemented correctly.
- **Minimal Interfaces:** The message definitions in `messages.rs` are clean, minimal, and perfectly match the new
  design's API (`UpdateIntentConfig`, `UpdateCState`, `PingEvent`, etc.).

### The Bad (Critical Flaws)

1. **Broken Scheduling Logic (`scheduler.rs`):** This is the most severe issue. The current implementation is purely
   reactive. It only fires a ping if the 1ms tick happens to land _exactly_ on a scheduled time slot. If the scheduler
   is delayed by even a millisecond and misses the window, the ping is skipped entirely. This completely fails the core
   requirement of a robust, clock-aligned, non-negotiable ping interval. The system will miss pings constantly under any
   real-world load. A proper implementation must be stateful, tracking the _next_ scheduled time and firing when that
   time has passed.
2. **Massive Performance Bug (`backend.rs`):** The backend creates a new `surge-ping::Client` for _every single ping_
   (`let client = Client::new(&Config::default())?;`). A `Client` is a heavy object that opens a raw socket. At any
   reasonable pps rate (e.g., 100 pps), this will instantly exhaust file descriptors and crash the process. This is a
   critical performance bug that shows a lack of understanding of the underlying library. The `Client` should be created
   once per actor.
3. **Ignored "Phase" Requirement:** The design document explicitly required preparing the codebase for a "phase"
   configuration, even if it was just a compile-time constant of zero, to support future needs. This requirement was
   completely ignored. There is no trace of phase-handling logic or even a placeholder constant.

### The Ugly (Other Flaws & Code Smells)

- **Lossy Event Conversion:** The `scheduler` incorrectly converts the rich `PingEvent` (which includes `InFlight`,
  `TimedOut`, `ReceivedRTT` states) into a simple `PingResult` for `MemDB` that only contains an optional RTT. This
  loses critical state information that the design specified `MemDB` should receive.
- **Missing `NetworkError` State:** The `backend` lumps all ping failures into `TimedOut`, failing to implement the
  distinct `NetworkError` state required by the design document.
- **Inefficient `TODO` for Sequence:** The code has a `TODO` for the ping sequence number, which is a key piece of data
  for tracking individual pings. This was left unimplemented, making the data sent to `MemDB` incomplete.
- **Awkward `tokio::spawn`:** The use of `tokio::spawn` inside a synchronous `SyncContext` handler in the backend is a
  code smell. While it might function, it's not a clean pattern and mixes synchronous and asynchronous paradigms
  unnecessarily. The entire ping operation could be handled within the synchronous context.
- **Inaccurate Documentation:** The `lib.rs` documentation incorrectly states that the ping timeout is configurable
  ("configurable rates and timeouts"), which directly contradicts the design document's explicit statement that the
  timeout is fixed at 10 seconds.

---

## Conclusion

This is a "looks good from my house" implementation. The file names are correct, but the core logic is fundamentally
broken and does not meet the design's primary requirements for precision and robustness. It needs another pass, this
time focusing on **correctness** instead of just structural changes.

**The current implementation is not shippable.** It must be reworked to address the critical flaws in scheduling and
backend resource management before it can be considered a successful execution of the plan.
