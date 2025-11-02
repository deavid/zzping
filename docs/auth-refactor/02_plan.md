# Auth Refactor — Implementation Plan (from 01_vision)

This document turns the vision in `01_vision.md` into concrete, incremental changes to the codebase. The goal is to separate global identity (Role) from per-component capabilities (Permissions), with the application being the sole owner of the Role -> Permissions policy. Components stay role-agnostic and enforce only their own local permissions.

## Objectives and guardrails

- Clean separation:
  - Role (passport) is established at connection time in the network layer.
  - Each component defines its own Permissions struct with simple booleans.
  - The application maps Role -> Permissions for each component (composition root only).
- One-way translation at the boundary:
  - The NetworkManager receives Role once and immediately translates it into component-specific Permissions.
  - The per-peer NetworkActor holds immutable Permissions; it never sees or queries Role again.
- Keep zznet core auth-agnostic:
  - No global Permission type in zznet; keep `zznet_api::types::Role` as the only auth-related primitive at the framework boundary.
  - Avoid central "authorization services" or runtime permission fetches.
- Prefer compile-time safety: permissions as typed booleans, not strings.
- Realistic tests over raw coverage; validate behavior end-to-end.

## Current state (quick baseline)

- The handshake/peer identity path delivers a canonical `zznet_api::types::Role` into `zznet_router::RouterActor::OnPeerConnected`, which calls `RoomManager::create_for_peer(peer_id, role, room_id, ...)`.
- Components implement `RoomManager` and may currently ignore the role (e.g., `zznet-demo` `ComponentANetworkManager`), or will soon need to enforce authorization.
- `common/zzping-auth` defines an application-specific `AuthRole` and (deprecated) `AuthRoleMapper`. The deprecation note matches the vision: components should not know app roles.
- Some components (e.g., `zzintent-config`) have a features placeholder for `permissions` but do not yet expose a concrete `permissions` module.

Conclusion: The framework already passes Role to the manager at the right place. We need to add per-component Permissions, inject application policy into managers, and ensure NetworkActors use permissions only.

## Target architecture (minimal changes to core)

- Keep the `RoomManager` trait signature as-is: it receives `Role` (string wrapper). This keeps zznet core auth-agnostic and stable.
- For each component crate:
  - Add `permissions.rs` with a local, minimal boolean struct defining capabilities for that component.
  - Update its `NetworkManager` to accept a policy map: `HashMap<String, Permissions>`.
  - In `create_for_peer`, look up `self.permissions_map.get(role.as_str())`. If it returns `None`, return `Err(CreateError::InvalidPermission { room_id })`. If `Some(perm)`, create the NetworkActor with that `Permissions` (clone) and do NOT pass Role further.
  - Update the per-peer `NetworkActor` to store the immutable `Permissions` and perform simple local checks before delegating to the MainActor.
- In application binaries/services (composition root):
  - Define the master policy as simple `HashMap<String, Permissions>` per component.
  - When constructing each component’s `NetworkManager`, pass the policy map in its constructor.

This preserves the vision’s boundaries: Role ends at the manager; NetworkActors are permission-only; MainActors are security-agnostic and only receive authorized commands.

## API shapes (tiny contracts)

- Component-side `permissions.rs` example:
  - Inputs: none (static type)
  - Outputs: `Permissions` struct (booleans, `Copy` or `Clone`)
  - Error modes: none (mapping returns `Option<Permissions>` at the manager)

- Manager policy injection (simple map):
  - Constructor field: `permissions_map: HashMap<String, Permissions>`
  - Usage: `let perms = self.permissions_map.get(role.as_str()).cloned().ok_or(CreateError::InvalidPermission { room_id: room_id.clone() })?;`

- NetworkActor:
  - Field: `permissions: Permissions`
  - Behavior: gate inbound commands with `if self.permissions.can_xxx { ... } else { /* deny via Unauthorized */ }`

## Step-by-step implementation plan

### Phase 1 — Scaffolding and no-op integration (low risk)

1. zzintent-config (pilot component)
   - Add `src/components/zzintent-config/src/permissions.rs`:
     - Define `IntentConfigPermissions { can_read: bool, can_write: bool }`.
   - Update `IntentConfigNetworkManager`:
     - Add field `permissions_map: HashMap<String, IntentConfigPermissions>`.
     - Update constructor to require `permissions_map`.
     - In `create_for_peer`, perform `self.permissions_map.get(role.as_str())`; on `None` return `CreateError::InvalidPermission { room_id }`.
   - Update `IntentConfigNetworkActor`:
     - Add `permissions: IntentConfigPermissions` and enforce before forwarding any write-like operation.
     - Gate read/write paths minimally to keep behavior unchanged for allowed roles.

2. zznet-demo (documentation-quality exemplar)
   - Add `component_a::permissions` with something like `ComponentAPermissions { can_ping: bool, can_publish: bool }`.
  - Update `ComponentANetworkManager` to accept a `permissions_map` and pass permissions into `ComponentANetworkActor`.
   - Keep default policy permissive in demo to avoid breaking existing tests; we’ll add a deny case test.

3. Application wiring (composition root)
   - In `apps/zzping-database` and `apps/zzping-collector` services/builders, create policy `HashMap<String, Permissions>` for each component:
     - Example: for intent-config
       - `"client-admin"` → `{ can_read: true,  can_write: true }`
       - `"collector"`    → `{ can_read: true,  can_write: false }`
       - Roles not in the map are denied.
   - Pass these maps into component builders/constructors that create their NetworkManagers.

