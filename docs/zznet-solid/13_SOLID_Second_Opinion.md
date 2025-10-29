# ZZNet SOLID Refactor – Skeptical Second Opinion (Code-First)

Date: 2025-10-28
Author: Independent audit (second opinion)
Status: Advisory – non-binding but code-backed
Scope: Validate whether the current implementation (not docs) follows SOLID and the Three-Actor pattern; identify disagreements with prior analysis; recommend concrete, minimal-risk corrections.

---

## 1) Motivation and methodology

This report is intentionally skeptical and code-first:
- I did not trust earlier claims or comments. I traced the code under `src/net/*` and key components.
- I reviewed the actual contents and boundaries of:
  - `zznet-session` (actor.rs, coordinator.rs)
  - `zznet-peer-manager` (lib.rs, peer_state.rs)
  - `zznet-router` (lib.rs, peer_channels.rs)
  - Representative component: `zzintent-config` (actor.rs, network_manager.rs, Cargo.toml)
- I validated responsibilities by what each module does, not by how it describes itself.

Outcome: a nuanced picture—substantial progress, but several architectural promises remain unfulfilled in code.

---

## 2) What changed in reality (brief inventory)

- `zznet-peer-manager` now owns peer identity/role/lifecycle state and broadcasts lifecycle events. It does not touch routing or rooms. This is a clean control-plane module.
- `zznet-router` owns channel registration, room membership/negotiation, and byte routing to room handlers via `PeerChannels`. It doesn’t use roles/auth.
- `zznet-session` is a small wrapper: an Actix `PeerManagerActor` and a thin `SessionCoordinator` that wires `PeerManager` + `Router` (and performs rollback on failures across planes).
- Components still import `zznet-session::PeerManagerActor` and some Main actors hold `Room<...>`—exposing network knowledge at the business layer.

---

## 3) SOLID assessment (strict)

I’m applying literal SOLID, judged only by the code.

### 3.1 Single Responsibility Principle (SRP)

- `zznet-peer-manager`: PASS. Single reason to change: evolution of peer state/lifecycle/eventing.
- `zznet-router`: PASS (with nuance). It handles data-plane concerns: channels, rooms, routing, negotiation. It does not handle roles/auth/business. Those are coherent data-plane responsibilities and give it one primary reason to change: routing/room concerns. Statelessness is not required by SRP.
- `zznet-session::SessionCoordinator`: BORDERLINE/FAIL. It owns orchestration/rollback across planes and lifecycle notifications—more than one reason to change (if either plane changes, this changes). Acceptable as a temporary façade during migration; not as a permanent layer.

Conclusion: SRP mostly satisfied in the split crates; weakened by the continuing existence of a coordination façade.

### 3.2 Interface Segregation Principle (ISP)

- `PeerManagerActor` exposes a wide, mixed surface:
  - Control-plane queries: role, identity, peer list, connected counts
  - Data-plane accessors: outbound sender, inbound subscription
  - Lifecycle mutations: add/remove/disconnect
- Clients that only need roles are forced to depend on everything.

Conclusion: ISP is violated. The actor remains a “fat interface.”

### 3.3 Dependency Inversion Principle (DIP)

- Inside the framework crates, dependency directions are good: core modules depend on abstractions (traits/channels), not on component code.
- At the component boundary, there is no trait abstraction; components depend on a concrete Actix actor and concrete message types.

Conclusion: DIP is weakened at the component boundary; still workable, but hurts testability and substitution.

### 3.4 Liskov Substitution Principle (LSP)

- No subtype hierarchies that violate expectations are evident. Interfaces are mostly traits with straightforward semantics. No LSP red flags found.

### 3.5 Open/Closed Principle (OCP)

- With the split, the core crates can be extended internally without modifying callers; however, as long as components import the façade actor, extension goes through that surface. OCP is acceptable but not ideal until the façade is retired or narrowed.

---

## 4) Three-Actor pattern audit (strict isolation test)

Requirement (code reality, not comments): Main actors must be network-oblivious. They should not:
- Know about peers, rooms, or channels
- Depend on `zznet-session`/`zznet-peer-manager`/`zznet-router`

Observed:
- `IntentConfigActor` holds an `Addr<PeerManagerActor>` and a `Room<...>`; this is direct network knowledge.
- Component `Cargo.toml` still lists `zznet-session` with a “temporary” comment.

Conclusion: Pattern is not enforced. Business actors still know about network details.

---

