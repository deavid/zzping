# Plan to Fix `zzpinger` Implementation

- **Author:** David Martínez Martí
- **Date:** November 16, 2025
- **Status:** Proposed
- **Goal:** Provide a clear, actionable plan to fix the critical flaws identified in `04_review_executed_plan03.md` and
  bring the `zzpinger` component into full compliance with the design specification.

## Guiding Principles

The previous phase focused on speed and structural change. This phase focuses on **correctness and robustness**.

1. **Correctness First:** No more shortcuts. The logic must be sound and resilient.
2. **Design is Law (Again):** We will re-read `ZZPINGER_COMPONENT_DESIGN.md` and implement its requirements precisely.
3. **No Code Smells:** We will address not only the outright bugs but also the "ugly" parts of the implementation to
   improve quality and maintainability.

## Execution Plan: A Targeted Strike

This is not a rewrite, but a series of targeted fixes to the existing, cleaned-up codebase.

### Phase 1: Backend Overhaul (`backend.rs`)

The goal is to make the backend performant, correct, and robust.

1. **Fix Critical Resource Leak:**

   - **Action:** Modify the `PingerBackendActor` struct to hold the `surge_ping::Client` as state.
   - **Implementation:** The `Client` will be created _once_ in the `PingerBackendActor::new` function and stored in the
     actor. The `handle` method will then use this long-lived client for all pings, preventing file descriptor
     exhaustion.

2. **Refine Error Handling:**

   - **Action:** Differentiate between timeouts and other network errors.
   - **Implementation:** The `perform_ping` function will be updated. The result of the `timeout` call will be
     inspected. If it's an `Elapsed` error, we emit `PingState::TimedOut`. If it's an `Err` from the inner `pinger.ping`
     call, we emit `PingState::NetworkError`.

3. **Eliminate `tokio::spawn` Code Smell:**
   - **Action:** Remove the `tokio::spawn` call from the `SyncContext` handler.
   - **Implementation:** The `handle` method of a `Sync` actor already runs on a dedicated thread. The ping operation
     can and should be performed synchronously within this handler. The `perform_ping` function will be converted from
     `async fn` to a regular `fn`. This simplifies the code and removes the unnecessary and confusing mix of sync and
     async paradigms.

### Phase 2: Scheduler Rework (`scheduler.rs`)

The goal is to implement the precise, clock-aligned, and stateful scheduling logic required by the design.

1. **Implement Stateful, Catch-up Scheduling:**

   - **Action:** Rewrite the `handle_tick` logic to be stateful instead of reactive.
   - **Implementation:**
     - Add a new state field to `PingerSchedulerActor`, e.g., `next_ping_slot_time: SystemTime`.
     - On each 1ms tick, the logic will not just check the current time. It will check if `SystemTime::now()` has
       surpassed `next_ping_slot_time`.
     - If it has, it will dispatch a ping for that slot and calculate the _next_ slot time based on the
       `pings_per_second` interval.
     - Crucially, if the scheduler was delayed and multiple slots were missed, it should loop and dispatch pings for all
       missed slots to "catch up" (though they will all be dispatched at once), ensuring the correct number of pings are
       sent.

2. **Implement Phase Handling:**

   - **Action:** Add the "phase" concept as required by the design.
   - **Implementation:**
     - Add a compile-time constant: `const PING_PHASE_DEGREES: u16 = 0;`.
     - The scheduling logic will calculate a nanosecond offset from this phase.
       `phase_offset_ns = (1_000_000_000 / pings_per_second) * (phase_degrees / 360)`.
     - This offset will be applied when calculating the `next_ping_slot_time`, shifting the entire ping pattern.

3. **Implement Sequence Numbering:**
   - **Action:** Properly track and assign a sequence number to each ping.
   - **Implementation:**
     - Add a `sequence_counter: u64` to the `PingerSchedulerActor`'s state.
     - When a ping is dispatched, increment this counter.
     - The sequence number must be added to the `SchedulePings` message, passed to the backend, included in the
       resulting `PingEvent`, and finally sent to `MemDB`. This requires modifying all three message/data structures.

### Phase 3: Data Integrity & Cleanup

The goal is to ensure no data is lost or misinterpreted and to fix remaining minor issues.

1. **Fix Lossy Event Conversion:**

   - **Action:** Ensure the full `PingEvent` state is preserved when sending data to `MemDB`.
   - **Implementation:** The `Handler<PingEvent>` on the `PingerSchedulerActor` will be rewritten. It must map
     `PingState::{InFlight, TimedOut, NetworkError, ReceivedRTT}` to the corresponding fields and states expected by
     `zzmem_db::messages::StorePingResult`. This may require updating `zzmem_db`'s message API if it's not already
     sufficient.

2. **Fix Documentation:**
   - **Action:** Correct the inaccurate documentation in `lib.rs`.
   - **Implementation:** Change the phrase "configurable rates and timeouts" to "configurable rates" to reflect that the
     timeout is fixed.

By executing this plan, the `zzpinger` component will be brought into full alignment with the design document, fixing
not just the obvious bugs but also the underlying logical flaws and code smells.
