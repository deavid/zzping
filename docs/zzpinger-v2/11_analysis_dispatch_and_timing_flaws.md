# **Title: `11_analysis_dispatch_and_timing_flaws.md`**

- **Author:** David Martínez Martí & Mr.Gemini
- **Date:** November 18, 2025
- **Status:** Final

---

## **1. Introduction**

This document serves as a detailed analysis of the `PingerSchedulerActor`'s message dispatch mechanism. It builds upon
previous findings to elaborate on two fundamental, intertwined flaws in how the scheduler sends work to the backend.
These are not minor inefficiencies but critical architectural bugs that make it impossible for the system to meet its
core design requirements for timing and precision.

## **2. Analysis of Dispatch Flaws**

### **Flaw 1: Non-Chronological Message Dispatch (A "Cardinal Sin")**

The most severe issue is that the scheduler sends `SchedulePings` commands to the backend in a non-chronological order.

- **Cause:** The scheduling loop in `handle_tick` is "target-centric." It iterates through each target and schedules all
  of its pings for the entire 10ms lookahead window before moving to the next target.
- **Example:** If Target A has slots at `T+1, T+2, T+3` and Target B has a slot at `T+1.5`, the message stream sent to
  the backend will be `[A, T+1]`, `[A, T+2]`, `[A, T+3]`, **`[B, T+1.5]`**.
- **Consequence:** This is a cardinal sin for the backend worker. The backend processes messages **sequentially** from
  the `mpsc` channel in FIFO order:

  ```rust
  Some(message) = work_rx.recv() => {
      let sleep_until = tokio::time::Instant::from_std(message.instant) + message.fire_duration;
      tokio::time::sleep_until(sleep_until).await;  // BLOCKING
      // ... spawn pings
  }
  ```

  Each message causes a **blocking** `sleep_until` before the next message can be processed. This creates **head-of-line
  blocking**: when the backend receives `[A, T+3ms]` followed by `[B, T+1ms]`, it will:

  1. Sleep until `T+3ms` and execute A's ping
  2. Then receive `[B, T+1ms]` (now in the past)
  3. Call `sleep_until(T+1ms)` which completes immediately but still yields to the async runtime, adding unpredictable
     latency

  The out-of-order messages make it impossible for the backend to execute pings at their scheduled times. Late-arriving
  earlier-scheduled pings cannot bypass messages currently being processed, fundamentally breaking the timing
  guarantees.

### **Flaw 2: Unbundled Commands and Implementation-Induced Drift**

The second flaw is the failure to bundle targets that share a time slot, which has consequences beyond mere
inefficiency.

- **Cause:** As a direct result of the target-centric loop, the scheduler sends one `SchedulePings` message per-target,
  per-slot, with `targets` always being a `vec!` of one.
- **Consequence 1: Introduction of Timing Drift.** This is a critical insight. The backend's **sequential message
  processing** combined with **sub-millisecond sleep imprecision** creates timing drift:

  **Scenario:** Two targets with close time slots (`T+1.0ms` and `T+1.5ms`) are sent as separate messages:

  1. Backend receives message 1, sleeps ~1ms, but actual sleep completes at `T+1.2ms` due to scheduler quantization
  2. Backend spawns ping 1, then processes message 2
  3. Backend attempts to sleep ~0.5ms, but `sleep_until` with durations under 1ms is inherently imprecise—the async
     runtime may:
     - Round up the sleep duration
     - Yield to other tasks even if the deadline is immediate
     - Context-switch, adding unpredictable latency
  4. Ping 2 fires at `T+2.0ms` instead of `T+1.5ms`

  **Why bundling eliminates relative drift:** If both targets shared a single message with `targets: vec![A, B]`, they
  would both spawn from the **same** `sleep_until` completion point. While absolute jitter from the scheduler's 1ms tick
  remains, the **relative** timing between targets in the same logical slot would be preserved (both fire within
  microseconds of each other), maintaining the constant-interval property required for data compression.

  The unbundled approach forces each target through a separate, imprecise sub-millisecond sleep, causing their actual
  send times to drift apart. This directly violates the non-negotiable design requirement of a perfectly regular,
  constant-interval ping stream.

- **Consequence 2: Violation of Backend Contract.** The design intends for the backend to receive a single command with
  all targets for a given slot. This allows it to perform one atomic `FuturesUnordered` batch operation. Sending
  unbundled commands prevents this, adding unnecessary channel traffic and complexity to the backend.

## **3. Conclusion**

The current dispatch mechanism in `PingerSchedulerActor` is architecturally unsound. The target-centric scheduling loop
makes it impossible to bundle commands correctly and, more critically, produces a non-chronological command stream that
breaks the backend's contract.

Furthermore, the unbundled nature of the commands forces the backend into a pattern of using imprecise, sub-millisecond
sleeps, which actively undermines the timing precision that the entire `zzpinger` component is designed to guarantee.

These flaws are not addressable with minor patches. They mandate a complete redesign of the `handle_tick` algorithm to
be **slot-centric**, which is a prerequisite for any further implementation.