## 5) Where I agree vs. disagree with prior report (12_SOLID_Refactor_Reality_Check)

Areas of agreement:
- ISP is still violated by the façade/actor surface.
- The Three-Actor isolation is broken: Main actors depend on network infrastructure and room types.
- Lack of enforcement (no CI or trait boundary) allows architectural drift.

Where I disagree or add nuance:
- “Router must be a stateless byte-forwarder” is too strict. It’s reasonable (and common) for the data plane to manage room membership and route bytes to the right logical channel. What matters is that it remains free of auth/role/business semantics. The current `zznet-router` appears compliant with that boundary.
- “PeerChannels is just PeerSession renamed”: Partly true historically, but the key question is responsibility. Today, `PeerChannels` contains room/channel routing concerns and not peer identity/auth. That’s a meaningful split of duties and is consistent with a data-plane.

---

## 6) Root causes of remaining gaps (my view)

- The façade stuck: `zznet-session` still provides a convenient, broad actor surface. Teams kept using it instead of adopting narrower interfaces, so the split didn’t reach the call sites.
- No narrow traits at the boundary: Components have no `PeerRegistry` and `MessageRouter` traits to depend on—only the concrete actor.
- No enforcement: Dependency rules for Main actors (no `zznet-*` infrastructure deps) are not enforced by CI.
- Orchestration layer not retired: A coordinator/facade continues to centralize responsibilities (rollback/events) that should live closer to composition root or be explicitly two-step operations.

---

## 7) Recommendations (prioritized, minimal-risk)

1) Split the façade by interface (fix ISP)
- Define two narrow interfaces in `zznet-api`:
  - `PeerRegistry` (control plane): peer role/identity queries, peer lists, lifecycle events subscription
  - `MessageRouter` (data plane): send to peer/room, get outbound sender, inbound subscription
- Provide adapters over the real `PeerManager` and `Router` to implement these traits (thin shims; can live in `zznet-session` temporarily but marked deprecated).
- Deprecate façade methods that belong to the other plane (e.g., data-plane methods on the control-plane actor).

2) Enforce Three-Actor isolation
- Remove `PeerManagerActor` and `Room<...>` from Main actors. All network knowledge moves into the component’s NetworkManager actor.
- Add CI checks that forbid Main-actor crates from depending on `zznet-session`, `zznet-peer-manager`, or `zznet-router`. Permit only `zznet-api` and component-local code.

3) Tame or retire `SessionCoordinator`
- Preferred: push orchestration into the application composition root (startup wiring), performing two explicit operations (register in peer-manager; register channels in router) with clear error handling. This removes the need for a central rollback layer.
- Alternative: keep `SessionCoordinator` explicitly as a temporary façade and aggressively narrow its surface (delegation-only, no mixed-plane helpers).

4) Codify “data-plane” scope
- Document in code (module-level docs) that `zznet-router` is allowed to:
  - Manage per-peer room membership and route bytes to room handlers
  - Provide per-peer inbound broadcast and outbound sender access
  - Never touch auth/roles/business rules
- This avoids churn from philosophical disagreements and builds shared understanding.

5) Improve testability at boundaries
- Provide mock implementations of `PeerRegistry` and `MessageRouter` for component unit tests so components don’t need an Actix system to test business logic.

---

## 8) Risk and effort

- Interface split (traits + adapters): Low-to-moderate. Mostly new files and targeted refactors. High return: enforces ISP and improves DIP/testability.
- Main-actor isolation: Moderate, affects a few components but can be incremental: move fields/calls to NetworkManager first, then flip dependencies.
- CI enforcement: Low. A small script that scans Cargo.toml for forbidden dependencies in listed crates.
- Coordinator retirement: Moderate. Move orchestration to composition root and update call sites; can run alongside traits/adapters refactor.

---

## 9) Final verdict (second opinion)

- The split achieved real progress: control/data responsibilities are separated in code; peer-manager owns identity/lifecycle; router owns room/channel routing; that’s good SRP for the core crates.
- The biggest issues now are at the edges: a fat façade (ISP), leaky business actors (Three-Actor rule broken), and lack of enforceable boundaries (DIP in practice).
- I recommend focusing on interface boundaries and enforcement rather than re-litigating router scope. Codify the acceptable data-plane responsibilities and move on.

If you want, I can produce a concise checklist (“Definition of Done”) for the above steps next—without touching any code.

---

## 10) Definition of Done — enforceable checklists

This section translates the recommendations into concrete, verifiable checklists. A task is Done only when every box in its subsection is checked and quality gates pass.

