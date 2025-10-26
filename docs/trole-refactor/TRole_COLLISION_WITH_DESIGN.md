# Why current TRole usage collides with the intended component-permissions design

Date: 2025-10-26

This note collects concrete findings from the codebase that show why the current `TRole`-generic design in the network core (`zznet-session`) conflicts with the project's stated design goal: "components should not deal in roles, they should deal in permissions and let application-level code map roles→permissions."

Summary (short)
----------------
- The network core (`PeerSession<TRole>` and `SessionManager<TRole>`) stores and exposes typed `TRole` values and exposes actor messages that are generic over `TRole` (e.g., `GetPeerRole<TRole>`, `GetPeersWithRole<TRole>`).
- At least one component (zzintent-config) depends on these typed TRole APIs to perform authorization checks against a component permission enum (it calls `GetPeerRole` and `GetPeersWithRole` and expects to receive component-enum values).
- Apps currently create `SessionManager<AuthRole>` at startup (connection-level roles) and pass that Addr into components — producing a type alignment problem when components expect a different role enum (component permission enum) or a wrapper type.
- Practical consequence: the network core is tightly coupled to component enums via generics, which contradicts the goal of making the core auth-agnostic and forcing role→permission mapping into the application layer.

Concrete evidence (code pointers)
---------------------------------
- Core storage and API
  - `src/net/zznet-session/src/peer_session.rs`
    - `pub struct PeerSession<TRole> { peer_role: Option<TRole>, ... }`
    - `pub fn role(&self) -> Option<&TRole>` and `pub fn get_peer_role_cloned(&self) -> Option<TRole>`
  - `src/net/zznet-session/src/session_manager.rs`
    - `pub struct SessionManager<TRole> { peers: HashMap<PeerId, PeerSession<TRole>>, ... }`
    - `Handler<GetPeerRole<TRole>>` / `Handler<GetPeersWithRole<TRole>>` / `Handler<BroadcastToRole<TRole>>` implemented for the actor.
  - `src/net/zznet-session/src/messages.rs` defines many messages generic over `TRole`.

- Component usage
  - `src/components/zzintent-config/src/actor.rs`
    - Calls `session_manager.send(GetPeersWithRole { role: receive_role })` where `receive_role` is a component-specific enum wrapped (previously `PermissionWrapper<IntentConfigPermission>` or `IntentConfigPermission`). It relies on typed return values to decide authorization.
    - Calls `session_manager.send(GetPeerRole::new(peer_id))` and then inspects the returned TRole value to decide whether to accept a config change.
  - `src/components/zzmem-db` and `src/components/zzcollector-state`
    - Accept `Addr<SessionManager<T>>` in builder/actor wiring (for room auto-registration) but **do not** actively inspect TRole in their core logic — they primarily use SessionManager for wiring and message routing.

- App wiring mismatch
  - `src/apps/zzping-database/src/service.rs` currently constructs `SessionManager::<AuthRole>::new(...).start()` and passes `Addr<SessionManager<AuthRole>>` to components (see `create_builders()` comment where it notes "IntentConfig SessionManager wiring needs type alignment (AuthRole vs PermissionWrapper<IntentConfigPermission>)").
  - That means the running `SessionManager` is instantiated with a connection-level role type (`AuthRole`), while some components expect a different typed role form (component permission enums), producing a type-level mismatch.

- Tests and helpers
  - Tests construct `PeerSession<MockRole>` and `SessionManager::<MockRole>` in many places. This indicates tests rely on the generic TRole API for direct, typed interactions.

Why this collides with the intended design
-----------------------------------------
The intended design repeatedly states that components should operate on component permissions (their own enums) and not on global connection-level roles. The current code violates that separation for several reasons:

1. Core exposes typed TRole across the actor boundary
   - By making `GetPeerRole`/`GetPeersWithRole` generic, the core forces callers to choose a TRole type at compile time. That effectively couples the network core to whatever enum the caller picks.
   - A component that wants to perform permission checks expecting its own enum must either:
     - Be given an `Addr<SessionManager<ThatEnum>>` (specialized SessionManager), or
     - Receive a mapped/wrapped role object from the SessionManager (e.g., `PermissionWrapper<T>`), or
     - Ask the SessionManager for a connection-level role and perform mapping locally — but the codebase currently uses the typed message forms, making this third option uncommon.

2. Apps instantiate SessionManager with connection-level role types
   - The apps create `SessionManager<AuthRole>`. If components expect `SessionManager<ComponentPermission>`, there is an unavoidable type mismatch.
   - To work around this mismatch the code previously introduced `PermissionWrapper<T>` and a `SessionManagerLike` adaptor (legacy), and currently uses a hybrid pattern in places (comments and TODOs reference `Arc<Mutex<SessionManager<PermissionWrapper<T>>>>` and hybrid Arc/Mutex usage). This shows the tension: runtime needs a single `SessionManager` instance, but generics want it to be typed differently per component.

3. Permission checks leak into component-network interactions
   - `zzintent-config` directly asks the SessionManager for peers with a particular *component permission enum* and for a peer's typed role — then makes authorization decisions inline.
   - If the core exposed only a canonical role string or id, components would be forced to perform explicit mapping from that role id into their own permission enum via `AuthRoleMapper` or `from_cn` style logic — which is the intended separation of responsibilities.

