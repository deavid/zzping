# ZZPing v0.3: Architectural Standards & Development Constraints

- **Status:** Authoritative / Immutable
- **Date:** November 2025
- **Context:** This document defines the rigid constraints that govern _how_ we write code in this repository. These are
  not suggestions; they are requirements for the system's verifiability and safety.

---

## 1. The Principle of Hermetic Verification

**"The repository is self-contained. The build is the truth."**

The system must be fully verifiable using standard build commands (`cargo test`, `cargo check`) without requiring
external environment setup, containers, or manual orchestration.

### The Constraints

1. **Single Command Verification:** A developer must be able to clone the repo and run `cargo test` to verify the entire
   distributed system. If a test requires a script to spin up a binary in the background, it is rejected.
2. **Sandboxed Execution:** Tests must **never** touch the host system's resources (network interfaces, real `/etc`
   config, production databases).
3. **Destructive Safety:** Test code must assume it is running with high privileges and prevent itself from causing
   damage. We do not write code that _could_ wipe a hard drive if a config path is wrong; we write code that physically
   _cannot_ access the hard drive in the test environment.

## 2. The Simulation Strategy (Deterministic Time)

**"We do not wait for the clock. We control it."**

Distributed systems are notoriously flaky because of timing. We eliminate this by decoupling logic from Wall Clock Time.

### The Constraints

1. **Zero `std::thread::sleep`:** Using thread sleep in tests is forbidden. It makes tests slow and flaky.
2. **Virtual Time:** All components must accept a `Clock` abstraction or run within a runtime that supports time
   freezing (e.g., `tokio::time::pause()`).
3. **Deterministic Choreography:** Integration tests must be scripted narratives (Act I, Act II, Act III). We do not
   "hope" a race condition resolves; we advance the virtual clock by exact milliseconds to force specific states.

## 3. Component Symmetry & Context Isolation

**"Components are Islands. The Network is the Ocean."**

To manage complexity, we refuse to let components "know" about the internals of other components.

### The Constraints

1. **The "Room" Model:** A component (e.g., `MemDB`) never talks directly to another component's internal state. It
   talks to a named "Room" (a typed message channel).
2. **Protocol Symmetry:** The code running on the Collector and the Database is the same code (same Actor), just
   configured differently. We do not write asymmetric "Client" and "Server" structs for business logic.
3. **Local Reasoning:** A developer should be able to understand, modify, and test a component (e.g., `zzpinger`)
   looking _only_ at that component's directory. Dependencies on global state or cross-crate internals are forbidden.

## 4. I/O Isolation (The Actor Firewall)

**"Business Logic does not do I/O. Actors do I/O."**

To ensure Hermeticity (Standard #1), business logic must be decoupled from physical side effects.

### The Constraints

1. **No Direct File Access:** Components like `MemDB` or `Pinger` must not open files or sockets directly.
2. **The Storage Actor Pattern:** Persistence is handled by a dedicated `StorageActor`.
   - **Production:** The actor wraps `std::fs` or `tokio::fs`.
   - **Testing:** The actor is configured in "Ephemeral Mode," keeping data in heap memory.
3. **No Traits/Mutex Soup:** We do not use `Arc<Mutex<Box<dyn Storage>>>` to solve this. We use the Actor Model. The
   `MemDB` actor sends a `Store` message to a `Storage` actor. In tests, the `Storage` actor just doesn't write to disk.

## 5. Coverage as Dead Code Detection

**"If it isn't exercised by the Simulation, it doesn't exist."**

We rely on coverage reports (`cargo llvm-cov`) not to chase high numbers, but to detect logic that is unreachable in
reality.

### The Constraints

1. **Narrative Testing:** We prefer high-fidelity integration scenarios (`zznet-demo`, `zzping-integration-test`) over
   mocking individual functions.
2. **The Audit:** If a line of code is not hit by the full system simulation, it indicates a flaw in the simulation or
   dead code in the implementation. Both must be fixed.

## Summary

This architecture is optimized for **Correctness** and **Refactorability**. By adhering to strict isolation and
determinism, we allow the system to be aggressively restructured without fear of subtle regression.
