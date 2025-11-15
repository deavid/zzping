# TRole-as-String (Role newtype) — proposal & migration plan

Date: 2025-10-26

Summary
-------
This document explains the rationale for representing peer roles as a compact string (or small newtype wrapper) inside the zznet core (SessionManager / PeerSession) instead of using a generic `TRole: ApplicationRole` type parameter everywhere. It describes the proposed `Role` newtype, advantages, migration steps for an incremental Option A prototype, tests to run, and risks/mitigations.

Context & background
--------------------
 Mapping responsibility: application code continues to implement role->component mapping helpers (previously represented by `AuthRoleMapper`) / typed role enums (e.g., `AuthRole`) for convenience. Components convert the compact wire `Role` → component enum using mapping logic when typed semantics are needed.

```rust
// zznet_api::types
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Role(pub String);

impl Role {
    pub fn as_str(&self) -> &str { &self.0 }
}

impl From<&str> for Role {
    fn from(s: &str) -> Self { Self(s.to_string()) }
}
```

- Internals: change `PeerSession` and `SessionManager` to store `Option<Role>` for the authenticated role (instead of `Option<TRole>`), and make `SessionManager` non-generic over role type.
- Messages: change SessionManager-level messages that currently reference `TRole` to use `Role` or `String` at the zznet layer:
  - `GetPeerRole` → returns `Option<Role>`
  - `GetPeersWithRole` → accepts `Role`
  - `BroadcastToRole` → accepts `Role`
  - `AddPeer` keeps `PeerSession<Role>` or `PeerSession` with Role internals

- Mapping responsibility: application code continues to implement role->component mapping helpers (previously represented by `AuthRoleMapper`) / typed role enums (e.g., `AuthRole`) for convenience. Components convert the compact wire `Role` → component enum using mapping logic when typed semantics are needed.

Why this is better
-------------------
- Simpler core: removes the generic parameter from core data structures and actor messages, reducing compile-time complexity and code duplication.
- Natural boundary: core deals with what it actually observes on the wire (strings). Domain-specific validation/conversion remains in application code where domain knowledge lives.
- Easier cross-component queries: components can query `SessionManager` for peers with role "database" (string) without needing a SessionManager instance specialized to their enum type.
- Migration-friendly: can be done incrementally (add `Role` newtype and new non-generic message variants in parallel with the existing generic API).

Trade-offs & cons
-----------------
- Loss of compile-time guarantees: typed enums used by components (`IntentConfigPermission`, `MemDBPermission`, etc.) must be constructed from `Role` at runtime; bad/malformed/unknown role strings need explicit handling.
- Required updates: messages, actors, components, and tests across the repo need to be updated. Tests that used `PeerSession<MockRole>` must be adapted.

Design details
--------------
1. Role type

- Add `zznet_api::types::Role` as the canonical role representation.
- Provide `Serialize`/`Deserialize`, `Eq`/`Hash`, `From<&str>` and `as_str()` helpers.

2. PeerSession changes (internal prototype)

- Replace `peer_role: Option<TRole>` with `peer_role: Option<Role>`.
- Change `role()` to `pub fn role(&self) -> Option<&Role>` and `get_peer_role_cloned()` to `Option<Role>`.
- Update tests in `peer_session.rs` to use `Role::from("mock-role")` or direct string helpers.

3. SessionManager changes (internal prototype)

- Make `SessionManager` non-generic over TRole. Store `HashMap<PeerId, PeerSession>` (PeerSession holds Role internally).
- Change message types used by the SessionManager actor layer to the non-generic forms (string/Role).
- Keep the generic message signatures around initially (if desired) for incremental migration, but prefer introducing non-generic messages and migrating call sites.

4. Messages API

- Create new non-generic variants for the most commonly used messages (actor-facing):
  - `GetPeerRole` (returns `Option<Role>`)
  - `GetPeersWithRole { role: Role }` (returns Vec<PeerId>)
  - `BroadcastToRole { role: Role, room_id: RoomId, bytes: Vec<u8> }`

- Mark the old generic message types as deprecated (if kept) or remove them once migration finishes.

Migration plan (staged)
-----------------------
This is an incremental approach so local checks keep us honest.

Phase 0 - Prep (low risk)
- Add the `Role` newtype in `zznet_api::types` and helper conversions.
- Add unit tests for `Role` serialization and helpers.

Phase 1 - Prototype internals & tests (medium risk)
- Update `PeerSession` to use `Role` internally and update `peer_session.rs` tests.
- Keep `SessionManager<TRole>` intact for now; simply change the stored PeerSession type to `PeerSession<Role>` and add conversions where required. This allows tests in session_manager to be updated with less churn.
- Run full test suite and fix regressions in tests that referenced `PeerSession::<MockRole>`.

Phase 2 - SessionManager actor message migration (medium→higher risk)
- Add non-generic actor messages for role operations (GetPeerRole, GetPeersWithRole, BroadcastToRole) using `Role`.
- Migrate internal `SessionManager` logic to respond to the new messages.
- Update components and call sites to use new messages.
- Iterate until all code uses the new messages; remove old generic messages.

 Phase 3 - App-level mapping & docs (low risk)
 - Add mapping helpers for components: per-component utilities such as
   - `fn map_role_to_intent_config_permissions(role: &Role) -> Option<IntentConfigPermissions>`
   - `fn map_role_to_component_permissions(role: &Role) -> Option<ComponentPermissions>`
   or a generic application-provided map helper if desired. These helpers should be owned by the application (composition root) and unit-tested.
 - Update docs and templates to show how to map `Role` to component enums or permission structs.

Phase 4 - Clean-up
- Remove leftover generic TRole type parameters across the core and component examples.
- Update docs, design guides, and tests.

Testing & verification
----------------------
- Run the full test suite after each phase:

```bash
cargo nextest run
```

- Run clippy and fix warnings:

```bash
cargo clippy --all-targets -- -D warnings
```

- Use `grep` to find any remaining `SessionManager<`/`PeerSession<` generic instantiations referencing role generics during migration.

Risks & mitigations
-------------------
- Risk: large API churn causing many small compile fixes across components.
  - Mitigation: staged approach; prototype internals and tests first, then migrate actor messages, then apps.
- Risk: runtime surprises when mapping invalid role strings.
  - Mitigation: add tight conversion helpers and unit tests; require application-level mapping helper implementations to be explicit about accepted roles and to error on unknown values.
- Risk: docs mismatch and onboarding confusion.
  - Mitigation: update `docs/` templates and add an ADR-style note (this proposal) in `docs/trole-refactor/` (this file).

Backwards compatibility
----------------------
- Optionally keep the generic messages as shims that convert between string `Role` and a `TRole` using a mapper closure supplied at SessionManager startup — useful if you need a temporary compatibility layer.

Estimated effort
----------------
- Phase 0: minutes (add Role type + tests).
- Phase 1: 1–3 hours (change PeerSession internals and update unit tests).
- Phase 2: 3–8 hours (migrate actor messages and update call sites across components + tests).
- Phase 3/4: 1–3 hours (docs + polishing + local enforcement fixes).

Next actions (pick one)
----------------------
- I can implement Phase 0 + Phase 1 locally as a prototype: add `Role` newtype and update `peer_session.rs` + unit tests. Then run the tests and report back. (This is the recommended next step.)
- Or I can produce a full PR plan listing the concrete files to edit in Phase 2 so you can review the exact diffs before changes.

If you want me to proceed with the prototype (Phase 0 + 1), I'll implement the `Role` newtype and update `PeerSession` internals, then run `cargo nextest run` and `cargo clippy` and report results.

