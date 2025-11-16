# Plan Ahead: `zzpinger` Rework

This document outlines the aggressive, high-speed execution plan to transform the `zzpinger` component from its current
misaligned state (State A) to the desired state defined in the new design document (State B).

## Guiding Principles & Risk Acceptance

The primary goal is **speed of execution** and **strict adherence to the new design**. We are deliberately choosing a
"scorched earth" strategy over a careful, incremental refactoring.

**This plan explicitly accepts the following risks:**

- The component will be in a non-compilable, completely broken state for a short period.
- Git history for specific logic may be lost in favor of clean, from-scratch rewrites.
- Subtle behaviors from the old implementation will be permanently destroyed, not preserved.

These risks are acceptable. The cost of untangling the old codebase is higher than the cost of a rapid, focused rewrite.

**Our principles are:**

1. **Delete Aggressively:** If a file, dependency, or line of code does not directly serve the new design, it will be
   deleted immediately. We will not comment out dead code.
2. **Rewrite, Don't Refactor:** The core logic is too misaligned to be salvaged. We will start from blank files.
3. **Design is Law:** The `ZZPINGER_COMPONENT_DESIGN.md` document is the single source of truth. No deviations will be
   made.
4. **Embrace Minimalism:** We will implement only what is explicitly required by the design.

## Execution Plan

The plan is broken down into distinct, aggressive phases.

### Phase 1: Annihilation (1-2 hours)

The goal of this phase is to create a clean slate by violently removing all misaligned code. The build **will** be
broken, and this is the desired outcome.

1. **Delete Files:** Immediately delete the following files from `src/components/zzpinger/src/`:
   - `network_actor.rs`
   - `network_manager.rs`
   - `network_messages.rs`
   - `permissions.rs`
2. **Gut Dependencies:** Edit `src/components/zzpinger/Cargo.toml` and remove the following dependencies:
   - `zznet-api`
   - `zznet-room`
   - `zznet-router`
   - `bincode`
   - `async-trait`
3. **Gut `lib.rs`:** Remove all `mod` declarations related to the deleted files.

At the end of this phase, `cargo check` must fail with many errors. This is our new, clean foundation.

### Phase 2: Scaffolding the New Structure (1 hour)

With the old structure destroyed, we will lay down the skeleton of the new design.

1. **Rename `actor.rs` -> `scheduler.rs`**: This file will become the new `PingerSchedulerActor`.
2. **Rename `pinger.rs` -> `backend.rs`**: This file will become the new `PingerBackendActor`.
3. **Clear Contents:** Delete all existing code within the following files, leaving them empty:
   - `scheduler.rs`
   - `backend.rs`
   - `messages.rs`
   - `builder.rs`
   - `api.rs`
4. **Update `lib.rs`:** Set up the new module structure:

   ```rust
   pub mod api;
   pub mod builder;
   pub mod error;
   pub mod messages;

   mod backend;
   mod scheduler;
   ```

### Phase 3: Implementation (Core Work)

This is where the new logic will be written from scratch. We will work from the bottom up: backend first, then
scheduler.

1. **Implement `messages.rs`:** Define the new, minimal message set required by the design (e.g., `UpdateIntentConfig`,
   `UpdateCState`, `PingEvent`, `SchedulePings` command for the backend).
2. **Implement `backend.rs`:**
   - Create the `PingerBackendActor`.
   - It must be a `SyncArbiter` actor.
   - Implement the handler for the `SchedulePings` message. This handler will contain the `surge-ping` logic, perform
     the final high-precision wait, and manage the 10-second timeout for each ping.
   - It will send `PingEvent`s back to a `Recipient<PingEvent>` that is provided upon its creation.
3. **Implement `scheduler.rs`:**
   - Create the `PingerSchedulerActor`.
   - Set up the `ctx.run_interval` for the 1ms scheduling tick.
   - Implement the core clock-alignment logic to determine which targets to ping on each tick.
   - Implement handlers for `UpdateIntentConfig` and `UpdateCState`.
   - Implement the logic to dispatch `SchedulePings` commands to the backend `Recipient`.
   - Implement the handler to receive `PingEvent` batches from the backend and forward them to `MemDB`, including
     backpressure logic.
4. **Implement `builder.rs` and `api.rs`:**
   - Create the new builder, which must accept `Recipient`s for the backend and `MemDB`.
   - Expose the public `Pinger` handle in `api.rs`.

### Phase 4: Integration

1. **Update `main.rs` (or equivalent):** The application's main entry point must be updated to:
   - Create the `PingerBackendActor` pool (`SyncArbiter`).
   - Create the `PingerSchedulerActor` using the new builder, passing in the required `Recipient`s.
2. **Compile & Test:** Compile the entire workspace and run the newly created unit tests.

This aggressive plan prioritizes achieving the target architecture quickly, accepting short-term disruption for
long-term simplicity and maintainability.
