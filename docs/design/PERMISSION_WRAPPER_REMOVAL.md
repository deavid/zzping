# PermissionWrapper removal

Date: 2025-10-26

Summary
-------
This document explains the historical purpose of `PermissionWrapper<T>` in the `zzintent-config` component, why it has been removed from the codebase, and what to do next if any follow-up work is needed.

Background / Original intent
----------------------------
- The project uses a two-layer authorization model:
  1. Connection-level `Role` (canonical string/newtype) used during TLS/connection authentication.
  2. Component-level permission enums (e.g. `IntentConfigPermission`) used by individual components to decide whether a peer may perform actions or receive messages.

- `PermissionWrapper<T>` was introduced as a small adapter/newtype around a component's permission enum `T` so components could:
  - Use a single concrete type as the generic `TRole` parameter for `SessionManager<TRole>` and related actor interfaces.
  - Keep component-level role types separate from the core `Role` string; perform mapping at application boundaries.
  - Provide a single place to add conversions (e.g., mapping `Role` -> component permission) in future.

What the code actually did
-------------------------
- The implementation of `PermissionWrapper<T>` was minimal: it simply wrapped the inner `T` and delegated all role behavior to `T` (previously via `ApplicationRole`). With the `TRole` trait removed, the wrapper can delegate to application-owned inherent methods or mapping helpers.
- The wrapper derived `Clone`, `Copy`, `Hash`, `Serialize` and provided a `Deserialize` impl that forwarded to `T`.
- There was no conversion logic from `Role` to `PermissionWrapper<T>` implemented in the wrapper itself; that mapping was performed elsewhere (or left unaligned).

Why it was removed
------------------
- During the recent cleanup the wrapper was removed because it provided zero runtime behavior beyond delegation; the inner types already implemented the same traits and semantics.
- The wrapper had become a placeholder with no functional advantage in the code paths exercised by the running system and tests.
- Removing it simplifies the code: fewer layers of indirection and fewer types to reason about while the two-layer model remains achievable via explicit `Role -> permission` conversions in components.

What changed in code & docs
--------------------------
- `src/components/zzintent-config/src/permission_wrapper.rs` was removed.
- Component code that referenced `PermissionWrapper<T>` must have been updated to use `T` directly or to use the component's concrete permission enum (e.g. `IntentConfigPermission`) depending on the previously applied refactor.
- Design docs that referenced `PermissionWrapper<T>` were left as historical notes; teams should update templates to reflect the current, simplified model or adopt an explicit mapping strategy described below.

Migration notes & recommendations
---------------------------------
If you (the maintainer) want to keep the original two-layer conceptual model, but with useful, explicit mapping points, consider one of these approaches:

1) Reintroduce a purposeful adapter (recommended if you want a dedicated conversion point)
  - Implement a small, explicit adapter that encapsulates mapping logic from `Role` to component permission.
   - Example API:
    - `impl TryFrom<Role> for ComponentPermission` (via application mapper trait implementations)
     - `impl From<ComponentPermission> for ComponentPermissionWrapper` (if wrapper type is desired)
   - Benefit: a single targeted place to implement mapping and custom per-component rules.

2) Use component permission enums directly (current approach)
   - Keep `SessionManager<T>` parameterized by the concrete component permission type `T` (e.g. `IntentConfigPermission`).
  - Implement a `Role -> T` mapping for `T` so the connection/auth layer can map incoming peer `Role` to a `T` value at connection time. This keeps mapping explicit while avoiding an extra wrapper type.

3) Keep the wrapper as a purely documented convention (not recommended)
   - Leave it as a type alias or removed type but document the intent in component templates. This is only sensible if you intentionally want minimal surface area and prefer direct `T` usage.

Code changes checklist for a safe migration
-----------------------------------------
- [ ] Update component actor generics and builder types to use `T` (component permission enum) instead of `PermissionWrapper<T>`.
 - [ ] Ensure a `Role -> T` mapping is implemented for `T` so you can map the connection-level `Role` into component permissions on connection/handshake.
- [ ] Audit `SessionManager` startup wiring: if you create `SessionManager` as `Addr<SessionManager<AuthRole>>` but components expect `SessionManager<T>`, either:
    - Start the `SessionManager` polymorphically per-component (hard), or
    - Perform mapping at message/send time by converting `Role` -> `T` when answering `GetPeerRole`/`GetPeersWithRole` queries.
- [ ] Update docs/design templates (`docs/design/COMPONENT_TEMPLATE_GUIDE.md` and component templates) to remove references to `PermissionWrapper<T>` or replace them with the chosen pattern.

Testing & verification
----------------------
- Run the full test suite after changes:

```bash
cargo nextest run
```

- Verify there are no remaining references to `PermissionWrapper` in source or docs:

```bash
grep -R "PermissionWrapper" -n . || true
```

- If you reintroduce an adapter, add unit tests for the mapping logic (`Role` -> component permission) and round-trip serde tests if applicable.

Follow-ups
----------
- If you want a clean replacement that implements the original vision (two-layer model with an explicit mapping point), I can open a PR that adds a small, well-documented adapter type with `TryFrom<Role>`/mapping usages and the matching unit tests.
- If you want to fully remove any remaining references and update templates/docs, I can prepare a short PR that changes component templates and the design docs.

Questions/Notes
---------------
- The wrapper removal simplifies the code but does not prevent implementing explicit mapping; the `Role`→component mapping helper remains the correct place to map connection-level roles to component-level permissions.
- If you removed the type to reduce cognitive load, we should also update the design docs that still mention `PermissionWrapper<T>` to avoid confusion.

References
----------
- Component actor example that previously used wrapper: `src/components/zzintent-config/src/actor.rs`
- Note about wiring in `src/apps/zzping-database/src/service.rs`
- Related docs: `docs/design/COMPONENT_TEMPLATE_GUIDE.md`, `docs/design/ZZNet_Component_Framework_Vision.md`

