# **Title: `ZZPINGER_COMPONENT_DESIGN_ADDENDUM_1.md`**

- **Author:** David Martínez Martí
- **Date:** November 17, 2025
- **Status:** Final
- **Amends:** `docs/design/ZZPINGER_COMPONENT_DESIGN.md`

---

## **1. Introduction & Motivation**

### **1.1. Purpose**

This document serves as a formal addendum to the canonical design specification for the `zzpinger` component. It
provides critical design revisions and clarifications based on a detailed analysis of a previous, flawed implementation
attempt. The changes outlined herein are mandatory and supersede any conflicting descriptions in the original design
document.

### **1.2. Summary of Changes**

This addendum introduces two major, non-negotiable design changes:

1. **A Rearchitected `zzpinger` Backend:** The backend's implementation model is completely revised to resolve critical
   performance, concurrency, and architectural issues identified in the initial implementation. The `SyncArbiter` actor
   model is replaced with a more appropriate and efficient pure-Tokio task model.
2. **A Scheduler Adaptive Interval Mechanism:** A new, stateful rate-control mechanism is introduced into the
   `PingerSchedulerActor`. This mechanism provides intelligent network backoff to avoid flooding degraded targets, while
   carefully preserving the constant-interval nature of the ping stream to ensure high data compression efficiency.

### **1.3. Goals**

The primary goals of these revisions are to:

- Guarantee high-precision, concurrent ping execution by aligning the implementation model with the asynchronous nature
  of the underlying libraries.
- Eliminate catastrophic performance bugs related to resource creation and management.
- Introduce a network-friendly backoff and recovery policy.
- Ensure that the ping stream remains perfectly regular at all times to satisfy the strict requirements of the
  downstream data compression algorithm.

---

## **2. Part 1: Rearchitected `zzpinger` Backend**

### **2.1. Motivation**

Analysis of the previous implementation revealed that the chosen `SyncArbiter` actor model for the backend was
fundamentally mismatched with the task at hand. This architectural mismatch led to severe issues:

- **The "Async-in-Sync" Problem:** The `SyncContext` of an Actix actor is a synchronous environment. Forcing an
  `async`-native library like `surge-ping` to run within it required awkward and inefficient workarounds.
- **Catastrophic Performance:** The implemented workaround involved creating a new Tokio runtime for every single ping.
  This is a severe anti-pattern that leads to massive thread churn and resource exhaustion, rendering the component
  unusable at any meaningful scale.
- **Sequential Pinging Bug:** The implementation failed to execute pings concurrently, instead pinging targets in a
  batch one by one. This completely violates the design's requirement for parallel execution and would introduce
  unacceptable artificial latency between pings within the same time slot.

The following design replaces this flawed model with one that is architecturally sound and performant.

### **2.2. New Architecture**

- **Model:** The backend **must** be implemented as a dedicated OS thread hosting its own self-contained Tokio runtime.
  It is **not an Actix actor.** It is a pure, long-running `async` function that is started once and communicates with
  the Actix-based scheduler via standard Tokio channels.

- **Concurrency:** The backend **must** use a `FuturesUnordered` collection to manage all in-flight ping operations.
  When a command to ping multiple targets is received, the backend must create a `Future` for each ping and push all of
  them into the `FuturesUnordered` collection. This is the mandatory pattern to ensure all pings for a given time slot
  are initiated concurrently and their results are processed as they complete, preventing head-of-line blocking.

- **Principle:** The backend remains a "Dumb Worker." It is completely stateless regarding application-level
  configuration like target lists or ping rates. Its only job is to execute the commands it receives as precisely as
  possible and report the results.

### **2.3. Communication Interface**

The backend **must** communicate with the `PingerSchedulerActor` exclusively through the following Tokio channels, which
are provided to it upon creation:

- **Inputs:**

  1. **Work Channel (`mpsc::Receiver<SchedulePings>`):** The primary input for receiving `SchedulePings` commands from
     the scheduler.
  2. **State Channel (`watch::Receiver<bool>`):** An instantaneous "kill switch" channel. The backend must subscribe to
     this channel to receive boolean state updates (`true` for enabled, `false` for disabled).

- **Outputs:**
  1. **Event Channel (`mpsc::Sender<PingEvent>`):** The sole output channel for sending all `PingEvent` outcomes (e.g.,
     `ReceivedRTT`, `TimedOut`) back to the scheduler.

### **2.4. Core Logic and Data Flow**

The backend's core logic **must** be implemented as a single `tokio::select!` loop that concurrently waits on two
sources: new messages from the work channel and the completion of any in-flight ping future.

The sequence for handling a `SchedulePings` message is as follows:

1. **Check State:** Before any waiting, the backend must synchronously check the latest value from the `watch` channel.
   If the state is `disabled` (`false`), the `SchedulePings` message **must** be dropped immediately, and no further
   action is taken for that message.