### 10.1 Split the façade by interface (ISP fix)

- [ ] Add `PeerRegistry` and `MessageRouter` traits to `zznet-api` with narrow method sets
  - [ ] `PeerRegistry`: get_peer_role, get_peer_identity, peers_with_role, peer_ids/get_connected_count, subscribe_events
  - [ ] `MessageRouter`: send_to_peer(room), broadcast_to_peers(room), peer_sender, subscribe_peer_inbound
- [ ] Provide thin adapters that implement these traits
  - [ ] Control-plane adapter over `zznet-peer-manager`
  - [ ] Data-plane adapter over `zznet-router`
- [ ] Deprecate mixed-plane methods on the façade
  - [ ] Mark `GetPeerSender` and `SubscribePeerInbound` on `PeerManagerActor` as deprecated with migration notes to `MessageRouter`
- [ ] Migrate one pilot component to the traits-only boundary
  - [ ] Replace imports of `zznet-session` with trait-based adapters
  - [ ] Add unit tests using mocks (no Actix system required)
- [ ] Quality gates
  - [ ] Build PASS (workspace)
  - [ ] Tests PASS (workspace)
  - [ ] Grep PASS: no new usages of deprecated façade methods in non-test code

### 10.2 Enforce Three-Actor isolation

- [ ] Remove network knowledge from Main actors
  - [ ] No `PeerManagerActor` field or type in Main actor structs
  - [ ] No `Room<...>` or room-channel fields in Main actor structs
- [ ] Move all network wiring to each component’s NetworkManager
  - [ ] NetworkManager owns per-peer actors and uses the new traits
- [ ] Enforce dependency rules via CI
  - [ ] CI check that Main-actor crates do NOT depend on `zznet-session`, `zznet-peer-manager`, or `zznet-router`
  - [ ] CI allows only `zznet-api` (plus component-local crates)
- [ ] Add at least one unit test proving the Main actor compiles and runs with mocked trait implementations only
- [ ] Quality gates
  - [ ] Build PASS
  - [ ] Tests PASS (including component unit tests without Actix)

### 10.3 Tame/retire `SessionCoordinator`

- [ ] Preferred: move orchestration to the application composition root
  - [ ] Replace single `add_peer(state, channels)` call with explicit two-step calls: `peer_manager.add_peer(state)` and `router.register_peer(channels)` with rollback at the app layer
  - [ ] Remove coordinator rollback logic from library code
- [ ] Alternative (temporary): mark `SessionCoordinator` as deprecated and reduce to delegation-only (no cross-plane logic)
- [ ] Quality gates
  - [ ] Build PASS
  - [ ] Tests PASS (integration tests cover add/remove/disconnect flows)

### 10.4 Codify data-plane scope (avoid churn)

- [ ] Add module-level docs in `zznet-router` clarifying allowed responsibilities:
  - [ ] Allowed: channel registration, room membership/negotiation, inbound broadcast, routing bytes to room handlers, outbound send
  - [ ] Not allowed: roles/auth/business logic
- [ ] Static check: `zznet-router` must not import role/auth/business types (grep in CI)
- [ ] Ensure tests for room negotiation and routing live with `zznet-router`
- [ ] Quality gates: Build PASS, Tests PASS

### 10.5 Improve testability at boundaries (DIP in practice)

- [ ] Provide mock implementations for `PeerRegistry` and `MessageRouter` (in `zzping-test-utils` or `zznet-api` dev features)
- [ ] Convert one component’s unit tests to use mocks (no Actix system)
- [ ] Add an example test demonstrating trait-based injection for NetworkManager
- [ ] Quality gates: Tests PASS (unit tests run without starting an actor system)

### 10.6 CI enforcement

- [ ] Add a CI job that fails when Main-actor crates depend on infrastructure crates
  - [ ] Script scans Cargo.toml for forbidden dependencies in `zzintent-config`, `zzmem-db`, `zzcollector-state` (extend list as needed)
- [ ] Add a CI job that fails when deprecated façade methods are used in non-test code
- [ ] Document the rules in `docs/zznet-solid/` and link from PR template
- [ ] Quality gates: CI PASS with the new checks enabled

---

## 11) Completion signal

When all subsections in 10.x are checked, the boundary is enforced and the architecture meets the stated SOLID and Three-Actor requirements in code (not just docs). At that point, `zznet-session` can either be deleted or reduced to a legacy compatibility crate with an explicit removal date.
