# **Title: `12_simplification_spec.md`**

- **Author:** David Martínez Martí & Mr.Gemini
- **Date:** November 18, 2025
- **Status:** Final / Approved
- **Supersedes:** `10_plan_scheduler_overhaul.md`, `ZZPINGER_COMPONENT_DESIGN_ADDENDUM_1.md` (Partial override on
  adaptive logic)

---

## **1. Introduction: The "Dumb Pinger" Doctrine**

This document establishes the final architectural specification for `zzpinger`.

We explicitly **reject** the complexity of adaptive rate control, hysteresis, and graceful degradation. Packet loss is
data; masking it by slowing down is data corruption.

**Core Principles:**

1. **Obedience:** If the user configures 30 pps, the pinger sends 30 pps. Rain or shine.
2. **Precision:** The primary goal is maintaining a perfect, clock-aligned rhythm (<0.5ms jitter) to enable downstream
   data compression.
3. **Silence:** System errors (`NetworkError`) must not flood the logs. We handle this via logging rate-limits, not by
   stopping the engine.

---

## **2. The Scheduler: Chronological Bucket Sort**

The Scheduler's only job is to calculate **when** pings should happen and dispatch them to the backend in strict
chronological order.

To fix the "Non-Chronological Dispatch" flaw (Doc 11), the Scheduler **must** implement the following algorithm in its
`handle_tick` loop.

### **2.1. State**

- `pings_per_second: u16` (Global config)
- `targets: Vec<IpAddr>`
- `next_slot: HashMap<IpAddr, SystemTime>` (Tracks the next fire time for each target)
- **NO** rate tiers, history windows, or recovery counters.

### **2.2. The "Bucket Sort" Algorithm**

On every tick (1ms interval):

1. **Define Window:** Calculate `deadline = SystemTime::now() + Duration::from_millis(10)`.
2. **Bucket:** Initialize a temporary `BTreeMap<SystemTime, Vec<IpAddr>>`.
   - _Why BTreeMap?_ It automatically sorts keys (time) so we iterate chronologically.
3. **Fill:** Iterate through **all** `targets`:
   - While `target.next_slot <= deadline`:
     - Insert `target` into the `Vec` at `target.next_slot` in the BTreeMap.
     - Advance `target.next_slot` by `1 / pps`.
4. **Dispatch:** Iterate through the `BTreeMap`:
   - For each `(time, targets_vec)`, send **one** `SchedulePings` message to the backend.
   - _Crucial:_ This bundles targets sharing a slot into a single message, minimizing backend wake-ups and drift.

---

## **3. The Backend: Concurrent Execution & The "Muffler"**

The Backend is a pure Tokio task (not an Actix actor) running in a dedicated thread.

### **3.1. The Contract**

- **Input:** `SchedulePings { aligned_time, instant, fire_duration, targets: Vec<IpAddr> }`.
- **Output:** `PingEvent { target, sent_time, state }`.
- **Concurrency:** It **must** use `FuturesUnordered` to execute all pings in a batch concurrently.

### **3.2. Log Rate Limiting (The "Muffler")**

To solve the log spam problem without stopping the pinger:

- **State:** Maintain a `last_error_log: Instant`.
- **Logic:** When a `NetworkError` occurs (e.g., socket failure, permission denied):
  1. Check `Instant::now() > last_error_log + Duration::from_secs(5)`.
  2. If true: Log the error via `error!()` and update `last_error_log`.
  3. If false: **Do not log.** Silently emit the `PingEvent` with `state: NetworkError`.
- **Result:** The hard drive is safe. The MemDB still receives the error data. The system keeps running.

---

## **4. Data Hygiene**

### **4.1. No Sequence Numbers**

- The `u16` ICMP sequence number is a private implementation detail of the Backend (required to match replies).
- It **must not** be exposed in `PingEvent`, `SchedulePings`, or sent to `MemDB`.
- Remove all `sequence` fields from public structs.

### **4.2. Precise Timing**

- The Backend **must** capture `SystemTime::now()` inside the async task, **immediately** before calling
  `pinger.ping()`.
- This measured time is what `MemDB` receives. We do not trust the scheduled time.

---

## **5. Implementation Plan Checklist**

- [ ] **Scrub:** Remove `RateTier`, `TargetState` (history/recovery), and `UpdateBackendRecipient` logic.
- [ ] **Scrub:** Remove `sequence` fields from `messages.rs` and `scheduler.rs`.
- [ ] **Scheduler:** Rewrite `handle_tick` to use the `BTreeMap` bucket sort pattern.
- [ ] **Backend:** Implement the `last_error_log` throttle.
- [ ] **Backend:** Ensure `client` is created once, `pingers` are not cached (premature opt), and `FuturesUnordered` is
      used.
