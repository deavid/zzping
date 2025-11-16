# Old Codebase Review: `zzpinger`

This document provides a high-level review of the existing `zzpinger` codebase located at `src/components/zzpinger/src/`
and its `Cargo.toml`. The goal is to identify misalignments with the new, clarified vision outlined in
`docs/design/ZZPINGER_COMPONENT_DESIGN.md`.

## `Cargo.toml` Dependency Audit

An inspection of `src/components/zzpinger/Cargo.toml` reveals several dependencies that are in direct conflict with the
new design's principles.

**Misaligned Dependencies:**

- `zznet-api`
- `zznet-room`
- `zznet-router`
- `bincode`

**Analysis:**

The presence of `zznet-api`, `zznet-room`, and `zznet-router` is the most significant misalignment. The new design
explicitly states: **"Pinger does not use zznet tooling - it doesn't talk to other processes."** These dependencies
confirm that the old implementation is deeply integrated with the `zznet` ecosystem, which is a core aspect that must be
removed entirely. `bincode` is likely used for network serialization in this context and is also unnecessary.

**Correctly Aligned Dependencies:**

- `surge-ping`: This is the ICMP backend library, which is appropriate. The new design requires a backend to perform the
  pings.
- `zzmem-db`: This is the designated output for ping events, which is correct.
- `actix`, `tokio`, `serde`, `tracing`, `thiserror`, `futures`, `log`: These are foundational crates for an Actix-based
  component and are expected to be used in the new implementation as well.

**Conclusion:** The dependency list confirms a major architectural deviation from the new vision. The first step in the
rework will be to strip out all `zznet`-related dependencies.

## Source Code Structure Review

A review of the files in `src/components/zzpinger/src/` shows a structure that reflects the old, overly complex design.

**File-by-File Misalignment Analysis:**

- `network_actor.rs`, `network_manager.rs`, `network_messages.rs`: These files are the most obvious evidence of the
  unwanted `zznet` integration. They implement the logic for cross-process communication that is explicitly forbidden in
  the new design. **These files must be deleted.**

- `permissions.rs`: The new design has no concept of a "permissions" system. This adds unnecessary complexity and is a
  prime example of the feature creep the new vision aims to eliminate. **This file must be deleted.**

- `actor.rs`: This file likely contains a monolithic actor trying to handle scheduling, pinging, and network
  communication all at once. The new design mandates a strict separation of concerns into two distinct actors: a
  `PingerActor` for scheduling and a `PingerBackendActor` for execution. This file will need to be completely rewritten
  to become the new, focused `PingerActor` (scheduler).

- `pinger.rs`: This probably contains the core pinging logic, using `surge-ping`. In the new design, this logic must be
  extracted and moved into a separate `PingerBackendActor`. The current implementation is likely tightly coupled with
  the monolithic `actor.rs`.

- `api.rs`, `messages.rs`: These files probably define a wide range of messages, including those for the complex network
  interactions and other unnecessary features. The new API is drastically simpler, consisting only of
  `UpdateIntentConfig` and `UpdateCState` for input, and `PingEvent` for output. These files will need to be heavily
  simplified or rewritten from scratch to reflect the minimal interface.

## Summary: Where We Are vs. Where We Want To Be

- **Where We Are:** A complex, monolithic component deeply integrated with a networking layer (`zznet`) that it
  shouldn't be using. It has unnecessary features like permissions and likely mixes the concerns of scheduling,
  execution, and networking within a single actor.
- **Where We Want To Be:** A simple, highly specialized component focused exclusively on high-precision ping scheduling.
  It is completely decoupled from `zznet`. Its logic is split between a pure scheduler actor and a separate backend
  actor for execution, with minimal and clearly defined interfaces.

The path forward requires a "scorched earth" approach. The most direct path to alignment is to delete the misaligned
files (`network_*`, `permissions.rs`) and completely rewrite the core actor, message, and backend logic from scratch,
following the new design document to the letter.
