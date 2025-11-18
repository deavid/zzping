# **Title: `10_plan_scheduler_overhaul.md`**

- **Author:** David Martínez Martí & Mr.Gemini
- **Date:** November 18, 2025
- **Status:** Proposed

---

## **1. Introduction & Motivation**

A thorough review of the `PingerSchedulerActor` implementation has revealed several subtle but critical deviations from
the canonical design specified in `ZZPINGER_COMPONENT_DESIGN_ADDENDUM_1.md`. These flaws relate to timing, state change
handling, and the core logic of the adaptive rate control.

This document outlines a comprehensive, multi-part plan to correct these issues, bringing the scheduler's behavior in
line with the design principles of robustness, rhythmic consistency, and true hysteresis.

## **2. Summary of Identified Issues**

1. **Ineffective Jitter Buffer:** The scheduler only schedules one ping per target per tick, failing to fill the `10ms`
   schedule-ahead window. This makes the system vulnerable to scheduler jitter.
2. **Flawed `RateTier` Logic:** The adaptive rate tiers currently act as an **override** instead of a **ceiling**. A
   back-off from a low base rate (e.g., 2pps) can incorrectly "accelerate" the rate to a higher tier value (e.g.,
   16pps).
3. **Incorrect "Slow Recovery" Implementation:** The recovery logic is too aggressive. It triggers an upgrade after a
   few successful _pings_, not the "consecutive good _windows_" of data mandated by the design. This leads to rapid
   oscillations, defeating the purpose of hysteresis.
4. **Stale Scheduling on State Changes:** The scheduler fails to realign its timing grid (`next_slot`) after critical
   state changes, leading to incorrect behavior:
   - **On Re-Enabling:** After a long disable, the scheduler attempts to "catch up" on all past pings at once, flooding
     the backend.
   - **On Rate Change:** The first ping after a back-off still fires at the old, faster cadence, breaking the rhythm.

## **3. Action Plan**

The fix is divided into three sequential parts. Parts 1 and 2 address the fundamental flaws in the adaptive rate
algorithm, and Part 3 corrects the timing and scheduling mechanics.

### **Part 1: Correct the `RateTier` Ceiling Logic**

The goal is to ensure rate tiers act as a "max PPS" cap, not an override.

- **1.1. Redefine `RateTier`'s Role:**
  - Remove the `pps()` method from `RateTier`.
  - Introduce a `tier_limit(&self) -> u16` method that returns the tier's maximum PPS (`Pps16` -> `16`, `Unlimited` ->
    `u16::MAX`).
- **1.2. Update `effective_pps` Calculation:**
  - At all call sites (primarily in `handle_tick`), the `effective_pps` must be calculated as:
    `let effective_pps = self.pings_per_second.min(target_state.rate_tier.tier_limit());`

### **Part 2: Implement True "Slow Recovery"**

The goal is to align the recovery mechanism with the design's requirement for sustained good performance over full data
windows.

- **2.1. Rework `TargetState` for Window Tracking:**
  - The `recovery_counter: u32` is flawed. It will be repurposed or replaced.
  - Add `pings_in_current_window: u16` to track progress toward a full window.
  - Add `consecutive_good_windows: u8` to track sustained performance.
- **2.2. Implement Window-Based Recovery Logic:**
  - In the `PingEvent` handler, after updating the history `VecDeque`, increment `pings_in_current_window`.
  - If `pings_in_current_window` is less than `HISTORY_WINDOW_SIZE`, do nothing further regarding recovery.
  - When a full window is completed (i.e., `pings_in_current_window >= HISTORY_WINDOW_SIZE`):
    - Reset `pings_in_current_window` to `0`.
    - Check the success rate. If it's above `RECOVERY_THRESHOLD`, increment `consecutive_good_windows`. If not, reset
      `consecutive_good_windows` to `0`.
    - If `consecutive_good_windows` reaches `RECOVERY_WINDOWS_REQUIRED`, then (and only then) move to the `higher()`
      tier and reset `consecutive_good_windows` to `0`.

### **Part 3: Fix Scheduling and State Realignment**

The goal is to make the scheduling loop robust and ensure timing is always realigned after a state change.

- **3.1. Fix Jitter Buffer Scheduling:**
  - In `handle_tick`, **remove** the early return `if !self.enabled || self.pings_per_second == 0 { return; }`.
    Scheduling must continue even when disabled; the backend's `watch` channel is responsible for dropping work.
  - Change the `if let Some(slot_time) = *next_slot...` to a `while let...` loop to ensure the `SCHEDULE_AHEAD_MS`
    buffer is always filled.
- **3.2. Unify Slot Calculation via Invalidation:**
  - **Prerequisite:** First, fix the bug in `handle_tick` where a `None` slot is initialized. It must use the target's
    `effective_pps` (calculated with the corrected logic from Part 1), not the global `pings_per_second`.
  - **On Rate-Tier Change:** In the `PingEvent` handler, after the logic from Part 2, if the `rate_tier` for a target
    has changed, set its `next_slot` to `None`.
  - **On Config Change:** In the `UpdateIntentConfig` handler, whenever `pings_per_second` changes (new value ≠ old
    value), set the `next_slot` for **all** targets to `None` to prevent firing at stale intervals.
  - **On Re-Enable:** In the `UpdateCState` handler, when `enabled` transitions from `false` to `true`, set the
    `next_slot` for **all** targets to `None` to realign scheduling after a mastership change.

## **4. Expected Outcome**

Upon completion of this plan, the `PingerSchedulerActor` will be fully compliant with its design specification. It will
be robust, predictable, and will correctly implement the principles of jitter absorption and adaptive rate hysteresis.
