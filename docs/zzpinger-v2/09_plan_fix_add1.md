# **Title: Aggressive Refactoring Plan for `zzpinger` v2**

- **Author:** David Martínez Martí
- **Date:** November 17, 2025
- **Status:** Proposed
- **Goal:** Outline a concrete, aggressive, step-by-step plan to refactor the `zzpinger` component from its current
  flawed state to the mandatory architecture defined in `ZZPINGER_COMPONENT_DESIGN_ADDENDUM_1.md`.

---

## **1. Guiding Principles**

- **Ruthless Deletion:** We will not refactor existing flawed code. We will delete it and rewrite it correctly.
- **Addendum is Law:** The design addendum is the single source of truth. Any deviation is a bug.
- **Simplicity Over Everything:** We will actively remove unnecessary complexity, abstractions, and premature
  optimizations.
- **Test-Driven Slices:** Each phase should result in a testable, verifiable slice of functionality.

---

## 2. Phase 1: Total Backend Replacement

**Objective:** Eradicate the `SyncArbiter` anti-pattern and replace it with the correct pure-Tokio implementation.

1. **DELETE `src/backend.rs`:** The entire `PingerBackendActor` file must be deleted. It is architecturally unsound and
   cannot be salvaged.
2. **DELETE `UpdateBackendRecipient`:** Remove the `UpdateBackendRecipient` message from `src/messages.rs`. The backend
   is not an optional, hot-swappable component.
3. **CREATE `src/backend/mod.rs`:** Create a new module for the backend. This module will export a single public `async`
   function: `pub async fn run_backend(...)`.
4. **IMPLEMENT `run_backend`:**
   - **Signature:** It will accept the necessary Tokio channel endpoints as arguments:
     - `work_rx: mpsc::Receiver<SchedulePings>`
     - `state_rx: watch::Receiver<bool>`
     - `event_tx: mpsc::Sender<PingEvent>`
   - **Initialization:**
     - Create the `surge_ping::Client` **once** at the start of the function.
     - Initialize an empty `FuturesUnordered` to manage concurrent ping futures.
     - Initialize an empty `HashMap<IpAddr, u16>` to manage ICMP sequence numbers.
   - **Core Loop:** Implement the main `tokio::select!` loop. This loop will race two branches:
     - `work = work_rx.recv()`: To receive new `SchedulePings` commands.
     - `result = futures.next()`: To process the result of a completed ping future.
5. **IMPLEMENT Ping Issuing Logic (inside the `work_rx` branch):**
   - **State Check:** Immediately check the `state_rx` watch channel. If disabled, drop the `SchedulePings` message and
     continue the loop.
   - **High-Precision Wait:** Perform an `async` sleep until the exact moment specified by
     `message.instant + message.fire_duration`.
   - **Concurrent Spawning:** Iterate through the targets in the message. For each target:
     - Create an `async` block that represents the `ping_future`.
     - **Inside the `ping_future`:**
       1. Increment the target's `u16` sequence number in the `HashMap`.
       2. Create a new `pinger` from the shared `client`.
       3. **Measure `SystemTime::now()`** immediately before the `pinger.ping()` call. This is the `actual_send_time`.
       4. Send an `InFlight` `PingEvent` with the `actual_send_time`.
       5. `await` the `pinger.ping()` call.
       6. Process the result (`Ok` or `Err`) and send the final `PingEvent` (`ReceivedRTT`, `TimedOut`, etc.).
     - Push the `ping_future` into the `FuturesUnordered` collection.

---

## 3. Phase 2: Scheduler & Interface Simplification

**Objective:** Remove all unnecessary abstractions and align the scheduler with the new backend interface.

1. **DELETE `src/api.rs`:** This file is a pointless facade and must be removed. The public API will be exposed directly
   from `src/lib.rs`.
2. **CLEANUP `src/builder.rs`:**
   - Delete the `PingerBuilder` struct.
   - Rename the `Pinger` struct to `PingerBuilder`. This struct is the true builder.
   - The `start()` method will now be responsible for:
     1. Creating the Tokio channels for the backend.
     2. Spawning the `run_backend` function onto a new OS thread.
     3. Creating the `PingerSchedulerActor` and providing it the `mpsc::Sender` to the backend's work channel.
     4. Starting the `PingerSchedulerActor` on its dedicated `Arbiter` thread.
3. **REFACTOR `src/scheduler.rs` (`PingerSchedulerActor`):**
   - **State:** Remove `backend_recipient: Option<Recipient<...>>`. Replace it with
     `backend_tx: mpsc::Sender<SchedulePings>`. This is not optional.
   - **State:** Remove the `sequence_counter: u64`. This concept is now fully encapsulated within the backend.
   - **Backpressure:** Delete all manual backpressure logic (`memdb_blocked`, `pending_results`, `flush_memdb_queue`).
   - **Event Handling:** When handling a `PingEvent` from the backend, immediately try to send it to `MemDB` using
     `memdb_recipient.try_send()`. If it fails (returns `Err`), log a warning and drop the event. The design dictates we
     stop pinging if `MemDB` is unavailable, which will be handled by the adaptive interval mechanism.

---

## 4. Phase 3: Implement Scheduler Adaptive Interval

**Objective:** Introduce the stateful, network-aware rate control mechanism into the scheduler.

1. **ADD State to `PingerSchedulerActor`:**
   - Define a new struct, `TargetState`, containing:
     - `rate_tier: RateTier` (an enum of the discrete pps gears).
     - `history: VecDeque<bool>` (a fixed-size sliding window of success/failure).
     - `recovery_counter: u32` (to track consecutive good windows for slow recovery).
   - Add a `HashMap<IpAddr, TargetState>` to the actor's main state.
