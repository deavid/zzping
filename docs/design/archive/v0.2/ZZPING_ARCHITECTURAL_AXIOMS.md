# **Title: `ZZPING_ARCHITECTURAL_AXIOMS.md`**

NOTE: Deprecated documentation.

**Status:** Immutable / Core Philosophy

**Context:** Required reading for understanding _why_ the system is built this way.

---

## **1. The Prime Directive: The Journal is Truth**

**"If it is not stored, it didn't happen."**

The sole purpose of `zzping` is to create a historical journal of network quality.

- **Traffic is a Side Effect:** We do not send pings because we want to generate traffic. We send pings only because it
  is currently the only way to measure the network.
- **Ghost Data is Forbidden:** Executing a network action (ping) without the ability to record the result creates "Ghost
  Data"—events that happened in reality but do not exist in the journal. This breaks the trust in the system.

**Implication:** If the database, disk, or internal reporting pipelines are clogged (backpressure) or dead, **we must
stop pinging.** We prioritize the integrity of the journal over the continuity of the traffic.

## **2. The Doctrine of Obedience**

**"The Pinger is Dumb. The User is Smart."**

We explicitly reject "Adaptive," "Smart," or "Good Neighbor" logic in the pinger.

- **The "Lie" of Adaptation:** If a user configures 30 pps, they expect 30 data points. If the network is dropping
  packets, and the pinger unilaterally decides to slow down to 1 pps to "be nice," it is corrupting the data. It is
  masking a "100% loss at 30Hz" event as a "1 sample at 1Hz" event.
- **Network Reality:** Modern network equipment is not impacted by 30–100 pps of ICMP. Designing complex back-off logic
  for defective hardware is solving a non-existent problem.
- **Compression:** Variable rates destroy delta-encoding efficiency. We prefer sudden jumps (Config Change) over sliding
  windows.

**Implication:** The scheduler executes the configured rate strictly. It does not back off during packet loss.

## **3. Failure Domains: System vs. Network**

We must distinguish between the thing we are measuring and the tool doing the measuring.

1. **Network Failure (Packet Loss, Timeout):**
   - **The Patient is sick.**
   - **Action:** Keep measuring. Record the failure. Do not stop.
2. **System Failure (Backpressure, Channel Closed):**
   - **The Doctor is having a heart attack.**
   - **Action:** Stop immediately.
   - _Backpressure:_ If `MemDB` is full, stop scheduling new pings (Pause).
   - _Dead Component:_ If the Backend or DB thread dies, Panic/Crash the process (Fail Fast).

## **4. The "Observer Effect" & Real-Time UX**

We accept "Double Traffic" (sending `InFlight` events before `Result` events) for the sake of User Experience.

- **The Void:** Without `InFlight`, a 10-second timeout looks like a broken UI. The user sees nothing.
- **The UX:** `InFlight` allows the GUI to infer potential packet loss _before_ the timeout occurs (e.g., turning a
  status light yellow at 120ms).
- **The Trade-off:** We accept 2x messaging overhead to gain Real-Time observability. Premature optimization (batching
  InFlight) is rejected until proven necessary.

## **5. Engineering Values**

- **Pragmatism > Purity:** We do not add architectural layers just for "elegance." If the Backend is 1:1 with the
  Scheduler, passing an `Addr` is acceptable if it avoids a complex channel shim. We do not "split hairs" over
  theoretical decoupling if it adds runtime complexity.
- **Simplicity > Granularity:** We do not need 1000 pps. We do not need nanosecond precision on timeouts.
- **Fatalism:** If a component enters an undefined state (e.g., Backend thread panic), the correct response is usually
  to crash the application (`std::process::exit`) and let the OS supervisor restart it clean. We do not try to "heal"
  corrupted process state.

---