Consequences in practice
------------------------
- Tight coupling: core <-> component type coupling increases complexity and slows future changes (adding a new component permission type requires wiring/choosing a TRole specialization or wrapper).
- Type friction at startup: apps that create SessionManager with `AuthRole` must either convert to component types or pass special wrappers; comments in `service.rs` indicate this friction is known and not fully resolved.
- Tests rely on typed mocks: many tests are written against `PeerSession<MockRole>`; migrating to a string-based Role core would require updating tests but would clarify responsibilities.
- Legacy/temporary workarounds: the codebase contains comments and leftover patterns (SessionManagerLike, Arc/Mutex usage, PermissionWrapper) that were introduced to bridge the mismatch — these add maintenance burden and potential for regression.

Bottom line
-----------
- The current `TRole` generic design in the network core directly conflicts with the declared architecture where components should only see component-specific permissions and role→permission mapping should be done by the application auth layer.
- The practical result is a type-level coupling and several ad-hoc bridging artifacts in the codebase (wrappers, comments, hybrid Arc/Mutex usage, and app-level notes), which increases cognitive and maintenance cost.

Minimal corrective principle (policy, not migration steps)
--------------------------------------------------------
- At runtime/zznet boundary, treat roles as a compact canonical identifier (string or small `Role` newtype). Keep the network core auth-agnostic.
- Let application-level code (zzping-auth / component `AuthRoleMapper`) convert the canonical role id into component permissions where needed. Components should assert permissions only against their own permission enums.

If you want a prototype to prove the new boundary (Role newtype in zznet core + component mapping), I can implement Phase 0+1 (add `Role` and update `PeerSession` internals) and run tests. But this document focuses only on findings: where and why the code currently collides with the intended architecture.

zzintent-config: TRole usage, purpose, and impact
-------------------------------------------------
The `zzintent-config` component is the clearest, concrete consumer of typed `TRole` values in the codebase. Below is a focused description of what it does with roles and what we would lose if the core stopped exposing role information entirely (i.e., removed any API that returns roles or filters peers by role) without offering a replacement.

What `zzintent-config` uses TRole for
- Authorization for config changes: when a peer requests a configuration change (`RequestConfigChange`), `zzintent-config` queries the `SessionManager` using `GetPeerRole` to obtain that peer's role and then checks whether the returned `TRole` grants update permission (via `PermissionCheck<T>::has_update_permission`). If the role is missing or fails the check, the change is rejected and an Error message is sent back.
- Targeted broadcasts: when the Database role needs to broadcast the latest config to interested collectors, `zzintent-config` calls `GetPeersWithRole` with the concrete permission value that represents "receive config updates". It then iterates the returned peer IDs and sends the ConfigUpdate to each using `SendToRoom`/SessionManager APIs.
- Convenience & type-safety: by receiving typed component permission values directly from SessionManager, `zzintent-config` can do equality comparisons and pattern matching on enum variants rather than parsing strings or calling mappers.

Why this mattered (purpose)
- Simplicity in authorization flow: `zzintent-config` treats SessionManager as the authoritative source of a peer's role in the application model (component permission enum), so it can make immediate authorization decisions without performing its own mapping.
- Efficient recipient discovery: `GetPeersWithRole` returns exactly the peers that hold a given component permission according to SessionManager's stored state, enabling direct broadcasts without per-peer mapping logic.

What we would lose if core role access were removed with no replacement
- Immediate typed authorization: `zzintent-config` would no longer be able to call `GetPeerRole` and receive a component enum value to feed into `has_update_permission`. It would have to request a raw role identifier (string/Role) and perform mapping locally before authorizing — an extra step.
- Direct typed peer filters: `GetPeersWithRole` would not be available in its typed form. To implement the same broadcast behavior the component would need to either:
  - Query `GetPeerIds` and then map/inspect each peer's role individually (extra round-trips or extra messages), or
  - Use a new string-based `GetPeersWithRoleStr` API and then map the returned Role strings into component permissions locally.
- Losing compile-time consistency: currently the typed flow makes the compiler help ensure the component and SessionManager agree on the shape of the permission (enum variant). Removing typed role access moves that agreement to runtime mapping and increases the chance of mismatches.
- Test changes & ergonomics: many unit/integration tests assume typed `PeerSession<IntentConfigPermission>` or the ability to mock typed roles. Tests would need rewrites to construct string roles and map them in test helpers.
- Potential for subtle bugs during migration: if components or apps interpret the on-wire role differently (different canonical strings, case/normalization issues), authorization behavior could change unexpectedly unless the mapping is standardized and well-tested.

Mitigations if we remove typed role access
- Provide a canonical `Role` newtype and a string-based `GetPeersWithRole` and `GetPeerRole` API so components can explicitly map Role→permission using `AuthRoleMapper`.
- Add helper utilities in each component crate to encapsulate Role→permission mapping and surface clear errors when mapping fails.
- Add tests that assert mapping invariants (e.g., `AuthRoleMapper` implementations accept the same canonical strings that the connection layer emits).

Net effect
- Removing typed `TRole` access without replacement would force `zzintent-config` and any other components that expect typed roles to add explicit mapping logic and per-peer lookups. That is possible and arguably correct from an architecture standpoint, but it is a non-trivial change that affects runtime behavior, tests, and developer ergonomics. It should be performed as an explicit migration with supporting APIs and tests rather than by silently removing role-returning APIs.
