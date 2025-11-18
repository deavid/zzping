# Plan: `zzpinger` Correctness and Resilience Fixes

- **Goal:** Implement strict backpressure handling, crash-on-failure logic, and fix the "Obedience" violation in the
  backend.
- **Execution:** This plan is designed for a coding agent. Follow the instructions literally.

## 1. Scheduler Refactoring (`src/components/zzpinger/src/scheduler.rs`)

### 1.1. Update Struct State and Constants

1. **Add Constants:** Define three new constants at the top of the file:
   - `MAX_PENDING_RESULTS`: 10,000
   - `BACKPRESSURE_PAUSE_THRESHOLD`: 1,000
   - `BACKPRESSURE_RESUME_THRESHOLD`: 100
2. **Update Struct:** Add two fields to `PingerSchedulerActor`:
   - `pending_results`: A `VecDeque<StorePingResult>` (requires importing `VecDeque`).
   - `memdb_blocked`: A `bool`, initialized to `false`.

### 1.2. Implement `flush_pending_results` Helper

Create a private method `flush_pending_results(&mut self)` in `PingerSchedulerActor`. **Logic:**

1. Loop while `pending_results` is not empty.
2. Peek at the front item.
3. Attempt to send it to `memdb_recipient` using `try_send`.
4. **Match the result:**
   - **Ok:** Pop the item from the front of the queue. Continue loop.
   - **Err(Full):** The pipe is clogged. **Stop** the loop immediately (return).
   - **Err(Closed):** The MemDB actor is dead. **Panic** immediately with a clear message ("Critical dependency MemDB
     lost").

### 1.3. Rewrite `Handler<PingEvent>`

Completely replace the existing implementation of `handle(PingEvent)`. **Logic:**

1. Convert the `PingEvent` to a `PingResult` / `StorePingResult` (existing logic).
2. Push the `StorePingResult` to the back of `self.pending_results`.
3. **Safety Cap:** If `pending_results` length exceeds `MAX_PENDING_RESULTS`:
   - Pop one item from the **front** (drop oldest).
   - Log a warning ("Dropping result due to buffer overflow").
4. Call `self.flush_pending_results()`.

### 1.4. Rewrite `handle_tick` (The Main Loop)

Modify the existing `handle_tick` method. **Logic Sequence:**

1. **Flush:** Call `self.flush_pending_results()` at the very start.
2. **Hysteresis Check:**
   - If `pending_results` length < `BACKPRESSURE_RESUME_THRESHOLD`, set `memdb_blocked = false`.
   - If `pending_results` length > `BACKPRESSURE_PAUSE_THRESHOLD`, set `memdb_blocked = true`.
3. **Block Check:**
   - If `memdb_blocked` is `true`:
     - Iterate over all targets in `self.next_slot` and set their value to `None`. (This invalidates the schedule so it
       restarts from `now` when we resume).
     - **Return** immediately (do not schedule any pings).
4. **Scheduling (Unblocked):**
   - Proceed with the existing Bucket Sort / Scheduling logic.
   - **CRITICAL CHANGE:** When sending `SchedulePings` to `backend_tx`, match the `try_send` result:
     - **Ok:** Continue.
     - **Err(Full):** Log a warning ("Backend overloaded").
     - **Err(Closed):** **Panic** immediately ("Backend thread died").

### 1.5. Cleanup

- Remove any comments marked `// OWNER REVIEW` or comments debating the correctness of dropping data.

---

## 2. Backend Refactoring (`src/components/zzpinger/src/backend.rs`)

### 2.1. Fix "Obedience" in `execute_ping`

Modify the `execute_ping` function. **Logic:**

1. Locate the `scheduler_addr.try_send(InFlight...)` block.
2. **Remove the `return None` statement.**
3. Update the logic to:
   - Attempt `try_send`.
   - If **Err(Full)**: Log a warning ("Scheduler mailbox full, dropping InFlight event") but **continue** execution (do
     not abort the ping).
   - If **Err(Closed)**: **Panic** immediately ("Scheduler actor died").
   - If **Ok**: Continue.

---

## 3. Verification

- Ensure code compiles.
- Ensure `VecDeque` is imported in `scheduler.rs`.
- Ensure no `sequence` fields remain in `scheduler.rs` (already removed in previous step, but double check).
