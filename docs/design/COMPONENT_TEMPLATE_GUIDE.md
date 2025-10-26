# Component Template Guide

This short guide explains the conventional structure and patterns used by components in this repository (for example, see `src/components/zzintent-config`). It exists to speed up onboarding and keep new components consistent and testable.

Goal
- Provide a minimal, working component layout that follows the project conventions.
- Describe common files, APIs, and tests so contributors can implement new components quickly.

High-level layout
- src/
  - components/<component-name>/
    - src/
      - actor.rs        -> Private actor implementation, state, message handlers
      - api.rs          -> Public API wrapper exposing ergonomic functions (uses Addr)
      - builder.rs      -> Builder pattern for creating/starting the actor
      - messages.rs     -> Message types (component data, commands, health types)
      - network_messages.rs -> Optional: messages that go over the network
      - permission_wrapper.rs -> Optional: small wrapper to adapt your role type
      - role.rs         -> Role config (Collector/Database style) and validation
      - tests/*.rs      -> Unit and integration tests (placed under the crate tests)

Required patterns
- Builder -> Start
  - Provide a `IntentConfigBuilder`-style builder that returns `Result<Addr<_>, Error>` from `start()`.
  - Validate role/config in `start()` and return `Err` for invalid setups instead of panicking.

- Actor encapsulation
  - Keep the actor implementation in `actor.rs` and implement `Handler<T>` for message types declared in `messages.rs`.
  - Prefer `Context<Self>` and `actix::spawn` for background async work.
  - Keep state private and provide `GetCurrent` / `GetHealth` messages for introspection.

- Messages & Serialization
  - Define component messages (`UpdateConfig`, `Subscribe`, etc.) in `messages.rs`.
  - Derive `Serialize`/`Deserialize` for message payloads that are sent over the network or persisted.
  - For persisted configuration, prefer RON for readability and stable diffs.

- Permission model
  - TBD - this has been refactored. Refer to other guides.

- Network integration (SessionManager)
  - If the component sends/receives messages to peers, accept a `SessionManager` in the builder.
  - Use `SessionManager::broadcast_to_room` with a per-send timeout when broadcasting.
  - Keep the `SessionManager` optional for unit tests (test ergonomics): document that production requires it and tests may allow behavior in debug mode only.

- Health & Observability
  - Expose a `GetHealth` message returning a compact health struct (subscriber count, successful/failed broadcasts, last broadcast timestamp).
  - If background tasks perform network sends, use atomic counters (`Arc<AtomicU64>`) so spawned tasks can update health without crossing actor context boundaries.

- Persistence
  - Only Database-like roles should persist state to disk. Use atomic rename (write to `.tmp` then rename) for atomic updates.
  - Return errors instead of panicking when persistence fails.

- Tests
  - Prefer small, fast unit tests that exercise actor handlers using `#[actix::test]` and the `ntest::timeout` attribute to keep runs quick.
  - Use `tempfile::tempdir()` for file-backed tests.
  - For network-related tests, create lightweight SessionManager instances and either connect a `PeerSession` or deliberately leave it disconnected to exercise failure paths.
  - Add an integration test that exercises the component with a small in-memory `SessionManager` (see `zzintent-config` tests for examples).

Quick checklist for new components
- [ ] `messages.rs` created with payloads and Message derives
- [ ] `actor.rs` implemented with private state and handlers
- [ ] `builder.rs` with `start() -> Result<Addr<_>, Error>`
- [ ] `api.rs` small ergonomic wrapper for callers
- [ ] Tests covering basic flows (subscribe, update, persistence) and at least one failure path
- [ ] `CONTRIBUTING.md` mentions any project-specific testing caveats

Examples and references
- The `zzintent-config` component is a complete example. See:
  - `src/components/zzintent-config/src/actor.rs`
  - `src/components/zzintent-config/src/messages.rs`
  - `src/components/zzintent-config/src/builder.rs`
  - `src/components/zzintent-config/src/api.rs`
  - `src/components/zzintent-config/src/*.rs` tests

How to run tests locally
```bash
# Run component tests (debug builds are default and recommended for some auth tests)
cargo test -p zzintent-config --lib

# Run the network/session tests
cargo test -p zznet-session --lib
```

Where to put questions
- If you're unsure about role semantics or room naming, open an issue or PR and tag maintainers listed in the `AUTHORS` file.

License and contribution
- Follow the project's CONTRIBUTING.md and code of conduct for PRs and code style.

---
Small, focused, and designed to get a component started in under 30 minutes.