2. **MODIFY Scheduling Logic (`handle_tick`):**
   - The scheduler's main tick will still align to the global clock.
   - For each time slot, it will iterate through all configured targets in its `TargetState` map.
   - For a given target, it will check if a ping is due in the current time slot _based on the target's current
     `rate_tier`_.
   - Collect all targets due for a ping into the `SchedulePings` message for that slot.
3. **IMPLEMENT Adaptation Logic:**
   - When a `PingEvent` is received from the backend, update the corresponding target's `TargetState`:
     - Push the outcome (`true` for `ReceivedRTT`, `false` for `TimedOut`/`NetworkError`) to its `history` `VecDeque`.
     - **Fast Back-off:** Recalculate the success rate from the `history` window. If it drops below the threshold for
       the current tier, immediately move the target's `rate_tier` down to a lower gear and reset the
       `recovery_counter`.
     - **Slow Recovery:** If the success rate is above the high-water mark for recovery, increment the
       `recovery_counter`. If the counter reaches the required number of consecutive good windows, move the target's
       `rate_tier` up to the next gear and reset the counter.

---

## 5. Final Cleanup and Verification

1. **Review `Cargo.toml`:** Ensure all dependencies are still required.
2. **Review `lib.rs`:** Ensure `pub` exports are minimal and only expose the `PingerBuilder`.
3. **Write Unit Tests:** Add targeted unit tests for:
   - The scheduler's adaptive interval logic (back-off and recovery).
   - The scheduler's clock-aligned slot calculation for different rate tiers.
4. **Write Integration Tests:** Create an integration test that runs the full component (scheduler + real backend) to
   verify end-to-end functionality and timing precision.

---

## **3. Phase 2: Scheduler & Interface Simplification**

**Objective:** Remove all unnecessary abstractions and align the scheduler with the new backend interface.

1. **DELETE `src/api.rs`:** This file is a pointless facade and must be removed. The public API will be exposed directly
   from `src/lib.rs`.

2. **CLEANUP `src/builder.rs`:**

   - Delete the `PingerBuilder` struct.
   - Rename the `Pinger` struct to `PingerBuilder`. This struct is the true builder.
   - The `start()` method will now be responsible for:
     1. Creating the Tokio channels for the backend.
     2. Spawning the `run_backend` function onto a new OS thread.
     3. Creating the `PingerSchedulerActor` and providing it the `mpsc::Sender` to the backend's work channel.
     4. Starting the `PingerSchedulerActor` on its dedicated `Arbiter` thread.

3. **REFACTOR `src/scheduler.rs` (`PingerSchedulerActor`):**
   - **State:** Remove `backend_recipient: Option<Recipient<...>>`. Replace it with
     `backend_tx: mpsc::Sender<SchedulePings>`. This is not optional.
   - **State:** Remove the `sequence_counter: u64`. This concept is now fully encapsulated within the backend.
   - **Backpressure:** Delete all manual backpressure logic (`memdb_blocked`, `pending_results`, `flush_memdb_queue`).
   - **Event Handling:** When handling a `PingEvent` from the backend, immediately try to send it to `MemDB` using
     `memdb_recipient.try_send()`. If it fails (returns `Err`), log a warning and drop the event. The design dictates we
     stop pinging if `MemDB` is unavailable, which will be handled by the adaptive interval mechanism.

---

## **4. Phase 3: Implement Scheduler Adaptive Interval**

**Objective:** Introduce the stateful, network-aware rate control mechanism into the scheduler.

1. **ADD State to `PingerSchedulerActor`:**

   - Define a new struct, `TargetState`, containing:
     - `rate_tier: RateTier` (an enum of the discrete pps gears).
     - `history: VecDeque<bool>` (a fixed-size sliding window of success/failure).
     - `recovery_counter: u32` (to track consecutive good windows for slow recovery).
   - Add a `HashMap<IpAddr, TargetState>` to the actor's main state.

2. **MODIFY Scheduling Logic (`handle_tick`):**

   - The scheduler's main tick will still align to the global clock.
   - For each time slot, it will iterate through all configured targets in its `TargetState` map.
   - For a given target, it will check if a ping is due in the current time slot _based on the target's current
     `rate_tier`_.
   - Collect all targets due for a ping into the `SchedulePings` message for that slot.

3. **IMPLEMENT Adaptation Logic:**
   - When a `PingEvent` is received from the backend, update the corresponding target's `TargetState`:
     - Push the outcome (`true` for `ReceivedRTT`, `false` for `TimedOut`/`NetworkError`) to its `history` `VecDeque`.
     - **Fast Back-off:** Recalculate the success rate from the `history` window. If it drops below the threshold for
       the current tier, immediately move the target's `rate_tier` down to a lower gear and reset the
       `recovery_counter`.
     - **Slow Recovery:** If the success rate is above the high-water mark for recovery, increment the
       `recovery_counter`. If the counter reaches the required number of consecutive good windows, move the target's
       `rate_tier` up to the next gear and reset the counter.

---

## **5. Final Cleanup and Verification**

1. **Review `Cargo.toml`:** Ensure all dependencies are still required.
2. **Review `lib.rs`:** Ensure `pub` exports are minimal and only expose the `PingerBuilder`.
3. **Write Unit Tests:** Add targeted unit tests for:
   - The scheduler's adaptive interval logic (back-off and recovery).
   - The scheduler's clock-aligned slot calculation for different rate tiers.
4. **Write Integration Tests:** Create an integration test that runs the full component (scheduler + real backend) to
   verify end-to-end functionality and timing precision.
