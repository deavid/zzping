# ZZNet SOLID Refactor — Skeptical Audit of Staged Changes (Code-First)

Date: 2025-10-28
Status: Advisory, pre-merge audit
Author: Independent reviewer (code-first)
Scope: Audit only the currently staged changes against the SOLID/Three-Actor goals and the "Second Opinion" (13_SOLID_Second_Opinion.md).

---

## 1) Executive summary

The staged changes move the architecture in the right direction:
- Narrow traits added in `zznet-api` (`PeerRegistry`, `MessageRouter`) and implemented in the right crates.
- Components stop depending on the façade/infra crates; Main actors are cleaner and more network-oblivious.
- The old façade (`zznet-session`) is explicitly deprecated where it crosses planes; rules and a simple enforcement script were added.

However, two correctness risks look blocking before merge:
- A single source of truth for peer lifecycle isn’t maintained in apps: Connection paths create a new `PeerManagerActor` while components read from a different `PeerManager` instance.
- Router channel registration after HELLO seems missing or unverified (channels removed from `AddPeer` without a visible replacement). If not wired, routing will silently fail at runtime.

Recommendation: Fix these two items and wire the CI check. Then merge the rest.

---

## 2) What changed (staged)

- Interface segregation (ISP):
  - `zznet-api`: Added `traits.rs` defining `PeerRegistry` (control plane) and `MessageRouter` (data plane).
  - `zznet-peer-manager`: Implements `PeerRegistry` for the control plane.
  - `zznet-router`: Implements `MessageRouter` for data-plane operations (send/broadcast, peer sender, inbound subscription).
- Components to trait boundary:
  - `zzintent-config`, `zzmem-db`, `zzcollector-state`: Cargo.toml stop depending on `zznet-session`/`zznet-router` (in main crates). Builders and NetworkManagers now take `Arc<dyn PeerRegistry>` and `Arc<dyn MessageRouter>`.
  - `IntentConfigActor` dropped `PeerManagerActor`, `Room<...>`, and direct channel fields; Three-Actor isolation improved.
- Façade taming:
  - `zznet-session::PeerManagerActor`: Cross-plane methods (`GetPeerSender`, `SubscribePeerInbound`) marked deprecated. `SessionCoordinator` is deprecated.
- App crates (composition roots):
  - `zzping-collector`, `zzping-database`: now depend directly on `zznet-peer-manager` and `zznet-router` (allowed at the app level).
- Enforcement and rules:
  - `docs/zznet-solid/ARCHITECTURE_RULES.md` added (Main actors network-oblivious, ISP split, router scope).
  - `scripts/check-component-dependencies.sh` added to forbid infra deps in Main-actor crates.

---

## 3) Alignment with 13_SOLID_Second_Opinion (DoD mapping)

- ISP split (10.1):
  - Traits added and implemented — PASS.
  - Façade methods deprecated — PARTIAL (done in `zznet-session`; in `zznet-peer-manager` actor, mixed-plane messages now return `None` with a warning but aren’t marked deprecated) — FOLLOW-UP.
  - One pilot component migrated — PASS (IntentConfig and MemDB moved to trait boundary).
- Three-Actor isolation (10.2):
  - Main actors dropped infra deps and network types — PASS (in staged components).
  - NetworkManagers own the traits — PASS.
  - CI enforcement — PARTIAL (script added; not wired to CI yet).
  - Tests with mocks — MISSING (no mock trait impls provided in the staged set).
- Coordinator (10.3):
  - `SessionCoordinator` deprecated — PASS.
  - Orchestration moved to composition root — MISSING (no explicit two-step add/remove wiring using PeerManager + Router).
- Router scope (10.4):
  - Implementation respects scope; explicit module docs not added — PARTIAL.
- Testability (10.5):
  - Mocks for traits and component unit tests without Actix — MISSING.
- CI enforcement (10.6):
  - Dependency checker script added — PASS (needs CI wiring).
  - Grep for deprecated façade usage — MISSING.

---

## 4) Risks and suspected gaps (blockers highlighted)

1) BLOCKER: Split peer lifecycle state across two managers in apps
- In `zzping-collector` and `zzping-database`, the service creates an `Arc<PeerManager>` for components but network code creates a new `PeerManagerActor` for `ConnectionManager`.
- Consequence: Components subscribe to a different lifecycle bus than the one `ConnectionManager` updates. Events and queries diverge, causing non-deterministic behavior and missing actors.
- Required: Use a single source of truth for peer lifecycle per app. Either:
  - Build one `Arc<PeerManager>` in the app, then spawn a `PeerManagerActor` that wraps this same instance for `ConnectionManager`, or
  - Refactor `ConnectionManager` to accept the underlying `PeerManager`/`PeerRegistry` directly.

