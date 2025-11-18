# **Title: `ZZPINGER_COMPONENT_DESIGN_ADDENDUM_2.md`**

- **Author:** David Martínez Martí
- **Date:** November 18, 2025
- **Status:** Final / Authoritative
- **Amends:** `docs/design/ZZPINGER_COMPONENT_DESIGN.md`
- **Supersedes:** `docs/design/ZZPINGER_COMPONENT_DESIGN_ADDENDUM_1.md` (Specifically sections regarding adaptive rate
  control)

---

## **1. Introduction: The "Simplification" Doctrine**

This document establishes the final, definitive architectural direction for the `zzpinger` component. It explicitly
**rejects and overrides** the complex adaptive rate control mechanisms proposed in Addendum 1.

During implementation and review, it was determined that adaptive logic ("backing off" during packet loss) creates two
fundamental problems:

1. **Data Corruption:** It masks the granularity of network outages. If a user configures 30pps, they expect 30 data
   points per second. Reducing the rate hiding the severity of packet loss is unacceptable.
2. **Architectural Complexity:** The state machines required for hysteresis and recovery introduced significant
   complexity and potential for bugs without solving a real problem.

Therefore, `zzpinger` returns to a philosophy of **Simplicity, Precision, and Obedience.**

---

## **2. Core Principles**

1. **Obedience:** The pinger is "dumb." If configured for 30 pps, it sends 30 pps, regardless of network conditions
   (packet loss, timeouts).
2. **Precision:** The primary technical goal is maintaining a perfect, clock-aligned rhythm with <0.5ms jitter to
   support downstream compression.
3. **Silence:** System errors (e.g., socket configuration issues) must not flood the logs or disk, but they must not
   stop the engine. This is solved via **Log Rate Limiting**, not execution stopping.

---

## **3. Revised Scheduler Architecture**

The Scheduler (`PingerSchedulerActor`) is stripped of all adaptive logic (`RateTier`, `TargetState`, history windows).
Its sole responsibility is chronological dispatch.

### **3.1. The "Bucket Sort" Dispatch (Mandatory)**

To prevent timing drift and head-of-line blocking in the backend, the Scheduler **must** use a "Bucket Sort" algorithm
for dispatch.

- **Algorithm:**
  1. On every 1ms tick, calculate the lookahead deadline (e.g., `now + 10ms`).
  2. Iterate through **all** targets. For each target, calculate pending ping slots up to the deadline.
  3. Insert targets into a `BTreeMap<SystemTime, Vec<IpAddr>>`. This automatically groups targets by their exact
     execution time slot.
  4. Iterate the `BTreeMap` and send **one** `SchedulePings` message per time slot, containing the `Vec<IpAddr>` of all
     targets for that instant.

**Why:** Sending unbundled messages (one per target) forces the backend to wake up, process, sleep, and spawn for every
single target, accumulating micro-latencies that drift the schedule. Bundling ensures all targets for a slot are spawned
in a single tight loop.

---

## **4. Revised Backend Architecture**

The Backend remains a pure Tokio task (not an Actor) running in a dedicated thread, managing its own concurrency via
`FuturesUnordered`.

### **4.1. The "Muffler" (Log Rate Limiting)**

The solution to "Log Spam" (filling the disk with errors when misconfigured) is implemented strictly in the Backend,
without stopping execution.

- **Mechanism:**
  - The Backend maintains a `last_error_log: Instant` timestamp.
  - When a `PingState::NetworkError` occurs (e.g., `send_to` fails):
    1. Check if `Instant::now() > last_error_log + Duration::from_secs(5)`.
    2. **If True:** Log the error using `error!()` and update `last_error_log`.
    3. **If False:** Suppress the log.
  - Regardless of logging, the `NetworkError` event is **always** sent to `MemDB`.

### **4.2. Data Hygiene**

- **No Sequence Numbers:** The `u16` ICMP sequence number is a private implementation detail required by the socket
  protocol. It must **not** be leaked in `PingEvent` or sent to `MemDB`. It serves no purpose outside the socket
  matching logic.
- **Measurement Point:** The Backend must capture `SystemTime::now()` **inside** the concurrent future, immediately
  before the socket call. This measured time is the authoritative `sent_time` reported in the event.

---

## **5. Summary of Removed Concepts**

For clarity, the following concepts from previous designs are **dead** and must not appear in the codebase:

- ❌ **Adaptive Rate Control:** No backing off, no "slow recovery," no "gears."
- ❌ **Target History:** The scheduler does not track success/failure rates.
- ❌ **UpdateBackendRecipient:** The backend is mandatory and static; it is not hot-swappable.
- ❌ **Public Sequence Numbers:** Removed from all inter-component messages.
