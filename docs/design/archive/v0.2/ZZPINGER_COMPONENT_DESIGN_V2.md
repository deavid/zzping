# **Title: `ZZPINGER_COMPONENT_DESIGN_V2.md`**

NOTE: Deprecated documentation.

- **Author:** David Martínez Martí & GitHub Copilot
- **Date:** November 18, 2025
- **Status:** Final / Authoritative
- **Supersedes:** `ZZPINGER_COMPONENT_DESIGN.md`, `ZZPINGER_COMPONENT_DESIGN_ADDENDUM_1.md`,
  `ZZPINGER_COMPONENT_DESIGN_ADDENDUM_2.md`

---

## **1. Introduction & Vision**

The `zzpinger` component is the heartbeat of the system. Its sole responsibility is to execute ICMP pings to a list of
targets in a perfectly constant, clock-aligned fashion and collect precise timing data.

This document consolidates the architectural vision, establishing the **"Simplification Doctrine"** as the governing
philosophy. We explicitly reject complex adaptive behaviors in favor of determinism and precision.

### **1.1. Core Principles**

1. **Obedience:** The pinger is "dumb." If configured for 30 pps, it sends 30 pps, regardless of network conditions
   (packet loss, timeouts). It does not "back off" or "recover."
2. **Precision:** The primary technical goal is maintaining a perfect, clock-aligned rhythm with <0.5ms jitter. This
   regularity is critical for downstream data compression in MemDB.
3. **Silence:** System errors (e.g., socket configuration issues) must not flood the logs or disk. We handle this via
   **Log Rate Limiting** (the "Muffler"), not by stopping execution.

---

## **2. Rationales & Key Decisions**

This section details the "Why" behind the major architectural choices.

### **2.1. Rationale for Removing Adaptive Control**

We explicitly removed adaptive pinging (rate back-off, hysteresis, "gears") for four key reasons:

1. **Data Integrity (The "Lie"):** Slowing down during an outage masks the severity of the problem. If a user configures
   30 pps, they expect 30 sample points per second. 100% loss at 30Hz is valuable data; 100% loss at 1Hz (because we
   backed off) is corrupted data.
2. **The "Good Neighbor" Fallacy:** Modern network equipment is not impacted by 30–100 ICMP packets per second.
   Designing complex back-off logic to protect defective hardware was solving a non-existent problem.
3. **Compression Efficiency:** The downstream database's compression algorithms rely on constant, predictable intervals
   (delta encoding). Constantly changing rates introduces jitter that degrades compression.
4. **Operational Simplicity:** The operational pain point—**Log Spam**—is solved more simply by a "Muffler" (log rate
   limiter) than by complex state machines.

### **2.2. Rationale for Pure Tokio Backend (vs. Actix SyncArbiter)**

The initial design used Actix's `SyncArbiter` for the backend. This was rejected because `SyncArbiter` provides a
synchronous context. Running an async-native library like `surge-ping` inside it required creating a new Tokio runtime
for _every single ping_, causing massive resource churn.

### **2.3. Rationale for "Bucket Sort" Scheduling**

We mandate a "Bucket Sort" algorithm in the scheduler to solve **Timing Drift**. Sending individual messages to the
backend forces it to wake up/sleep repeatedly for targets sharing the same millisecond slot. Bundling them ensures all
targets for a slot are spawned in a single tight loop.

---

## **3. System Architecture**

The component consists of two distinct entities running in separate threads to ensure isolation and timing precision.

### **3.1. The Scheduler (`PingerSchedulerActor`)**

The Scheduler is an Actix actor responsible for calculating **when** pings should happen.

- **Role:** Chronological Dispatcher.
- **Algorithm (Bucket Sort):**
  1. On every 1ms tick, calculate a lookahead deadline (e.g., `now + 10ms`).
  2. Iterate through **all** targets. For each target, calculate pending ping slots up to the deadline based on its PPS.
  3. Insert targets into a `BTreeMap<SystemTime, Vec<IpAddr>>`. This groups targets by their exact execution time slot.
  4. Iterate the `BTreeMap` and send **one** `SchedulePings` message per time slot to the Backend.

### **3.2. The Backend (Pure Tokio Task)**

The Backend is **not** an Actor. It is a dedicated, long-running `async` function (`run_backend`) running on its own OS
thread with a private Tokio runtime.

