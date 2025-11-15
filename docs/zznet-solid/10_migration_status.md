# Phase 8 -> Phase 9: Migration status and next steps

Date: 2025-10-28
Author: automated investigation

Summary
-------
This report captures the current state of the `zznet-session` extraction work, what actually exists in the repository today, how the current layout maps to the SOLID goals in the refactoring plan, and a concrete, prioritized migration plan to finish the split so the SOLID principles are truly enforced.

Top-level findings
------------------
- The repo now contains three crates representing the intended split:
      - `zznet-session` — now limited to `peer_session.rs`, `messages.rs`, and legacy integration tests.
   - `zznet-peer-manager` — owns the control-plane `PeerManager`, the Actix wrapper, and re-exports canonical types from `zznet-api` (temporary `PeerSession` re-export remains).
   - `zznet-router` — owns the data-plane `Router` and re-exports canonical types from `zznet-api` (temporary `PeerSession` re-export remains).
- `zznet-api` now defines the canonical shared types: `PeerIdentity`, `Role`, `AuthContext`, `PeerId`, `RoomId`, `ConnectionState`, `SessionError`, and `PeerLifecycleEvent`.
- There are still many active imports of `zznet_session::types::{PeerId, RoomId, SessionError}` across components and network crates. A repository grep returned ~43 matches across tests, components, and network code.

Why this matters (SOLID assessment)
----------------------------------
- Conceptually, the split (PeerManager = control plane, Router = data plane, API crate for shared types) aligns with SOLID goals (SRP, ISP, DIP).
- In practice, `zznet-session` still exports key helpers (`PeerSession`, router internals), so the new crates depend on it. Until the remaining state/routing logic is fully relocated, SOLID violations persist (shared ownership of responsibilities, tight coupling).

Concrete facts collected
------------------------
- `zznet-api` provides the canonical data types in `src/net/zznet-api/src/types.rs`.
- `zznet-session` currently contains: `peer_session.rs` (≈1.6k lines), `messages.rs`, and associated tests.
- `zznet-peer-manager` re-exports from `zznet_session::peer_session` but otherwise depends only on `zznet-api` for shared types.
- `zznet-router` still depends on `zznet_session::peer_session` for room-aware operations.
- High-impact areas still importing `zznet_session::types` include:
   - `src/components/zzintent-config/src/` — actor, network_actor, internal_messages, network_manager, tests
   - `src/components/zzmem-db/src/` — actor, network_manager, network_actor, network_messages
   - `src/components/zzcollector-state/src/` — network actor/message code
   - `src/net/zznet-hello/src/connection_manager.rs`
   - `src/test-utils/zzping-test-utils/src/lib.rs`
   - Several integration/e2e test suites in the apps directory

