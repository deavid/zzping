# Auth Refactor — Rollout Progress

Short template to track the migration from Role-at-runtime to per-component Permissions with app-owned policy maps.

## How to use this file

- Update the table when a step lands. Keep notes brief and link PRs.
- Status values: TODO | In-Progress | Done | N/A
- “App policy wired” indicates which application(s) provide the policy `HashMap<String, Permissions>` for that component.

## Global milestones

- [x] Phase 1 (Scaffolding): permissions.rs + managers accept `HashMap<String, Permissions>` for pilot components
- [x] Phase 2 (Enforcement + Tests): NetworkActors enforce; builder/framework tests cover permit/deny
- [x] Phase 3 (Full rollout): all networked components migrated; apps define all policy maps

## Per-component checklist

| Component | permissions.rs | Manager uses HashMap | NetworkActor enforces | Builder tests | Framework tests | App policy wired (DB/Collector) | Status | PR / Notes |
|---|---|---|---|---|---|---|---|---|
| zzintent-config | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Done | Builder and framework tests verify policy enforcement; zzping-database test validates client-admin (RW) vs collector (RO) |
| zzcollector-state | ✅ | ✅ | ✅ |  |  | ✅ | Done | Database: collector sends heartbeats, admin queries; permissions enforced in NetworkActor |
| zzpinger | ✅ | ✅ | ✅ |  |  | ✅ | Done | Collector: can update targets; permissions enforced in NetworkActor |
| zznet-demo (ComponentA) | ✅ | ✅ | ✅ | N/A | ✅ | ✅ | Done | Tests verify permissions enforcement |

## Snippet (for application wiring)

- Example: intent-config policy map in `zzping-database/src/service.rs`

```rust
let mut intent_config_policy = std::collections::HashMap::new();
intent_config_policy.insert(
    "client-admin".to_string(),
    IntentConfigPermissions { can_read: true, can_write: true },
);
intent_config_policy.insert(
    "collector".to_string(),
    IntentConfigPermissions { can_read: true, can_write: false },
);
// Pass `intent_config_policy` into the component's NetworkManager constructor
```

## Notes

- Roles not present in a policy map are denied by default (simple, explicit fail-closed behavior).
- Components must not import app `AuthRole`; they only receive their local `Permissions` at NetworkActor construction time.