2) BLOCKER: Router peer-channel registration after HELLO
- Previously, `ConnectionManager` sent `AddPeer { peer_state, peer_channels }`. Now it sends only `AddPeer { peer_state }` (channels removed from message).
- I don’t see a replacement path that calls `Router.register_peer(peer_channels)` after handshake, nor matching remove/disconnect hooks.
- If not wired, `MessageRouter::peer_sender/subscribe_peer_inbound` will return `None`, and routing/broadcast will fail silently.
- Required: Ensure the handshake pipeline explicitly registers/unregisters channels in `Router` (composition root or a small adapter). Verify with tests.

3) Mixed-plane message stubs in `zznet-peer-manager::PeerManagerActor`
- `GetPeerSender` and `SubscribePeerInbound` now always return `None` and warn, but are not marked `#[deprecated]` here.
- Risk: Silent runtime regressions where legacy callers still expect real channels.
- Required: Mark as `#[deprecated]` and add an automated grep/deny check for non-test code.

4) CI enforcement is not wired
- The new `scripts/check-component-dependencies.sh` is not yet part of CI jobs.
- Required: Add a CI step to run it on PRs targeting `main`/`dev`.

5) Router scope documentation (code-level) missing
- The rules doc lays out the scope, but module-level docs in `zznet-router` would help enforce the boundary.
- Required: Add module-level docs summarizing allowed vs. not-allowed responsibilities and a quick static import check in CI (grep for `Role`/`PeerIdentity`).

6) Trait impl naming hazards
- In `zznet-peer-manager`, the `PeerRegistry` impl methods call similarly named inherent methods (e.g., `get_peer_role`). It likely resolves to the inherent methods, but it’s brittle.
- Suggested: Use fully qualified calls (`PeerManager::get_peer_role(self, ...)`) or rename inherent methods to avoid accidental recursion in future refactors.

7) Testability at boundaries
- No mock implementations provided for `PeerRegistry`/`MessageRouter`, and no component unit tests using them.
- Suggested: Provide lightweight mocks (in `zznet-api` under a dev feature or in `zzping-test-utils`) and convert one component test to use them.

---

## 5) Minimal changes required before merge

- Unify peer lifecycle:
  - Create one `Arc<PeerManager>` per app and use it everywhere. If an Actix address is required, spawn a `PeerManagerActor` that delegates to the same manager.
- Wire router registration explicitly:
  - On handshake completion, register `PeerChannels` with `Router.register_peer(...)`. On removal/disconnect, call `remove_peer`/`disconnect_peer` accordingly. Keep this in the composition root or an app-local adapter (preferred over a central coordinator).
- Harden deprecations and CI:
  - Mark mixed-plane messages in `zznet-peer-manager::PeerManagerActor` as `#[deprecated]`.
  - Add a CI grep step to fail on use of deprecated façade messages in non-test code.
  - Add the dependency check script to CI.
- Add `zznet-router` module-level docs for scope; optional CI grep to prevent importing `Role`/`PeerIdentity`.

---

## 6) Nice-to-have follow-ups (non-blocking)

- Provide mocks for `PeerRegistry` and `MessageRouter`, with an example test for a component using them (no Actix system).
- Replace ambiguous method calls in the `PeerRegistry` impl with fully-qualified names.
- Add an example "composition root" snippet demonstrating two-step registration and rollback.

---

## 7) Quality gates to run post-fix

- Build: `cargo check` (workspace) should PASS.
- Tests:
  - Unit/integration tests for HELLO → registration → send/receive path in both apps.
  - A small test that proves components receive lifecycle events from the same `PeerManager` that `ConnectionManager` uses.
- Grep checks:
  - No forbidden deps in Main actor crates.
  - No usages of deprecated façade messages in non-test code.

---

## 8) Verdict

Substantial progress toward SOLID and the Three-Actor pattern is evident in the staged changes. With two targeted fixes (single PeerManager instance per app, explicit Router channel registration), plus light CI wiring and deprecation clean-up, this will be safe to merge and materially improve the architecture. Without those fixes, runtime behavior is at high risk (missing events, missing routing).
