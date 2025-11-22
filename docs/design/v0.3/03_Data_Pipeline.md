# ZZPing v0.3: Data Pipeline Architecture

- **Status:** Authoritative / Active
- **Date:** November 2025
- **Dependency:** `02_Component_Architecture.md`

---

## 1. The Unified Stream

ZZPing generates two types of data events:

1. **In-Flight Events:** "I just sent a ping to 8.8.8.8 (Seq 100)."
2. **Result Events:** "Seq 100 returned in 15ms" OR "Seq 100 timed out."

### The "Single Pipe" Decision

We treat these as a **single, unified stream** of `PingEvent` records.

- **No Priority Queues:** We do not prioritize Results over In-Flight messages.
- **No Filtering:** We do not drop In-Flight messages when the network is busy.
- **Why?** Simplicity. Managing split-brain buffers (where we have results for pings we "never sent" because the
  In-Flight packet was dropped) creates impossible reconciliation logic.

## 2. The Pipeline Flow

### Step 1: The Pinger (Source)

- **Action:** Executes scheduled pings.
- **Output:** Sends `PingEvent` to `MemDB` via local channel.
- **Backpressure:** If `MemDB`'s mailbox is full, **Pinger stops scheduling**. (The "Journal is Truth" axiom).

### Step 2: MemDB (Collector - Buffer)

- **Role:** The Shock Absorber.
- **Storage:** In-Memory Ring Buffer (e.g., last 1 hour of data).
- **Action:**
  - Stores events locally (for resilience).
  - Batches events (e.g., 100 events or 1 second).
  - Sends `SubmitBatch` to the Database via ZzNet.
- **Failure Mode:** If the Database is unreachable, `MemDB` keeps buffering until it hits the RAM limit. Then it drops
  the **oldest** data (Ring Buffer).

### Step 3: MemDB (Database - Ingester)

- **Role:** The Writer.
- **Action:**
  - Receives `SubmitBatch`.
  - **Immediate:** Appends to the "Hot" in-memory tier (for real-time queries).
  - **Delayed:** Batches data into 1-minute chunks for disk compression.
- **Persistence:** Data is written to disk in highly compressed chunks (custom format).

## 3. Resilience & Recovery

### A. The "Journal Gap" Problem

What happens if the Database restarts?

- The TCP connection breaks.
- The Collector `MemDB` enters "Disconnected Mode" and buffers data.
- When the Database returns, the Collector reconnects.
- **The Fix:** The Collector simply resumes sending batches. Because the Database is an append-only journal, it accepts
  the "late" data and appends it. Timestamps are absolute, so order of arrival doesn't matter for correctness.

### B. The "Restart" Problem (Collector)

What happens if the Collector restarts while the Database is down?

- The In-Memory buffer is lost.
- **Mitigation:** We accept this loss. We do _not_ persist the high-frequency buffer to disk on the Collector (e.g. to
  save SD card life on Pis).
- **Exception:** `IntentConfig` _is_ cached to disk, so the Collector knows _what_ to ping even if the DB is down.

## 4. Real-Time Monitoring

How do we see "live" pings in the GUI?

- The GUI connects to the Database as a `Client`.
- It joins the `memdb` room.
- The Database `MemDB` component acts as a **relay**.
- It forwards the "Unified Stream" (In-Flight + Results) to connected Clients.
- **Result:** The GUI sees the "In-Flight" event 15ms before the "Result" event, allowing it to animate a "Ping Sent..."
  visualization in real-time.

## 5. Summary of Guarantees

| Scenario              | Behavior                               | Data Loss?                          |
| :-------------------- | :------------------------------------- | :---------------------------------- |
| **Network Flap**      | Collector buffers in RAM.              | No.                                 |
| **Database Restart**  | Collector buffers in RAM.              | No.                                 |
| **Collector Restart** | RAM buffer is wiped.                   | **Yes** (Last ~1 hour).             |
| **Disk Full (DB)**    | DB rejects batches. Collector buffers. | Eventually Yes (Ring Buffer wraps). |