Deliverables:
- Code compiles, all existing tests pass.
- New tests compile but can be temporarily permissive if managers default to allow.

### Phase 2 — Enforce permissions and add tests

4. Enforce in NetworkActors
   - Implement the actual gating logic in the per-peer NetworkActors for pilot components.
   - Ensure MainActors remain role/permission agnostic.

5. Add realistic integration tests
   - Builder-level tests (app black-box): verify that a `collector` role cannot write config but can read; `client-admin` can write.
   - Framework-internals tests (manual wiring): exercise `RouterActor` → `RoomManager` → `NetworkActor` path, assert unauthorized requests are rejected locally.
   - Reuse existing mock transport and harness utilities.

6. Align docs and remove confusing leftovers
   - `common/zzping-auth`: keep `AuthRole` and helpers; leave `AuthRoleMapper` deprecated. Add a note pointing to this plan and the composition-root mapping pattern.
   - Ensure `zznet` crates stay auth-agnostic in public API and docs.

Deliverables:
- Deny-path tests in demo and pilot components.
- Updated docs for the component and app mapping functions.

### Phase 3 — Roll out to remaining components

7. Replicate the pattern
   - For each component under `src/components/*` that participates in networking, add its own `permissions.rs` and inject policy into its `NetworkManager`.
   - Update managers and network actors to enforce.

8. Applications define full master policy
  - In `zzping-database`/`zzping-collector`, centralize per-component policy maps next to service wiring (e.g., `auth_policy.rs`).
  - Optionally factor common patterns into small helpers within the application crates (not in zznet core).

9. Backward-compatibility and toggles
   - Initially keep policies permissive for components without clear semantics and tighten once tests are in place.
   - Maintain a simple feature flag or config switch in applications to turn enforcement on/off per component during migration.

Deliverables:
- All networked components switched to permissioned NetworkActors.
- Application crates are the only place that map Role->Permissions.

## Tests and validation

- Unit tests: lightweight checks that policy maps contain expected entries and that lookups enforce behaviors.
- Integration tests (framework-level): realistic wiring using mock transports to assert that unauthorized operations are rejected by the `NetworkActor` without consulting any role service.
- Builder tests (app-level): black-box behavior across service boundaries using existing harness; assert that permitted/denied flows match policy.
- Static checks: a grep for usages of `Role` inside component `NetworkActor`s should return none. All references to Role in components should be limited to `NetworkManager::create_for_peer`.

Quality gates (PASS required):
- Build and typecheck across workspace.
- All existing tests remain green; new tests added incrementally per component.

## Incremental migration strategy

- Start with `zzintent-config` and `zzpinger` as pilots (clear read/write semantics).
- Land Phase 1 (scaffolding + no-op integration) behind permissive policies.
- Add enforcement + tests in Phase 2 for those pilots.
- Roll out Phase 3 to the rest with the same template.

## Risks and mitigations

- API churn in components:
  - Mitigate by keeping `RoomManager` unchanged and introducing policy injection via constructors.
- Overly broad policies during rollout:
  - Keep defaults permissive; add deny-path tests before tightening.
- Coupling to app code:
  - Only the application crates provide the policy maps. Components depend only on their local `Permissions` type, not on `AuthRole`.

## Work items checklist (tracked per component)

For each component in `src/components/*`:
- [ ] Add `permissions.rs` with boolean fields.
- [ ] Update `NetworkManager` to accept `HashMap<String, Permissions>`.
- [ ] In `create_for_peer`, translate Role → Permissions via `permissions_map.get(role.as_str())` and error on `None`.
- [ ] Update `NetworkActor` to store `Permissions` and gate actions.
- [ ] Add or update integration tests for permit/deny paths.

For applications (`zzping-database`, `zzping-collector`):
- [ ] Implement master policy HashMaps for each component.
- [ ] Pass them to component builders.
- [ ] Add builder-level tests to validate end-to-end behavior.

## Example: intent-config

- Permissions
  - `IntentConfigPermissions { can_read_config: bool, can_write_config: bool }`
- Application mapping (illustrative):
  - `client-admin` → `{ read: true, write: true }`
  - `collector`    → `{ read: true, write: false }`
  - others         → `None` (deny access to the room)
- NetworkActor guards
  - Before performing a write: `if !self.permissions.can_write_config { return self.send_unauthorized(); }`
  - Reads always allowed when `can_read_config`.

## Non-goals (stay simple)

- No central authorization actor/service.
- No dynamic mid-session permission changes.
- No string-based permission checks.
- No global Permission type in zznet core.

## Why a HashMap policy (KISS)

- Simpler than function traits and dynamic dispatch; easy to read and reason about.
- Directly models a fixed mapping from known roles to permissions.
- Fast lookups with clear failure semantics (missing entry ⇒ deny).
- Keeps the framework and components free of advanced generics or trait objects for policy.

## Acceptance criteria

- Components’ NetworkActors are role-agnostic and only hold immutable, per-peer Permissions.
- All Role → Permissions logic lives in application crates and is injected at construction time.
- Unauthorized operations are rejected locally by NetworkActors without querying any role/identity.
- Tests cover both permitted and denied paths at framework-level and builder-level.

---

If desired, we can add a short `docs/auth-refactor/03_progress.md` later to track per-component rollout with links to tests and PRs.