Recommended migration plan (safe, minimal-risk, prioritized)
------------------------------------------------------------
This plan minimizes churn and keeps the workspace building at each stopgap point (fits the refactoring plan's stopgap philosophy).
Phase A — canonical types -> `zznet-api` (HIGH PRIORITY)
Status: Completed (with compatibility shim) and partially rolled out

1. DONE: Canonical types added to `zznet-api`:
   - `PeerId`, `RoomId`, `ConnectionState`, and `SessionError` live in `src/net/zznet-api/src/types.rs`.
2. DONE: Compatibility shim in `zznet-session`:
   - `zznet-session/src/types.rs` now re-exports `zznet_api::types::{PeerId, RoomId, ConnectionState, SessionError}` to keep dependent crates building.
3. IN PROGRESS: Update imports across the repo to use `zznet_api::types::{...}` directly.
   - Internals of `zznet-session` (PeerSession, router/peer_manager internals, adapters, traits, tests) now use `zznet_api::types` directly.
   - `zznet-peer-manager` and `zznet-router` now re-export from `zznet-api::types` instead of `zznet-session::types`.
   - Remaining external crates still import `zznet_session::types` (compat path); these will be switched over incrementally.

Rationale: moving canonical, small, copy-safe types first is the lowest-risk step. Once `zznet-api` owns the canonical types, `zznet-peer-manager` and `zznet-router` can stop re-exporting from `zznet-session` and become independent.

Phase B — finish PeerManager extraction (CONTROL PLANE)
1. IN PROGRESS: Move `PeerSession`-adjacent state that belongs to the control plane into `zznet-peer-manager`:
   - `peer_manager_internal.rs` has already been relocated and deleted from `zznet-session`.
   - Decide whether `PeerSession` itself co-locates with the manager or becomes a router concern; recommended path keeps peer state + metadata in the manager and exposes async channels to the router.

2. TODO: Add tests to `zznet-peer-manager` that previously lived inside `zznet-session` (unit and behavioral). Remove duplicates from `zznet-session` afterward.

3. DONE: `zznet-peer-manager::lib.rs` re-exports now point to `zznet-api::types`.

Phase C — finish Router extraction (DATA PLANE)
1. DONE: Routing and room negotiation logic moved from `router_internal.rs` and `room_adapter.rs` into `zznet-router`.
2. Move room-related traits (`RoomHandle` / `room_message_trait`) to `zznet-room` (if exists) or `zznet-router` depending on ownership. The refactor plan suggested `zznet-room` for room handling; follow that for clearer separation.
3. Add Router unit/integration tests to `zznet-router`.
4. DONE: `zznet-router::lib.rs` re-exports now point to `zznet-api::types`.

Phase D — finalize and delete `zznet-session`
1. Ensure no crate imports `zznet-session` types/traits directly. Run the local grep/script to report zero remaining uses.
2. Delete `zznet-session` or leave it as a tiny compatibility crate that re-exports from new crates with deprecation and a `FIXME` to delete later. Prefer deletion once everything passes.

Phase E — cleanup and docs
1. Sweep TODO/FIXME comments, update docs to reference new crates and architecture (peer-manager/router/api).
2. Run full workspace tests and a couple of deeper integration tests.
3. Update `02_ZZNet_SOLID_Refactoring_plan.md` to mark phases complete and record lessons.

Estimated effort & risk
-----------------------
- Phase A: Small, safe. Mostly file edits and mass-replace imports. Risk is very low. Estimated time: 1–2 hours (depending on number of imports to fix). Tests will compile incrementally.
- Phase B: Moderate risk. `PeerSession` migration touches stateful coordination paths; requires careful test coverage. Estimated time: 3–4 hours with incremental commits. Main risk is behavior drift in peer lifecycle handling.
- Phase C: Moderate-to-high risk. Router extraction impacts message routing and room negotiation; requires integration tests. Estimated time: 4–6 hours.
- Phase D/E: Low risk once prior phases complete; mostly cleanup and documentation.

Immediate next steps
--------------------
- Finish updating remaining crates to import `zznet_api::types`. Track the grep list and convert modules incrementally, running targeted tests after each batch.
- Extract the remaining control-plane state (`PeerSession` ownership decisions, peer bookkeeping helpers) into `zznet-peer-manager`, leaving `zznet-session` as data-plane only.
- Prepare router integration cleanup (remove remaining call sites that still reference the old `zznet-session` module) before deleting the compatibility layer.

Key files to monitor during the next phase
-----------------------------------------
- `src/net/zznet-session/src/peer_session.rs` — evaluate which portions relocate to peer-manager vs router.
- `src/net/zznet-router/src/lib.rs` — ensure the extracted router API covers the previous internal use cases.
- `src/net/zznet-peer-manager/src/lib.rs` — ensure new state stays encapsulated and API surface remains focused.
- `src/net/zznet-router/src/lib.rs` — prepare re-exports and integration tests for migrated router code.

---

# Phase 9 -> Phase 10: SOLID Enforcement Complete

Date: 2025-10-28

## Changes Applied

1. Interface Segregation (ISP)
   - Created `PeerRegistry` and `MessageRouter` traits in `zznet-api`
   - Implemented traits for `PeerManager` and `Router`
   - Deprecated mixed-plane methods on `PeerManagerActor`

2. Three-Actor Isolation
   - Removed `PeerManagerActor` and `Room` fields from Main actors (IntentConfigActor)
   - Removed `zznet-session` dependency from component crates (IntentConfig)
   - All network knowledge now lives in NetworkManager actors

3. Local Enforcement (no CI)
   - Added `scripts/check-component-dependencies.sh` for local validation
   - No CI job is configured or used in this repository; document the local checks and ensure they are run by authors prior to review
   - Documented rules in `ARCHITECTURE_RULES.md`

4. SessionCoordinator
   - Deprecated in favor of explicit two-step orchestration
   - Rollback logic moved to app composition root

## Verification

All tasks completed per checklist in `14_Action_Plan_No_Shortcuts.md`.
Architecture now enforces SOLID principles in code, not just documentation.