2. **High-Precision Wait:** If enabled, the backend then `await`s an asynchronous sleep until the exact moment specified
   by the message's `instant + fire_duration`.
3. **Initiate Pings Concurrently:** Immediately after the wait completes, the backend iterates through all targets in
   the message. For each target, it creates an `async` block or function that constitutes the `ping_future`. This future
   is then pushed into the `FuturesUnordered` collection, which begins its execution.

The most critical requirement is **Timing Measurement:** The _actual_ `SystemTime` of the ping send **must** be captured
from _inside_ the `ping_future`, as close as possible to the underlying network socket call. This measured time, not the
scheduled time, is what must be reported in the final `PingEvent`.

When a `ping_future` completes, the `select!` loop will wake, the result will be processed, a `PingEvent` will be
constructed with the accurate send time, and it will be sent to the output event channel.

### **2.5. Resource and Sequence Management**

- **Client Management:** The `surge_ping::Client` object **must** be created only **once** when the backend thread is
  initialized and must be reused for the lifetime of the process.
- **Pinger Management:** The `surge_ping::Pinger` object is a lightweight handle. A new one should be created on-demand
  for each ping. Caching these objects is unnecessary complexity and is forbidden.
- **ICMP Sequence Number:** The `u16` ICMP sequence number is an internal implementation detail of the backend. The
  backend **must** manage this state, likely in a `HashMap<IpAddr, u16>`, incrementing it for each ping to a given
  target and handling its own `wrapping_add` logic. This sequence number is for matching ICMP echo replies and is
  distinct from any scheduler-level sequence numbers.

---

## **3. Part 2: Scheduler Adaptive Interval Mechanism**

### **3.1. Motivation**

A static ping rate, while simple, is not robust. It can overwhelm a struggling network target, generating excessive
error traffic and potentially contributing to network congestion. This mechanism introduces a feedback loop into the
scheduler, allowing it to intelligently reduce the ping rate for degraded targets.

Crucially, this is achieved by adjusting the **ping interval itself**, not by randomly skipping scheduled pings. This
preserves the perfectly regular, constant-interval nature of the ping stream, which is a non-negotiable requirement for
the downstream data compression algorithms to function efficiently.

### **3.2. Core Design Principles**

The implementation of this mechanism **must** adhere to the following principles:

1. **No Initial Ramp-Up:** When a new target is introduced (either at startup or via a configuration update), the
   scheduler **must** immediately begin pinging it at the full rate specified by `IntentConfig`. A slow ramp-up would
   complicate the mastership handoff protocol and delay data acquisition. The "Fast Back-off" principle is sufficient to
   handle cases where the new target is unresponsive.

2. **Discrete Rate Tiers:** The adaptive rate **must not** be continuously variable. The scheduler must operate in a
   small, predefined set of fixed-rate "gears." This ensures that, for any given period, the interval between sent pings
   remains perfectly constant.

   - _Example Tiers:_ `[2 pps, 4 pps, 8 pps, 16 pps, unlimited]`

3. **Asymmetric Adaptation (Hysteresis):** The logic for switching between tiers must be asymmetric to prevent rapid,
   wasteful oscillations between high and low rates.
   - **Fast Back-off:** The decision to _decrease_ the ping rate (move to a lower gear) **must** be based on a short,
     recent window of performance data. The system must be highly responsive to sudden network degradation.
   - **Slow Recovery:** The decision to _increase_ the ping rate (move to a higher gear) **must** require a longer,
     sustained period of proven network stability.

### **3.3. State Management**

- The scheduler **must** maintain a small, bounded, sliding window of the most recent ping outcomes for **each target
  individually**. A `VecDeque<bool>` (representing success/failure) of a fixed size (e.g., 100-200 entries) is the
  recommended state.
- The scheduler must also store the current rate tier for each target.
- This per-target state is minimal and can be either transferred during a mastership handoff or safely rebuilt by a new
  instance, which will gracefully self-correct its rate within a few seconds of operation.

### **3.4. Implementation Guidance**

The implementer is expected to deliver the simplest possible code that satisfies the above principles. A recommended,
but not mandatory, approach is as follows:

- **State:** For each target, store a `VecDeque` of the last `N` outcomes and the current rate tier.
- **Back-off Logic:** After processing a `PingEvent`, update the target's sliding window. Recalculate the success rate
  (e.g., successes / `N`). If this rate drops below a threshold for the _current_ tier, immediately move down to the
  next appropriate, lower-rate tier.
- **Recovery Logic:** To implement "slow up," require a sustained period of high performance. This can be achieved by
  tracking the number of _consecutive good windows_. For example, to move from the 8pps tier to the 16pps tier, the
  scheduler might require seeing 3 consecutive windows of 100 pings each where the success rate is consistently above a
  high-water mark (e.g., 98%). A single intermittent failure should reset this recovery counter, preventing a premature
  return to a high ping rate.

The specific window sizes, success/failure thresholds, and consecutive window counts for recovery should be implemented
as easily tunable internal constants.
