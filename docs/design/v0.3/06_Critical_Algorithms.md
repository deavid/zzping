# ZZPing v0.3: Critical Algorithms & Open Problems

- **Status:** Draft / Analysis
- **Date:** November 2025
- **Context:** This document captures algorithmic requirements from previous iterations that must be solved within the
  new ZzNet architecture. It defines the _problems_ clearly, while treating the _solutions_ as candidates to be verified
  during implementation.

---

## 1. Time Monotonicity (The Timestamp Problem)

### The Problem

We need absolute timestamps (UTC) for data correlation, but `SystemTime` is not monotonic (it jumps during NTP updates).
`Instant` is monotonic but relative (uptime).

- **Risk:** If `SystemTime` jumps backwards by 5 seconds, we might record a ping as happening _before_ the previous one,
  corrupting the time-series database and breaking delta-encoding compression.

### Constraint

The Pinger currently aligns execution to `SystemTime` ticks (e.g., exactly at `.000`).

### Candidate Solution: The Anchor Pair

Maintain a reference pair: `(AnchorSysTime, AnchorInstant)`.

- **Calculation:** `EventTime = AnchorSysTime + (Instant::now() - AnchorInstant)`.
- **Reset:** Periodically reset the Anchor to slowly drift towards `SystemTime` without steps.
- **Status:** Verify if `zzpinger` implementation currently handles this or if it trusts `SystemTime` blindly.

## 2. Data Consistency & The "Cursor" (The Gap Problem)

### The Problem

The Database writes to disk in an append-only, highly compressed format. Once a minute-chunk is written, it is
immutable.

- **Risk:** If a Collector reconnects after a network outage, it might send data that fills a gap _already written_ to
  disk as "empty". Since we can't rewrite the past, that data is lost or must be discarded.

### The ZzNet Opportunity

The `MemDB` NetworkActor (on the Database) is spawned fresh on every connection.

- **Startup:** On creation, the NetworkActor can query the Storage Engine: "What is the last timestamp you have
  persisted for this Collector?"
- **Handshake:** It sends this `LastPersistedTimestamp` to the Collector in the initial `MemDB` protocol handshake
  (before streaming starts).
- **Filtering:** The Collector `MemDB` rewinds its buffer to that timestamp and resumes sending.

### Constraint

We must ensure the Database doesn't write "Gaps" to disk prematurely.

- **Candidate Strategy:** The Database `MemDB` buffer ("Hot Tier") must be large enough (e.g., 2-4 hours) to hold data
  in memory _before_ compression, allowing late-arriving data to fill gaps before the immutable write happens.

## 3. Buffer Management (The Pruning Problem)

### The Problem

If the Database is unreachable for days, the Collector's memory will fill up. We must drop data gracefully.

### The Simplification Doctrine

We reject complex "fsync-aware" pruning in favor of predictable resource usage.

### The Policy

**Capacity-Based Ring Buffer.**

- **Metric:** Message Count (not bytes, not time).
- **Limit:** Configurable (e.g., `3,600,000` records ~ 1 hour @ 1k pps).
- **Behavior:** When full, new messages overwrite the oldest messages.
- **Rationale:** Predictable RAM usage. In a catastrophic outage (>1 hour), preserving the _newest_ data is more
  valuable than preserving the _oldest_.

## 4. Data Compression (The 6-Bit Goal)

### The Asset

The legacy codebase (`src/old`) contains a highly efficient compression algorithm capable of storing ping results in ~6
bits per ping.

### The Migration

This algorithm must be ported to the new `zzmem-db` component (Database side).

- **Input:** Sorted vector of `PingResult`s (from the "Hot Tier" buffer).
- **Output:** Compressed Blob.
- **Timing:** This happens asynchronously (e.g., once a minute) on the Database side. It is decoupled from the network
  receiving path.
