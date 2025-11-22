# ZZPing v0.3: Orchestration & Mastership

- **Status:** Authoritative / Active
- **Date:** November 2025
- **Dependency:** `03_Data_Pipeline.md`

---

## 1. The Core Problem

We need to solve three coordination challenges:

1. **Mastership:** Ensuring only one Collector instance is pinging from a specific location (Installation ID) at a time.
2. **Handoff:** Upgrading a Collector binary without leaving a data gap.
3. **Config:** Propagating target lists from the Database to the Collector.

## 2. Mastership: The Two Authorities

To prevent "Split-Brain" (two collectors pinging the same targets from the same location), we require agreement from two
independent authorities.

### Authority 1: The Local Lock (Tactical)

- **Component:** `TCPLock`.
- **Mechanism:** Binds a specific TCP port (defined in config).
- **Rule:** If you hold the port, you _can_ be Master. If you can't bind the port, you _must_ be Standby.
- **Purpose:** Prevents two processes on the _same machine_ from fighting.

### Authority 2: The Database (Strategic)

- **Component:** `CState` (Database side).
- **Mechanism:** Tracks active connections by `Installation ID`.
- **Rule:** The Database decides which connection is "Primary" and which is "Standby".
- **Purpose:** Orchestrates the Handoff (handoff logic lives here).

## 3. The Mastership State Machine

Every Collector exists in one of these states:

1. **STANDBY:**
   - Connected to DB.
   - `TCPLock` not held.
   - Pinger: **Stopped**.
2. **AWAITING_LOCK:**
   - DB commanded "Become Primary".
   - Trying to acquire `TCPLock`...
   - Pinger: **Stopped**.
3. **PRIMARY:**
   - DB commanded "Become Primary".
   - `TCPLock` held successfully.
   - Pinger: **Running**.
4. **ORPHAN_MASTER:** (Special Case)
   - DB Connection Lost.
   - `TCPLock` still held.
   - Pinger: **Running**. (We keep pinging during network outages).

NOTE: These are still subject to change, specially ORPHAN_MASTER might or might not be required to be its own state.
However, the intent is correct regardless of implementation specifics.

## 4. The Handoff Protocol (Zero-Downtime Upgrade)

How do we replace `Collector-Old` with `Collector-New` without missing a ping?

### Step 1: The Setup

- `Collector-Old` is **PRIMARY**.
- `Collector-New` starts up.
  - Tries to bind port -> Fails (Old has it).
  - Connects to DB as **STANDBY**.

### Step 2: Detection

- Database `CState` sees two connections with the same `Installation ID`.
- Database decides to upgrade.

### Step 3: The Swap (Pre-Scheduled)

- Database calculates a swap time `T = Now + 5s`.
- Sends `PrepareToSwap(T)` to both collectors.

### Step 4: Execution at time T

- **Collector-Old:**
  - Stops Pinging.
  - Releases `TCPLock`.
  - Enters **DRAINING** mode (flushing MemDB buffer).
- **Collector-New:**
  - Acquires `TCPLock`.
  - Starts Pinging.
  - Becomes **PRIMARY**.

NOTE: Adquiring lock before pinging in this scenario will be problematic and we might need some kind of exception to
allow the collector becoming primary to start pinging for a few seconds before adquiring the lock / or to have the old
one to release the lock early even if it keeps pinging. The reason is that freeing and requesting a TCP port might not
be in sync and cause problems, creating a gap.

### Step 5: Verification & Cleanup

- Database watches `Collector-New` for data.
- **Success:** `Collector-New` sends data. Database sends `Shutdown` to `Collector-Old`.
- **Failure:** `Collector-New` crashes/fails. Database sends `PromoteToPrimary` to `Collector-Old` (Rollback).

## 5. Configuration Propagation

### The "Global Intent" Model

We simplified the configuration model.

- **One Global Config:** The Database stores _one_ list of targets and _one_ ping rate.
- **No Sharding:** Every collector pings the entire target list.

### The Flow

1. **Admin:** Updates config via GUI -> Database.
2. **Database:** Persists to disk (`intent.ron`).
3. **Database `IntentConfig`:** Publishes `ConfigUpdate` event.
4. **Network:** Broadcasts to all connected Collectors via `intent-config` room.
5. **Collector `IntentConfig`:**
   - Updates in-memory state.
   - Notifies `Pinger` to update schedule.
   - **Caches to Disk:** Writes to `last_intent.ron`.

### The "Orphan Start" Capability

If a Collector starts up and cannot reach the Database:

1. It reads `last_intent.ron`.
2. It attempts to acquire `TCPLock`.
3. If successful, it enters **ORPHAN_MASTER** state and starts pinging immediately.