- **Role:** Concurrent Execution Worker.
- **Resource Management (CRITICAL):** The `surge_ping::Client` (and its underlying raw socket) must be instantiated
  **exactly once** when the backend task starts. It must be reused for all pings. Creating clients per-ping is forbidden
  to prevent file descriptor exhaustion.
- **Concurrency Model:** Uses `FuturesUnordered` to manage in-flight pings. This prevents head-of-line blocking.
- **Isolation:** Runs on a dedicated `Arbiter` thread.

#### **Implementation Detail: The Backend Loop**

The backend must implement a `tokio::select!` loop racing two branches:

1. **Work Channel (`work_rx.recv()`):** Receives `SchedulePings`. Performs the high-precision sleep, then spawns ping
   futures into the `FuturesUnordered` set.
2. **Futures Completion (`futures.next()`):** Receives completed `PingEvent`s (success or timeout) and forwards them to
   the Scheduler.

---

## **4. Interfaces & Data Flow**

### **4.1. Inputs**

- **`IntentConfig`:** `UpdateIntentConfig { targets, pings_per_second }`. Replaces target list.
- **`CState`:** `UpdateCState { enable: bool }`. Acts as a master switch. If `false`, the Backend drops work immediately
  upon receipt (via a `watch` channel check).

### **4.2. Outputs**

- **`MemDB`:** `PingEvent { target_host, sent_time, state }`.
  - **Timeout Policy:** The ICMP timeout is strictly hardcoded to **10 seconds**. It is not configurable.

### **4.3. Internal Communication**

- **Work Channel (`mpsc::Sender<SchedulePings>`):** Carries the batch of targets and the precise `Instant` to fire them.
- **State Channel (`watch::Sender<bool>`):** Instant propagation of the Enable/Disable signal.
- **Event Channel (`mpsc::Sender<PingEvent>`):** Returns results to the Scheduler.

---

## **5. Timing & Precision Model**

### **5.1. Clock Alignment & Phase**

Pings are **system-clock aligned**.

- **Alignment:** If configured for 2 pps, pings fire at `SS.000` and `SS.500`.
- **Phase Handling:** The scheduler supports a phase offset (0-360 degrees).
  - _Math:_ `phase_offset_ns = interval_ns * (degrees / 360)`.
  - _Logic:_ The scheduling grid is shifted by this offset relative to the second boundary.
  - _Status:_ Currently hardcoded to `0` degrees via `PING_PHASE_DEGREES` constant, but the math must remain implemented
    for future proofing.

### **5.2. Measurement**

- **Actual Time:** The backend captures `SystemTime::now()` **inside the future**, immediately before the socket call.
  This measured time is the authoritative `sent_time` reported in `PingEvent`. We do not trust the scheduled time.

---

## **6. Safety & Failure Modes**

### **6.1. The "Muffler" (Log Rate Limiting)**

To prevent log spam during system failures (e.g., "Network Unreachable"):

- The Backend maintains a `last_error_log: Instant`.
- When a `NetworkError` occurs:
  1. Check if `now > last_error_log + 5s`.
  2. **If True:** Log error via `error!()`, update timestamp.
  3. **If False:** Suppress log.
- **Note:** The `PingEvent` with `NetworkError` state is _always_ sent to MemDB, regardless of logging.

### **6.2. Backpressure**

- **Backend Overload:** If Scheduler -> Backend channel is full, Scheduler **drops** the batch and logs warning.
- **MemDB Unavailability:** If Scheduler -> MemDB channel is full, Scheduler **drops** the events and logs warning.
- **Rationale:** We prioritize process stability and "Obedience" (keeping the schedule running) over buffering data that
  cannot be consumed.

### **6.3. Data Hygiene**

- **No Sequence Numbers:** The `u16` ICMP sequence number is a private implementation detail. It must **not** be leaked
  in public structs.

---

## **7. Summary of Rejected Concepts**

The following are explicitly **banned**:

- ❌ **Adaptive Rate Control:** No backing off, no hysteresis.
- ❌ **SyncArbiter Backend:** Must be pure Tokio.
- ❌ **Public Sequence Numbers:** Removed from inter-component messages.
- ❌ **Configurable Timeout:** 10s hardcoded.
